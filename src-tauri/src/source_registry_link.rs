use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, Url};

pub const SOURCE_REGISTRY_LINK_EVENT: &str = "source-registry-link";

const LINK_SCHEME: &str = "openkara";
const ADD_REGISTRY_ACTION: &str = "add-registry";
const MAX_LINK_CHARS: usize = 4096;
const MAX_INVALID_LINK_DISPLAY_CHARS: usize = 256;
const MAX_PENDING_LINKS: usize = 8;

/// What an `openkara://` link asks the frontend to do. Links never act on
/// their own: an add request only opens the add-registry confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceRegistryLink {
    AddRegistry { url: String },
    Invalid { link: String },
}

pub fn parse_link(link: &str) -> SourceRegistryLink {
    match registry_url(link) {
        Some(url) => SourceRegistryLink::AddRegistry { url },
        None => SourceRegistryLink::Invalid {
            link: link.chars().take(MAX_INVALID_LINK_DISPLAY_CHARS).collect(),
        },
    }
}

fn registry_url(link: &str) -> Option<String> {
    if link.chars().count() > MAX_LINK_CHARS {
        return None;
    }
    let link = Url::parse(link).ok()?;
    let is_add_registry = link.scheme() == LINK_SCHEME
        && link
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case(ADD_REGISTRY_ACTION))
        && matches!(link.path(), "" | "/")
        && link.username().is_empty()
        && link.password().is_none()
        && link.port().is_none()
        && link.fragment().is_none();
    if !is_add_registry {
        return None;
    }
    let mut pairs = link.query_pairs();
    let (key, url) = pairs.next()?;
    if key != "url" || pairs.next().is_some() {
        return None;
    }
    let registry = Url::parse(&url).ok()?;
    let is_https_registry = registry.scheme() == "https"
        && registry.host_str().is_some_and(|host| !host.is_empty())
        && registry.username().is_empty()
        && registry.password().is_none();
    is_https_registry.then(|| registry.into())
}

#[derive(Default)]
pub struct PendingRegistryLinks(Mutex<VecDeque<SourceRegistryLink>>);

impl PendingRegistryLinks {
    fn push(&self, link: SourceRegistryLink) {
        let mut pending = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if pending.len() == MAX_PENDING_LINKS {
            pending.pop_front();
        }
        pending.push_back(link);
    }

    pub fn take(&self) -> Vec<SourceRegistryLink> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner)).into()
    }
}

/// Queues the links the app was opened with and tells the main window to
/// collect them. The queue covers a cold start, where the link arrives before
/// the frontend listens.
pub fn receive<R: Runtime>(app: &AppHandle<R>, links: impl IntoIterator<Item = Url>) {
    let pending = app.state::<PendingRegistryLinks>();
    let mut received = false;
    for link in links
        .into_iter()
        .filter(|link| link.scheme() == LINK_SCHEME)
    {
        pending.push(parse_link(link.as_str()));
        received = true;
    }
    if received {
        let _ = app.emit_to("main", SOURCE_REGISTRY_LINK_EVENT, ());
    }
}

/// Wires the deep-link plugin to the queue. Call after the plugin is
/// registered.
pub fn listen<R: Runtime>(app: &tauri::App<R>) {
    use tauri_plugin_deep_link::DeepLinkExt;

    app.manage(PendingRegistryLinks::default());
    let deep_link = app.deep_link();
    // Installers register the scheme for bundled builds; an AppImage has no
    // installer, so it registers itself each launch to follow its own path.
    #[cfg(target_os = "linux")]
    if app.env().appimage.is_some() {
        if let Err(error) = deep_link.register_all() {
            tracing::warn!("could not register the openkara:// link scheme: {error}");
        }
    }
    if let Ok(Some(links)) = deep_link.get_current() {
        receive(app.handle(), links);
    }
    let handle = app.handle().clone();
    deep_link.on_open_url(move |event| receive(&handle, event.urls()));
}
#[cfg(test)]
mod tests {
    use super::*;

