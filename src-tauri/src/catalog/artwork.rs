//! Streaming listings carry an artwork key instead of downloaded artwork; the
//! webview loads each visible image through the `source-art` protocol. A
//! listing therefore returns as soon as its source answers, and images load in
//! parallel. Descriptor headers stay in the host and never reach the webview.

use crate::source_host::{Cancellation, ResourceDescriptor, SourceHost};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, UriSchemeResponder};

pub const SCHEME: &str = "source-art";
const CAPACITY: usize = 20_000;

#[derive(Default)]
pub struct ArtworkRegistry {
    inner: Mutex<(
        HashMap<String, (String, ResourceDescriptor)>,
        VecDeque<String>,
    )>,
}
impl ArtworkRegistry {
    /// Remembers where `source_id` serves this artwork and returns its key.
    pub fn register(&self, source_id: &str, descriptor: ResourceDescriptor) -> String {
        let identity = serde_json::to_vec(&(source_id, &descriptor.url, &descriptor.headers))
            .expect("artwork identity serializes");
        let key = crate::hash::hex_lower(Sha256::digest(identity));
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !guard.0.contains_key(&key) {
            guard.1.push_back(key.clone());
        }
        guard
            .0
            .insert(key.clone(), (source_id.to_owned(), descriptor));
        while guard.0.len() > CAPACITY {
            if let Some(oldest) = guard.1.pop_front() {
                guard.0.remove(&oldest);
            }
        }
        key
    }
    fn get(&self, key: &str) -> Option<(String, ResourceDescriptor)> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.0.get(key).cloned()
    }
}

fn cached_path(data_root: &Path, key: &str) -> PathBuf {
    data_root.join("source-artwork").join(format!("{key}.img"))
}

/// Cached artwork first; otherwise the source downloads it under its own
/// resource-origin rules and the bytes are cached for later sessions.
pub async fn load(
    registry: &ArtworkRegistry,
    host: &SourceHost,
    data_root: &Path,
    key: &str,
) -> Option<Vec<u8>> {
    if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let path = cached_path(data_root, key);
    if let Ok(bytes) = tokio::fs::read(&path).await {
        return Some(bytes);
    }
    let (source_id, descriptor) = registry.get(key)?;
    let bytes = host
        .load_resource(&source_id, descriptor, Cancellation::new())
        .await
        .ok()?;
    let parent = path.parent()?;
    if tokio::fs::create_dir_all(parent).await.is_ok() {
        let partial = parent.join(format!("{key}.{}.part", std::process::id()));
        if tokio::fs::write(&partial, &bytes).await.is_ok() {
            let _ = tokio::fs::rename(&partial, &path).await;
        }
    }
    Some(bytes)
}

fn content_type(bytes: &[u8]) -> &'static str {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'G', b'I', b'F', b'8', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        [_, _, _, _, b'f', b't', b'y', b'p', b'a', b'v', b'i', b'f', ..] => "image/avif",
        _ => "application/octet-stream",
    }
}

pub fn serve(app: &AppHandle, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let app = app.clone();
    let key = request.uri().path().trim_start_matches('/').to_owned();
    tauri::async_runtime::spawn(async move {
        let bytes = match app.path().app_data_dir() {
            Ok(data_root) => {
                load(
                    &app.state::<std::sync::Arc<ArtworkRegistry>>(),
                    &app.state::<SourceHost>(),
                    &data_root,
                    &key,
                )
                .await
            }
            Err(_) => None,
        };
        let response = match bytes {
            Some(bytes) => Response::builder()
                .header(header::CONTENT_TYPE, content_type(&bytes))
                .header(header::CACHE_CONTROL, "max-age=86400")
                .body(bytes),
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Vec::new()),
        };
        responder.respond(response.expect("artwork response builds"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_depend_on_source_url_and_headers_only() {
        let registry = ArtworkRegistry::default();
        let art = |url: &str, expires: Option<&str>| ResourceDescriptor {
            url: url.into(),
            headers: Default::default(),
            expires_at: expires.map(Into::into),
        };
        let key = registry.register("a", art("https://x.test/1.jpg", None));
        assert_eq!(key.len(), 64);
        assert_eq!(
            key,
            registry.register(
                "a",
                art("https://x.test/1.jpg", Some("2099-01-01T00:00:00Z"))
            )
        );
        assert_ne!(
            key,
            registry.register("b", art("https://x.test/1.jpg", None))
        );
        assert_eq!(registry.get(&key).unwrap().0, "a");
    }

    #[test]
    fn sniffs_common_image_types() {
        assert_eq!(content_type(&[0xFF, 0xD8, 0xFF, 0xE0]), "image/jpeg");
        assert_eq!(content_type(b"\x89PNG\r\n"), "image/png");
        assert_eq!(content_type(b"RIFF\0\0\0\0WEBPVP8 "), "image/webp");
        assert_eq!(content_type(b"<html>"), "application/octet-stream");
    }
}