    fn add(url: &str) -> SourceRegistryLink {
        SourceRegistryLink::AddRegistry {
            url: url.to_owned(),
        }
    }

    fn invalid(link: &str) -> SourceRegistryLink {
        SourceRegistryLink::Invalid {
            link: link.to_owned(),
        }
    }

    #[test]
    fn an_add_registry_link_names_the_encoded_https_url() {
        assert_eq!(
            parse_link(
                "openkara://add-registry?url=https%3A%2F%2Fsources.example.com%2Fregistry.json"
            ),
            add("https://sources.example.com/registry.json")
        );
        assert_eq!(
            parse_link("openkara://add-registry/?url=https://sources.example.com/r.json?v=2"),
            add("https://sources.example.com/r.json?v=2")
        );
    }

    #[test]
    fn links_without_one_https_registry_url_are_invalid() {
        for link in [
            "openkara://add-registry",
            "openkara://add-registry?url=",
            "openkara://add-registry?url=http%3A%2F%2Fsources.example.com%2Fr.json",
            "openkara://add-registry?url=file%3A%2F%2F%2Fetc%2Fpasswd",
            "openkara://add-registry?url=https%3A%2F%2Fuser%3Apass%40sources.example.com%2F",
            "openkara://add-registry?url=not%20a%20url",
            "openkara://add-registry?url=https%3A%2F%2Fa.example%2F&url=https%3A%2F%2Fb.example%2F",
            "openkara://install?url=https%3A%2F%2Fsources.example.com%2Fr.json",
            "openkara://add-registry/extra?url=https%3A%2F%2Fsources.example.com%2Fr.json",
            "openkara://add-registry?url=https%3A%2F%2Fsources.example.com%2Fr.json#confirm",
            "openkara://add-registry?url=https%3A%2F%2Fsources.example.com%2Fr.json&install=fixture",
            "https://add-registry?url=https%3A%2F%2Fsources.example.com%2Fr.json",
        ] {
            assert_eq!(parse_link(link), invalid(link), "{link}");
        }
    }

    #[test]
    fn an_oversized_link_is_invalid_and_shortened_for_display() {
        let link = format!(
            "openkara://add-registry?url=https%3A%2F%2Fsources.example.com%2F{}",
            "a".repeat(MAX_LINK_CHARS)
        );
        let SourceRegistryLink::Invalid { link: shown } = parse_link(&link) else {
            panic!("an oversized link must be invalid");
        };
        assert_eq!(shown.chars().count(), MAX_INVALID_LINK_DISPLAY_CHARS);
        assert!(link.starts_with(&shown));
    }

    #[test]
    fn received_links_wait_until_the_frontend_takes_them_once() {
        let app = tauri::test::mock_app();
        app.manage(PendingRegistryLinks::default());
        receive(
            app.handle(),
            [
                Url::parse(
                    "openkara://add-registry?url=https%3A%2F%2Fsources.example.com%2Fr.json",
                )
                .unwrap(),
                Url::parse("openkara://add-registry").unwrap(),
                Url::parse("file:///tmp/song.mp3").unwrap(),
            ],
        );
        let pending = app.state::<PendingRegistryLinks>();
        assert_eq!(
            pending.take(),
            vec![
                add("https://sources.example.com/r.json"),
                invalid("openkara://add-registry"),
            ]
        );
        assert!(pending.take().is_empty());
    }

    #[test]
    fn a_link_flood_keeps_only_the_newest_links() {
        let pending = PendingRegistryLinks::default();
        for index in 0..MAX_PENDING_LINKS + 3 {
            pending.push(add(&format!("https://sources.example.com/{index}.json")));
        }
        let kept = pending.take();
        assert_eq!(kept.len(), MAX_PENDING_LINKS);
        assert_eq!(kept[0], add("https://sources.example.com/3.json"));
    }
}
