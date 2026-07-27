use crate::{
    commands::error::{CommandError, CommandResult},
    config::RegisteredLibrary,
    library::error::LibraryError,
    remote::errors::{
        remote_error_from_status, RemoteError, RemoteErrorKind, RemoteObjectMetadata,
        RemoteProviderCapabilities, RemoteResult,
    },
    remote::provider::ConditionalSource,
};
use base64::Engine;
use reqwest::{
    blocking::{Client, Response},
    header::ETAG,
    Method, StatusCode, Url,
};
use std::{
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom},
    path::Path,
};

fn base64_encode(input: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(input.as_bytes())
}

use super::types::{
    load_remote_credential, slugify_display_name, store_remote_credential,
    stored_webdav_server_url, RemoteAuthPayloadInput, StoredWebDavSecret, WebDavSecret,
    WebDavSessionData,
};

pub(crate) fn normalize_server_url(raw: &str) -> CommandResult<String> {
    let mut url = Url::parse(raw).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "invalid WebDAV server URL: {error}"
        )))
    })?;
    if !raw.ends_with('/') {
        let next_path = format!("{}/", url.path().trim_end_matches('/'));
        url.set_path(&next_path);
    }
    Ok(url.to_string())
}

pub(crate) fn normalize_webdav_root_path(raw: Option<&str>, fallback_display_name: &str) -> String {
    let candidate = raw.unwrap_or_default().trim().trim_matches('/');
    if candidate.is_empty() {
        format!("/{}", slugify_display_name(fallback_display_name))
    } else {
        format!("/{}", candidate)
    }
}

pub(crate) fn join_url(base: &str, relative: &str) -> CommandResult<String> {
    Url::parse(base)
        .and_then(|url| url.join(relative))
        .map(|url| url.to_string())
        .map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to join URL {base} + {relative}: {error}"
            )))
        })
}

pub(crate) fn remote_path_display_from_url(url: &str) -> String {
    Url::parse(url)
        .ok()
        .map(|url| {
            let host = url.host_str().unwrap_or("webdav");
            let path = url.path().trim_end_matches('/');
            format!("{host}{path}")
        })
        .unwrap_or_else(|| url.to_owned())
}

pub(crate) fn webdav_client() -> CommandResult<Client> {
    use crate::remote::net_policy::RetryPolicy;
    // WebDAV needs limited redirects in addition to the shared policy timeouts.
    let policy = RetryPolicy::default();
    let request_timeout = policy.read_timeout.max(policy.attempt_deadline);
    Client::builder()
        .connect_timeout(policy.connect_timeout)
        .timeout(request_timeout)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to create WebDAV client: {error}"
            )))
        })
}

pub(crate) fn webdav_send(
    client: &Client,
    method: Method,
    url: &str,
    username: &str,
    password: &str,
    if_match: Option<&str>,
    body: Option<Vec<u8>>,
) -> CommandResult<Response> {
    let mut request = client
        .request(method, url)
        .basic_auth(username, Some(password));
    if let Some(tag) = if_match {
        request = request.header("If-Match", tag);
    }
    if let Some(bytes) = body {
        request = request.body(bytes);
    }
    // Single-shot send. Callers that can rebuild the request should prefer
    // `crate::remote::send_with_retry` for transport retries; WebDAV helpers
    // still apply the shared connect/read timeouts via `webdav_client`.
    request.send().map_err(|_error| {
        tracing::trace!("WebDAV request to {url} failed");
        CommandError::from(LibraryError::Internal(
            "WebDAV request failed. Check the server URL and try again.".to_owned(),
        ))
    })
}

pub(crate) fn webdav_exists(
    client: &Client,
    url: &str,
    username: &str,
    password: &str,
) -> CommandResult<bool> {
    Ok(
        webdav_send(client, Method::HEAD, url, username, password, None, None)?.status()
            != StatusCode::NOT_FOUND,
    )
}

pub(crate) fn webdav_get_etag(
    client: &Client,
    url: &str,
    username: &str,
    password: &str,
) -> CommandResult<Option<String>> {
    let response = webdav_send(client, Method::HEAD, url, username, password, None, None)?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "WebDAV HEAD {url} failed with status {}",
            response.status()
        ))));
    }
    Ok(response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned))
}

pub(crate) fn ensure_webdav_collection_chain(
    client: &Client,
    server_url: &str,
    target_url: &str,
    username: &str,
    password: &str,
) -> CommandResult<()> {
    let server = Url::parse(server_url).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "invalid WebDAV server URL: {error}"
        )))
    })?;
    let target = Url::parse(target_url).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "invalid WebDAV target URL: {error}"
        )))
    })?;

    let server_segments = non_empty_path_segments(&server);
    let target_segments = non_empty_path_segments(&target);
    if !target_segments.starts_with(&server_segments) {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "WebDAV target URL {target_url} is not inside server URL {server_url}"
        ))));
    }

    let mut current_segments = server_segments;
    for segment in target_segments.iter().skip(current_segments.len()) {
        current_segments.push(segment.clone());
        let next_path = format!("/{}/", current_segments.join("/"));
        let mut current = server.clone();
        current.set_path(&next_path);
        let current_url = current.to_string();
        if webdav_exists(client, &current_url, username, password)? {
            continue;
        }

        let response = webdav_send(
            client,
            Method::from_bytes(b"MKCOL").expect("MKCOL should parse"),
            &current_url,
            username,
            password,
            None,
            None,
        )?;
        match response.status() {
            StatusCode::CREATED | StatusCode::METHOD_NOT_ALLOWED | StatusCode::CONFLICT => {}
            status => {
                return Err(CommandError::from(LibraryError::Internal(format!(
                    "failed to create WebDAV collection {current_url}: {status}"
                ))))
            }
        }
    }
    Ok(())
}

fn non_empty_path_segments(url: &Url) -> Vec<String> {
    url.path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn download_webdav_file(
    client: &Client,
    url: &str,
    destination: &Path,
    username: &str,
    password: &str,
) -> CommandResult<Option<String>> {
    let mut response = webdav_send(client, Method::GET, url, username, password, None, None)?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "failed to download {url}: {}",
            response.status()
        ))));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to create {}: {error}",
                parent.display()
            )))
        })?;
    }
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    // Stream the body to disk as it arrives instead of buffering the whole
    // file with `response.bytes()`. The buffered call imposed a single
    // total-body deadline; streaming makes the client timeout a per-read idle
    // timeout, so slow-but-steady links complete instead of failing at the
    // deadline (issue #205).
    let mut file = fs::File::create(destination).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to create {}: {error}",
            destination.display()
        )))
    })?;
    crate::remote::net_policy::stream_response_body(&mut response, &mut file).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to stream WebDAV response: {error}"
        )))
    })?;
    Ok(etag)
}

pub(crate) fn upload_webdav_file(
    client: &Client,
    url: &str,
    source: &Path,
    username: &str,
    password: &str,
) -> CommandResult<Option<String>> {
    let bytes = fs::read(source).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to read {}: {error}",
            source.display()
        )))
    })?;
    upload_webdav_bytes(client, url, bytes, username, password)
}

pub(crate) fn upload_webdav_bytes(
    client: &Client,
    url: &str,
    bytes: Vec<u8>,
    username: &str,
    password: &str,
) -> CommandResult<Option<String>> {
    let response = webdav_send(
        client,
        Method::PUT,
        url,
        username,
        password,
        None,
        Some(bytes),
    )?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "failed to upload {url}: {}",
            response.status()
        ))));
    }
    Ok(response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned))
}

/// Conditionally PUT bytes to a WebDAV URL with compare-and-swap semantics.
///
/// - `expected_revision = Some(etag)`: sends `If-Match: <etag>`. A mismatch
///   yields HTTP 412 → [`RemoteErrorKind::RemoteConflict`].
/// - `expected_revision = None`: sends `If-None-Match: *` (conditional-create).
///   A pre-existing object yields HTTP 412 → `RemoteConflict`.
///
/// Servers that omit stable ETags or ignore conditional requests remain
/// readable but are rejected for safe writes (the provider reports
/// `conditional_replace = false` when no ETag is available, so this function
/// is only reached when a stable ETag was observed via `stat`).
pub(crate) fn webdav_conditional_put(
    client: &Client,
    url: &str,
    bytes: Vec<u8>,
    username: &str,
    password: &str,
    expected_revision: Option<&str>,
) -> RemoteResult<RemoteObjectMetadata> {
    let mut request = client
        .request(Method::PUT, url)
        .basic_auth(username, Some(password));
    if let Some(etag) = expected_revision {
        request = request.header("If-Match", etag);
    } else {
        // Conditional-create: fail if the resource already exists.
        request = request.header("If-None-Match", "*");
    }
    let response = request.body(bytes).send().map_err(|_e| {
        RemoteError::new(
            RemoteErrorKind::NetworkUnavailable,
            format!("WebDAV conditional PUT to {url} failed"),
        )
    })?;

    let status = response.status();
    if !status.is_success() {
        // 412 Precondition Failed is the CAS-conflict signal.
        if status == StatusCode::PRECONDITION_FAILED {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteConflict,
                format!("WebDAV conditional PUT conflict at {url}"),
            ));
        }
        return Err(remote_error_from_status(status, "WebDAV conditional PUT"));
    }

    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let size = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    Ok(RemoteObjectMetadata {
        size,
        revision: etag,
    })
}

// ---------------------------------------------------------------------------
// Staged upload + server-side MOVE (PR#5)
//
// WebDAV servers vary in their support for partial PUT / Content-Range, so we
// do NOT claim `resumable_upload = true`. Instead, large uploads use a safe
// staging path: upload to `.openkara/staging/<op-id>/<filename>.part`, verify,
// then MOVE to the final path. The MOVE is server-side when the server
// supports it (most WebDAV servers do), avoiding a re-upload.
//
// The staging path is operation-scoped so concurrent operations do not
// collide. A changed provider_revision invalidates the staged partial — the
// caller discards it and starts a new staging upload.
// ---------------------------------------------------------------------------

/// Build the staging URL for an operation-scoped partial upload.
pub(crate) fn webdav_staging_url(
    root_url: &str,
    operation_id: &str,
    relative_path: &str,
) -> CommandResult<String> {
    // Sanitize the relative path into a single filename segment so the staged
    // object lives under `.openkara/staging/<op-id>/` without nested dirs.
    let flat_name = relative_path.replace('/', "_");
    join_url(
        root_url,
        &format!(".openkara/staging/{operation_id}/{flat_name}.part"),
    )
}

/// Upload bytes to the staging path. Returns the staging URL on success.
pub(crate) fn webdav_staged_upload(
    client: &Client,
    root_url: &str,
    operation_id: &str,
    relative_path: &str,
    bytes: Vec<u8>,
    username: &str,
    password: &str,
) -> CommandResult<String> {
    let staging_url = webdav_staging_url(root_url, operation_id, relative_path)?;
    // Ensure the staging collection chain exists.
    let server_url = {
        let parsed = Url::parse(root_url).map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "invalid WebDAV root URL: {error}"
            )))
        })?;
        // The server URL is the scheme + host + first path segment (the
        // share root). For staging we ensure the full chain.
        let staging_dir_url = join_url(root_url, &format!(".openkara/staging/{operation_id}/"))?;
        ensure_webdav_collection_chain(
            client,
            parsed.as_str(),
            &staging_dir_url,
            username,
            password,
        )?;
        parsed.to_string()
    };
    let _ = server_url;
    upload_webdav_bytes(client, &staging_url, bytes, username, password)?;
    Ok(staging_url)
}

/// Move a staged object to its final URL using a server-side MOVE. Most WebDAV
/// servers support MOVE; the destination is specified via the `Destination`
/// header.
pub(crate) fn webdav_move_staged_to_final(
    client: &Client,
    staging_url: &str,
    final_url: &str,
    username: &str,
    password: &str,
) -> CommandResult<Option<String>> {
    // webdav_send does not support custom headers, so we build the MOVE
    // request directly with the Destination + Overwrite headers.
    let response = client
        .request(
            Method::from_bytes(b"MOVE").expect("MOVE should parse"),
            staging_url,
        )
        .basic_auth(username, Some(password))
        .header("Destination", final_url)
        .header("Overwrite", "T")
        .send()
        .map_err(|_error| {
            CommandError::from(LibraryError::Internal(
                "WebDAV MOVE request failed".to_owned(),
            ))
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "WebDAV MOVE failed with status {status}"
        ))));
    }
    Ok(response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned))
}

/// Stat a WebDAV object: HEAD request returning size (Content-Length) and
/// revision (ETag). Returns `Ok(None)` on 404.
pub(crate) fn webdav_stat(
    client: &Client,
    url: &str,
    username: &str,
    password: &str,
) -> CommandResult<Option<RemoteObjectMetadata>> {
    let response = webdav_send(client, Method::HEAD, url, username, password, None, None)?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "WebDAV HEAD {url} failed with status {}",
            response.status()
        ))));
    }
    let size = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let revision = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    Ok(Some(RemoteObjectMetadata { size, revision }))
}

pub(crate) fn parse_webdav_payload(
    payload: Option<serde_json::Value>,
) -> CommandResult<WebDavSessionData> {
    let payload = payload.ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "WebDAV connection details are required for this provider".to_owned(),
        ))
    })?;

    match serde_json::from_value::<RemoteAuthPayloadInput>(payload).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "invalid remote auth payload: {error}"
        )))
    })? {
        RemoteAuthPayloadInput::WebDav {
            server_url,
            username,
            password,
            root_path,
        } => {
            if server_url.trim().is_empty() {
                return Err(CommandError::from(LibraryError::Internal(
                    "WebDAV server URL cannot be empty".to_owned(),
                )));
            }
            if username.trim().is_empty() {
                return Err(CommandError::from(LibraryError::Internal(
                    "WebDAV username cannot be empty".to_owned(),
                )));
            }
            if password.trim().is_empty() {
                return Err(CommandError::from(LibraryError::Internal(
                    "WebDAV password cannot be empty".to_owned(),
                )));
            }

            Ok(WebDavSessionData {
                server_url: normalize_server_url(&server_url)?,
                username,
                password,
                root_path: root_path.map(|value| value.trim().to_owned()),
            })
        }
    }
}

pub(crate) fn store_webdav_secret(
    app_data_dir: &Path,
    library_id: &str,
    username: String,
    password: String,
) -> CommandResult<()> {
    store_remote_credential(
        app_data_dir,
        library_id,
        &StoredWebDavSecret { username, password },
    )
}

pub(crate) fn load_webdav_secret(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<WebDavSecret> {
    let remote_root_url = library
        .remote_root_locator()
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_string(),
            ))
        })?
        .to_owned();
    if let Some(secret) = load_remote_credential::<StoredWebDavSecret>(app_data_dir, library.id())?
    {
        return Ok(WebDavSecret {
            root_url: remote_root_url,
            username: secret.username,
            password: secret.password,
        });
    }
    Err(CommandError::from(LibraryError::Internal(
        "missing stored credentials for the remote repository".to_owned(),
    )))
}

pub(crate) fn webdav_marker_url(root_url: &str) -> CommandResult<String> {
    join_url(root_url, ".openkara-library")
}

pub(crate) fn webdav_database_url(root_url: &str) -> CommandResult<String> {
    join_url(root_url, "openkara.db")
}

struct WebDavBootstrapStorage<'a> {
    library: &'a RegisteredLibrary,
    secret: &'a WebDavSecret,
    client: Client,
}

impl<'a> WebDavBootstrapStorage<'a> {
    fn new(library: &'a RegisteredLibrary, secret: &'a WebDavSecret) -> CommandResult<Self> {
        Ok(Self {
            library,
            secret,
            client: webdav_client()?,
        })
    }
}

impl super::bootstrap::RemoteBootstrapStorage for WebDavBootstrapStorage<'_> {
    fn location_label(&self) -> &'static str {
        "WebDAV path"
    }

    fn ensure_layout(&mut self) -> CommandResult<()> {
        let server_url = stored_webdav_server_url(self.library)?;
        ensure_webdav_collection_chain(
            &self.client,
            &server_url,
            &self.secret.root_url,
            &self.secret.username,
            &self.secret.password,
        )?;
        for directory in ["media", "media-g", "stems", "artwork"] {
            let directory_url = join_url(&self.secret.root_url, &format!("{directory}/"))?;
            ensure_webdav_collection_chain(
                &self.client,
                &server_url,
                &directory_url,
                &self.secret.username,
                &self.secret.password,
            )?;
        }
        Ok(())
    }

    fn marker_exists(&mut self) -> CommandResult<bool> {
        let marker_url = webdav_marker_url(&self.secret.root_url)?;
        webdav_exists(
            &self.client,
            &marker_url,
            &self.secret.username,
            &self.secret.password,
        )
    }

    fn upload_marker(&mut self, marker_bytes: &[u8]) -> CommandResult<()> {
        let marker_url = webdav_marker_url(&self.secret.root_url)?;
        upload_webdav_bytes(
            &self.client,
            &marker_url,
            marker_bytes.to_vec(),
            &self.secret.username,
            &self.secret.password,
        )?;
        Ok(())
    }

    fn probe_committed_database(
        &mut self,
    ) -> CommandResult<Option<super::bootstrap::CommittedDatabaseProbe>> {
        use super::bootstrap::CommittedDatabaseProbe;
        use crate::remote::manifest::{RepositoryManifest, MANIFEST_PATH};

        // Prefer the repository manifest when present.
        let manifest_url = join_url(&self.secret.root_url, MANIFEST_PATH)?;
        if webdav_exists(
            &self.client,
            &manifest_url,
            &self.secret.username,
            &self.secret.password,
        )? {
            let temp_path = std::env::temp_dir().join(format!(
                "openkara-manifest-probe-{}.json",
                uuid::Uuid::new_v4()
            ));
            download_webdav_file(
                &self.client,
                &manifest_url,
                &temp_path,
                &self.secret.username,
                &self.secret.password,
            )?
            .ok_or_else(|| {
                CommandError::from(LibraryError::Internal(
                    "WebDAV manifest download failed: file not found".to_owned(),
                ))
            })?;
            let content = fs::read_to_string(&temp_path).map_err(|error| {
                let _ = fs::remove_file(&temp_path);
                CommandError::from(LibraryError::Internal(format!(
                    "failed to read WebDAV manifest: {error}"
                )))
            })?;
            let _ = fs::remove_file(&temp_path);
            let manifest: RepositoryManifest = serde_json::from_str(&content).map_err(|error| {
                CommandError::from(LibraryError::Internal(format!(
                    "failed to parse WebDAV repository manifest: {error}"
                )))
            })?;
            let etag = webdav_get_etag(
                &self.client,
                &manifest_url,
                &self.secret.username,
                &self.secret.password,
            )?;
            return Ok(Some(CommittedDatabaseProbe {
                revision: etag,
                database_path: manifest.database_path,
                generation: manifest.generation,
                database_size: Some(manifest.database_size),
                database_sha256: Some(manifest.database_sha256),
            }));
        }

        // Legacy repositories without a manifest: root openkara.db.
        let database_url = webdav_database_url(&self.secret.root_url)?;
        if !webdav_exists(
            &self.client,
            &database_url,
            &self.secret.username,
            &self.secret.password,
        )? {
            return Ok(None);
        }
        let etag = webdav_get_etag(
            &self.client,
            &database_url,
            &self.secret.username,
            &self.secret.password,
        )?;
        Ok(Some(CommittedDatabaseProbe {
            revision: etag,
            database_path: "openkara.db".to_owned(),
            generation: 0,
            database_size: None,
            database_sha256: None,
        }))
    }

    fn download_database(&mut self, database_path: &str, destination: &Path) -> CommandResult<()> {
        let database_url = join_url(&self.secret.root_url, database_path)?;
        download_webdav_file(
            &self.client,
            &database_url,
            destination,
            &self.secret.username,
            &self.secret.password,
        )?
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "WebDAV database download failed for {database_path}: file not found"
            )))
        })?;
        Ok(())
    }

    fn upload_database(&mut self, source: &Path) -> CommandResult<Option<String>> {
        // One-time empty-repository seed only.
        let database_url = webdav_database_url(&self.secret.root_url)?;
        upload_webdav_file(
            &self.client,
            &database_url,
            source,
            &self.secret.username,
            &self.secret.password,
        )
    }
}

pub(crate) fn initialize_or_sync_webdav_library(
    _app_data_dir: &Path,
    library: &RegisteredLibrary,
    secret: &WebDavSecret,
) -> CommandResult<Option<String>> {
    let mut storage = WebDavBootstrapStorage::new(library, secret)?;
    super::bootstrap::bootstrap_remote_library(
        super::bootstrap::BootstrapMode::CreateOrOpen,
        library,
        &mut storage,
    )
}

pub(crate) fn refresh_existing_webdav_library(
    _app_data_dir: &Path,
    library: &RegisteredLibrary,
    secret: &WebDavSecret,
) -> CommandResult<Option<String>> {
    let mut storage = WebDavBootstrapStorage::new(library, secret)?;
    super::bootstrap::bootstrap_remote_library(
        super::bootstrap::BootstrapMode::RequireExisting,
        library,
        &mut storage,
    )
}

// Legacy path-relative upload helper. CAS publish hashes local staged bytes
// and uploads via content-addressed paths instead.
#[allow(dead_code)]
pub(crate) fn upload_relative_file_to_remote(
    library: &RegisteredLibrary,
    secret: &WebDavSecret,
    relative_path: &str,
) -> CommandResult<()> {
    let local_root = library.working_copy_root().ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "remote repository is missing a cached working copy".to_string(),
        ))
    })?;
    let source = local_root.join(relative_path);
    let client = webdav_client()?;
    let server_url = stored_webdav_server_url(library)?;
    if let Some(parent) = Path::new(relative_path).parent() {
        let mut current = String::new();
        for segment in parent.iter().filter_map(|segment| segment.to_str()) {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(segment);
            let directory_url = join_url(&secret.root_url, &format!("{current}/"))?;
            ensure_webdav_collection_chain(
                &client,
                &server_url,
                &directory_url,
                &secret.username,
                &secret.password,
            )?;
        }
    }
    let file_url = join_url(&secret.root_url, relative_path)?;
    upload_webdav_file(
        &client,
        &file_url,
        &source,
        &secret.username,
        &secret.password,
    )?;
    Ok(())
}

pub(crate) fn delete_relative_path_from_remote(
    secret: &WebDavSecret,
    relative_path: &str,
) -> CommandResult<()> {
    let client = webdav_client()?;
    let url = join_url(&secret.root_url, relative_path)?;
    let response = webdav_send(
        &client,
        Method::DELETE,
        &url,
        &secret.username,
        &secret.password,
        None,
        None,
    )?;
    match response.status() {
        StatusCode::OK | StatusCode::NO_CONTENT | StatusCode::ACCEPTED | StatusCode::NOT_FOUND => {
            Ok(())
        }
        status => {
            tracing::trace!("WebDAV delete at {url} returned {status}");
            Err(CommandError::from(LibraryError::Internal(
                "WebDAV delete failed. Check permissions and try again.".to_owned(),
            )))
        }
    }
}

pub(crate) struct WebDAVProvider<'a> {
    app_data_dir: &'a Path,
    secret: WebDavSecret,
    library: &'a RegisteredLibrary,
}

impl<'a> WebDAVProvider<'a> {
    pub(crate) fn new(
        app_data_dir: &'a Path,
        secret: WebDavSecret,
        library: &'a RegisteredLibrary,
    ) -> Self {
        Self {
            app_data_dir,
            secret,
            library,
        }
    }
}

impl RemoteProvider for WebDAVProvider<'_> {
    fn capabilities(&self) -> RemoteProviderCapabilities {
        // WebDAV supports conditional replacement when the server returns stable
        // ETags and honors If-Match/If-None-Match. We report the capability
        // optimistically; the actual CAS enforcement depends on the server.
        // If `stat` returns no ETag for the manifest path, the executor fails
        // closed before reaching `conditional_replace`.
        RemoteProviderCapabilities {
            conditional_replace: true,
            // PR#5: WebDAV servers vary in partial-PUT / Content-Range
            // support, so we do NOT claim resumable_upload. Large uploads
            // use a safe staging path + server-side MOVE instead.
            // WebDAV servers generally support Range requests (RFC 7233),
            // and `download_range` is implemented below, so we advertise
            // `range_download` to enable the resumable download path.
            resumable_upload: false,
            range_download: true,
            revision_metadata: true,
            // PR#5: Most WebDAV servers support MOVE (RFC 4918 §9.9). We
            // report this optimistically; the staged-upload path falls back
            // to a direct PUT if MOVE is unavailable.
            server_side_move: true,
        }
    }

    fn stat(&self, relative_path: &str) -> CommandResult<Option<RemoteObjectMetadata>> {
        let client = webdav_client()?;
        let url = join_url(&self.secret.root_url, relative_path)?;
        webdav_stat(&client, &url, &self.secret.username, &self.secret.password)
    }

    fn conditional_replace(
        &self,
        relative_path: &str,
        source: ConditionalSource,
        expected_revision: Option<&str>,
    ) -> RemoteResult<RemoteObjectMetadata> {
        let client = webdav_client()
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
        let url = join_url(&self.secret.root_url, relative_path)
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
        let bytes = source
            .read_bytes()
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
        // Ensure the parent collection exists for a first-create path.
        if expected_revision.is_none() {
            if let Some(parent) = Path::new(relative_path).parent() {
                let parent_path = parent.to_string_lossy().replace('\\', "/");
                if !parent_path.is_empty() {
                    let parent_url = join_url(&self.secret.root_url, &format!("{parent_path}/"))
                        .map_err(|e| {
                            RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message)
                        })?;
                    let server_url = crate::remote::types::stored_webdav_server_url(self.library)
                        .map_err(|e| {
                        RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message)
                    })?;
                    ensure_webdav_collection_chain(
                        &client,
                        &server_url,
                        &parent_url,
                        &self.secret.username,
                        &self.secret.password,
                    )
                    .map_err(|e| {
                        RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message)
                    })?;
                }
            }
        }
        webdav_conditional_put(
            &client,
            &url,
            bytes,
            &self.secret.username,
            &self.secret.password,
            expected_revision,
        )
    }

    fn get_revision(&self, relative_path: &str) -> CommandResult<Option<String>> {
        let client = webdav_client()?;
        let url = join_url(&self.secret.root_url, relative_path)?;
        webdav_get_etag(&client, &url, &self.secret.username, &self.secret.password)
    }

    fn download_file(&self, relative_path: &str, destination: &Path) -> CommandResult<()> {
        let client = webdav_client()?;
        let url = join_url(&self.secret.root_url, relative_path)?;
        download_webdav_file(
            &client,
            &url,
            destination,
            &self.secret.username,
            &self.secret.password,
        )?
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "remote file {relative_path} was not found"
            )))
        })?;
        Ok(())
    }

    fn download_range(
        &self,
        relative_path: &str,
        destination: &Path,
        offset: u64,
        length: u64,
    ) -> RemoteResult<u64> {
        if length == 0 {
            return Ok(0);
        }

        let client = webdav_client()
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
        let url = join_url(&self.secret.root_url, relative_path)
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;

        // RFC 7233 Range request: bytes=<start>-<end> (inclusive end).
        let end = offset
            .checked_add(length)
            .and_then(|value| value.checked_sub(1))
            .ok_or_else(|| {
                RemoteError::new(
                    RemoteErrorKind::RemoteIntegrityFailed,
                    format!("download_range offset/length overflow for {relative_path}"),
                )
            })?;
        let range_header = format!("bytes={offset}-{end}");

        let mut response = client
            .request(Method::GET, &url)
            .basic_auth(&self.secret.username, Some(&self.secret.password))
            .header("Range", &range_header)
            .send()
            .map_err(|_e| {
                RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!("WebDAV range GET to {url} failed"),
                )
            })?;

        let status = response.status();
        // 206 Partial Content is the expected success for a Range request.
        // A 200 OK means the server ignored the Range header — only
        // acceptable when offset == 0 (full-body fallback). For nonzero
        // offsets, a 200 OK would write the full body at the wrong position.
        if status == StatusCode::NOT_FOUND {
            return Err(RemoteError::new(
                RemoteErrorKind::PermissionDenied,
                format!("remote file {relative_path} was not found"),
            ));
        }
        if status == StatusCode::OK && offset > 0 {
            return Err(RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!(
                    "WebDAV range download returned 200 OK for nonzero offset {offset} \
                     — server ignored Range header"
                ),
            ));
        }
        if !status.is_success() {
            return Err(remote_error_from_status(status, "WebDAV range GET"));
        }

        // 206 Partial Content MUST include a matching Content-Range.
        if status == StatusCode::PARTIAL_CONTENT {
            let content_range = response.headers().get("content-range").ok_or_else(|| {
                RemoteError::new(
                    RemoteErrorKind::RemoteIntegrityFailed,
                    "206 Partial Content missing Content-Range header",
                )
            })?;
            let cr_str = content_range.to_str().map_err(|e| {
                RemoteError::new(
                    RemoteErrorKind::RemoteIntegrityFailed,
                    format!("invalid Content-Range header: {e}"),
                )
            })?;
            crate::remote::errors::verify_content_range(cr_str, offset, length)?;
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!("failed to create {}: {error}", parent.display()),
                )
            })?;
        }

        // Open for write at a specific offset. We intentionally do NOT
        // truncate — the file may already contain bytes from a prior range
        // download, and truncating would destroy them.
        #[allow(clippy::suspicious_open_options)]
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(destination)
            .map_err(|error| {
                RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!("failed to open {}: {error}", destination.display()),
                )
            })?;
        file.seek(SeekFrom::Start(offset)).map_err(|error| {
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("failed to seek {}: {error}", destination.display()),
            )
        })?;

        // Stream the body straight to the destination as it arrives instead of
        // buffering the whole chunk with `response.bytes()`. The buffered call
        // imposed one total-body deadline per chunk (an 8 MiB chunk needed
        // >= 68 KB/s just to finish); streaming makes the client timeout a
        // per-read idle timeout, so a slow-but-steady link makes progress and
        // an interruption leaves the bytes received so far durable on disk for
        // sub-chunk resume (issue #205). For a 206 the body is exactly
        // `length` bytes; for a 200 full-body fallback (offset == 0 only) the
        // whole file arrives and is written from the start.
        let written = crate::remote::net_policy::stream_response_body(&mut response, &mut file)
            .map_err(|error| {
                RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!("failed to stream WebDAV range response: {error}"),
                )
            })?;

        // Validate body length. For a 206 response, the body must be exactly
        // `length` bytes. For a 200 full-body response (offset == 0 only),
        // the body must be at least `length` bytes. A truncated transfer is a
        // transport failure, not corruption, so it stays retryable and the
        // partial bytes on disk are preserved for resume.
        if status == StatusCode::PARTIAL_CONTENT {
            if written != length {
                return Err(RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!(
                        "WebDAV range download body length mismatch: \
                         requested {length} bytes at offset {offset}, got {written} bytes"
                    ),
                ));
            }
        } else if written < length {
            return Err(RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!(
                    "WebDAV full-body response shorter than requested range: \
                     requested {length} bytes, got {written} bytes"
                ),
            ));
        }

        Ok(written)
    }

    fn upload_file(&self, relative_path: &str) -> CommandResult<()> {
        let local_root = self.library.working_copy_root().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a cached working copy".to_string(),
            ))
        })?;
        let source = local_root.join(relative_path);
        let bytes = fs::read(&source).map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to read {}: {error}",
                source.display()
            )))
        })?;

        let client = webdav_client()?;
        let server_url = crate::remote::types::stored_webdav_server_url(self.library)?;

        // Ensure the parent collection of the final path exists so the MOVE
        // (or fallback PUT) lands in an existing collection.
        if let Some(parent) = Path::new(relative_path).parent() {
            let parent_path = parent.to_string_lossy().replace('\\', "/");
            if !parent_path.is_empty() {
                let parent_url = join_url(&self.secret.root_url, &format!("{parent_path}/"))?;
                ensure_webdav_collection_chain(
                    &client,
                    &server_url,
                    &parent_url,
                    &self.secret.username,
                    &self.secret.password,
                )?;
            }
        }

        let final_url = join_url(&self.secret.root_url, relative_path)?;

        // Staged upload: PUT to `.openkara/staging/<op-id>/<filename>.part`,
        // then server-side MOVE to the final path. The operation id is derived
        // from the final path so concurrent uploads of different files do not
        // collide.
        let operation_id = uuid::Uuid::new_v4().to_string();
        let staging_url = webdav_staged_upload(
            &client,
            &self.secret.root_url,
            &operation_id,
            relative_path,
            bytes,
            &self.secret.username,
            &self.secret.password,
        )?;

        match webdav_move_staged_to_final(
            &client,
            &staging_url,
            &final_url,
            &self.secret.username,
            &self.secret.password,
        ) {
            Ok(_) => Ok(()),
            Err(move_error) => {
                // Some WebDAV servers do not support MOVE. Fall back to a
                // direct PUT of the original file bytes.
                tracing::trace!(
                    "WebDAV MOVE failed ({}); falling back to direct PUT for {relative_path}",
                    move_error.message
                );
                let bytes = fs::read(&source).map_err(|error| {
                    CommandError::from(LibraryError::Internal(format!(
                        "failed to re-read {}: {error}",
                        source.display()
                    )))
                })?;
                upload_webdav_bytes(
                    &client,
                    &final_url,
                    bytes,
                    &self.secret.username,
                    &self.secret.password,
                )?;
                Ok(())
            }
        }
    }

    fn delete_path(&self, relative_path: &str) -> CommandResult<()> {
        delete_relative_path_from_remote(&self.secret, relative_path)
    }

    fn initialize_or_sync(&self) -> CommandResult<Option<String>> {
        initialize_or_sync_webdav_library(self.app_data_dir, self.library, &self.secret)
    }

    fn get_file_size(&self, relative_path: &str) -> CommandResult<Option<u64>> {
        let client = webdav_client()?;
        let url = join_url(&self.secret.root_url, relative_path)?;
        let response = webdav_send(
            &client,
            Method::HEAD,
            &url,
            &self.secret.username,
            &self.secret.password,
            None,
            None,
        )?;
        if !response.status().is_success() {
            return Ok(None);
        }
        Ok(response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok()))
    }

    fn create_range_fetcher(
        &self,
        relative_path: &str,
    ) -> CommandResult<Option<Box<dyn crate::audio::remote_source::HttpFetcher>>> {
        let url = join_url(&self.secret.root_url, relative_path)?;
        let auth_value = format!(
            "Basic {}",
            base64_encode(&format!(
                "{}:{}",
                self.secret.username, self.secret.password
            ))
        );
        let headers = vec![("Authorization".to_owned(), auth_value)];
        Ok(Some(Box::new(
            crate::audio::remote_source::ProviderFetcher::new(url, headers),
        )))
    }

    fn refresh_existing(&self) -> CommandResult<Option<String>> {
        refresh_existing_webdav_library(self.app_data_dir, self.library, &self.secret)
    }
}

use super::provider::RemoteProvider;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cache,
        config::{RemoteLibraryConnectionConfig, RemoteLibraryProvider},
        library::Song,
        library_root::LibraryRoot,
    };
    use std::{
        collections::{HashMap, HashSet},
        net::{Ipv4Addr, SocketAddrV4, TcpListener},
        sync::{Arc, Mutex},
        thread::{self, JoinHandle},
    };
    use tempfile::tempdir;
    use tiny_http::{Header, Method as HttpMethod, Response, Server, StatusCode as HttpStatusCode};

    #[test]
    fn normalize_server_url_adds_trailing_slash() {
        let result =
            normalize_server_url("https://webdav.example.com/path").expect("URL should normalize");
        assert_eq!(result, "https://webdav.example.com/path/");
    }

    #[test]
    fn normalize_server_url_preserves_existing_trailing_slash() {
        let result =
            normalize_server_url("https://webdav.example.com/").expect("URL should normalize");
        assert_eq!(result, "https://webdav.example.com/");
    }

    #[test]
    fn normalize_server_url_preserves_subdirectory_with_slash() {
        let result = normalize_server_url("https://server.com/dav/").expect("URL should normalize");
        assert_eq!(result, "https://server.com/dav/");
    }

    #[test]
    fn normalize_server_url_rejects_invalid_url() {
        let result = normalize_server_url("not a url");
        assert!(result.is_err());
    }

    #[test]
    fn normalize_webdav_root_path_defaults_to_slugified_display_name() {
        let result = normalize_webdav_root_path(None, "My Karaoke");
        assert!(result.starts_with("/my-") || result.starts_with("/My-"));
    }

    #[test]
    fn normalize_webdav_root_path_strips_leading_trailing_slashes() {
        assert_eq!(
            normalize_webdav_root_path(Some("///my/path///"), "fallback"),
            "/my/path"
        );
    }

    #[test]
    fn normalize_webdav_root_path_adds_leading_slash() {
        assert_eq!(
            normalize_webdav_root_path(Some("music/karaoke"), "fallback"),
            "/music/karaoke"
        );
    }

    #[test]
    fn join_url_combines_base_and_relative_segments() {
        let result = join_url("https://server.com/dav/", "library/songs").expect("URL should join");
        assert_eq!(result, "https://server.com/dav/library/songs");
    }

    #[test]
    fn remote_path_display_renders_host_and_path() {
        let display = remote_path_display_from_url("https://dav.example.com/share/OpenKara/");
        assert_eq!(display, "dav.example.com/share/OpenKara");
    }

    #[test]
    fn remote_path_display_falls_back_to_raw_string_on_parse_failure() {
        let display = remote_path_display_from_url("not-a-valid-url");
        assert_eq!(display, "not-a-valid-url");
    }

    #[test]
    fn webdav_marker_url_produces_predicable_path() {
        let result =
            webdav_marker_url("https://server.com/dav/OpenKara/").expect("marker URL should build");
        assert!(result.contains("/dav/OpenKara/"));
        assert!(result.ends_with(".openkara-library") || result.ends_with("openkara.library"));
    }

    #[test]
    fn webdav_database_url_produces_predicable_path() {
        let result = webdav_database_url("https://server.com/dav/OpenKara/")
            .expect("database URL should build");
        assert!(result.contains("/dav/OpenKara/"));
        assert!(result.ends_with(".db"));
    }

    struct TestWebDavServer {
        base_url: String,
        directories: Arc<Mutex<HashSet<String>>>,
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        server: Option<Arc<Server>>,
        thread: Option<JoinHandle<()>>,
    }

    impl TestWebDavServer {
        fn start() -> Self {
            let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
            let address = listener.local_addr().unwrap();
            let server = Arc::new(Server::from_listener(listener, None).unwrap());
            let directories = Arc::new(Mutex::new(HashSet::from(["/".to_owned()])));
            let files = Arc::new(Mutex::new(HashMap::new()));
            let thread_directories = Arc::clone(&directories);
            let thread_files = Arc::clone(&files);
            let thread_server = Arc::clone(&server);
            let thread = thread::spawn(move || {
                while let Ok(request) = thread_server.recv() {
                    respond_to_webdav_request(request, &thread_directories, &thread_files);
                }
            });

            Self {
                base_url: format!("http://127.0.0.1:{}/", address.port()),
                directories,
                files,
                server: Some(server),
                thread: Some(thread),
            }
        }

        fn directory_exists(&self, path: &str) -> bool {
            self.directories.lock().unwrap().contains(path)
        }

        fn file(&self, path: &str) -> Option<Vec<u8>> {
            self.files.lock().unwrap().get(path).cloned()
        }
    }

    impl Drop for TestWebDavServer {
        fn drop(&mut self) {
            if let Some(server) = self.server.take() {
                server.unblock();
            }
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }

    fn respond_to_webdav_request(
        mut request: tiny_http::Request,
        directories: &Arc<Mutex<HashSet<String>>>,
        files: &Arc<Mutex<HashMap<String, Vec<u8>>>>,
    ) {
        let path = request.url().split('?').next().unwrap_or("/").to_owned();
        match *request.method() {
            HttpMethod::Head => {
                let exists = if path.ends_with('/') {
                    directories.lock().unwrap().contains(&path)
                } else {
                    files.lock().unwrap().contains_key(&path)
                };
                let status = if exists { 204 } else { 404 };
                let _ = request.respond(Response::empty(HttpStatusCode(status)));
            }
            HttpMethod::Put => {
                let mut body = Vec::new();
                request.as_reader().read_to_end(&mut body).unwrap();
                files.lock().unwrap().insert(path, body);
                let mut response = Response::empty(HttpStatusCode(201));
                response.add_header(Header::from_bytes("ETag", b"test-etag").unwrap());
                let _ = request.respond(response);
            }
            HttpMethod::Get => {
                let body = files.lock().unwrap().get(&path).cloned();
                match body {
                    Some(body) => {
                        let mut response =
                            Response::from_data(body).with_status_code(HttpStatusCode(200));
                        response.add_header(Header::from_bytes("ETag", b"test-etag").unwrap());
                        let _ = request.respond(response);
                    }
                    None => {
                        let _ = request.respond(Response::empty(HttpStatusCode(404)));
                    }
                }
            }
            HttpMethod::NonStandard(ref method) if method.as_str() == "MKCOL" => {
                directories.lock().unwrap().insert(path);
                let _ = request.respond(Response::empty(HttpStatusCode(201)));
            }
            _ => {
                let _ = request.respond(Response::empty(HttpStatusCode(405)));
            }
        }
    }

    fn test_remote_library(
        root_path: &Path,
        server_url: &str,
        root_url: &str,
    ) -> RegisteredLibrary {
        RegisteredLibrary::remote(
            "remote-webdav-test".to_owned(),
            "Remote WebDAV Test".to_owned(),
            RemoteLibraryProvider::WebDav,
            "openkara".to_owned(),
            root_url.to_owned(),
            "127.0.0.1/OpenKara".to_owned(),
            Some(RemoteLibraryConnectionConfig::WebDav {
                server_url: server_url.to_owned(),
            }),
            Some(root_path.join("openkara.db").to_string_lossy().into_owned()),
            None,
        )
    }

    #[test]
    fn webdav_initializes_uploads_and_reopens_remote_library() {
        let server = TestWebDavServer::start();
        let app_data_dir = tempdir().unwrap();
        let first_working_copy = tempdir().unwrap();
        let root_url = join_url(&server.base_url, "OpenKara/").unwrap();
        let secret = WebDavSecret {
            root_url: root_url.clone(),
            username: "openkara".to_owned(),
            password: "secret".to_owned(),
        };
        let first_library =
            test_remote_library(first_working_copy.path(), &server.base_url, &root_url);

        initialize_or_sync_webdav_library(app_data_dir.path(), &first_library, &secret)
            .expect("new WebDAV remote repository should initialize");
        let local_root = LibraryRoot::open(first_working_copy.path()).unwrap();
        let media_path = local_root.media_path("song-1", "wav");
        fs::write(&media_path, b"openkara test audio").unwrap();
        let connection = cache::open_database(&local_root.database_path()).unwrap();
        cache::upsert_song(
            &connection,
            &Song {
                hash: "song-1".to_owned(),
                file_path: Some("media/song-1.wav".to_owned()),
                cdg_path: None,
                media_g_container: None,
                instrumental: true,
                language: None,
                audio_source_kind: "original".to_owned(),
                title: Some("Remote Song".to_owned()),
                artist: Some("OpenKara".to_owned()),
                album: None,
                duration_ms: 1_000,
                cover_art: None,
                has_cover_art: false,
                artwork_thumb_path: None,
                imported_at: 1,
                original_ext: Some("wav".to_owned()),
            },
        )
        .unwrap();

        upload_relative_file_to_remote(&first_library, &secret, "media/song-1.wav")
            .expect("media file should upload");
        upload_relative_file_to_remote(&first_library, &secret, "openkara.db")
            .expect("library metadata should upload");

        assert!(server.directory_exists("/OpenKara/"));
        assert!(server.directory_exists("/OpenKara/media/"));
        assert!(server.directory_exists("/OpenKara/media-g/"));
        assert!(server.directory_exists("/OpenKara/stems/"));
        assert_eq!(
            server.file("/OpenKara/media/song-1.wav").as_deref(),
            Some(b"openkara test audio".as_slice())
        );
        assert!(server.file("/OpenKara/openkara.db").is_some());

        let second_working_copy = tempdir().unwrap();
        let second_library =
            test_remote_library(second_working_copy.path(), &server.base_url, &root_url);
        initialize_or_sync_webdav_library(app_data_dir.path(), &second_library, &secret)
            .expect("existing WebDAV remote repository should reopen");
        let second_root = LibraryRoot::open(second_working_copy.path()).unwrap();
        let second_connection = cache::open_database(&second_root.database_path()).unwrap();
        let songs = cache::list_songs(&second_connection).unwrap();

        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].title.as_deref(), Some("Remote Song"));
        assert_eq!(songs[0].file_path.as_deref(), Some("media/song-1.wav"));
    }

    #[test]
    fn webdav_refresh_existing_rejects_empty_remote_location() {
        let server = TestWebDavServer::start();
        let app_data_dir = tempdir().unwrap();
        let working_copy = tempdir().unwrap();
        let root_url = join_url(&server.base_url, "MovedOpenKara/").unwrap();
        let secret = WebDavSecret {
            root_url: root_url.clone(),
            username: "openkara".to_owned(),
            password: "secret".to_owned(),
        };
        let library = test_remote_library(working_copy.path(), &server.base_url, &root_url);

        let error = refresh_existing_webdav_library(app_data_dir.path(), &library, &secret)
            .expect_err("empty WebDAV path should not be initialized during relocation");

        assert!(error.message.contains("not an OpenKara remote repository"));
        assert!(!server.directory_exists("/MovedOpenKara/"));
        assert!(server.file("/MovedOpenKara/openkara.db").is_none());
    }

    #[test]
    fn download_webdav_file_streams_full_body_to_disk() {
        // Regression for issue #205: the full-file download now streams the
        // body to disk instead of buffering it with `response.bytes()`. Verify
        // the streamed file matches the source byte-for-byte through the real
        // provider helper.
        let server = TestWebDavServer::start();
        let client = webdav_client().unwrap();
        let url = join_url(&server.base_url, "song.bin").unwrap();
        let body: Vec<u8> = (0..200_000).map(|i| (i % 256) as u8).collect();
        upload_webdav_bytes(&client, &url, body.clone(), "openkara", "secret").unwrap();

        let dest_dir = tempdir().unwrap();
        let dest_path = dest_dir.path().join("nested/out.bin");
        let etag = download_webdav_file(&client, &url, &dest_path, "openkara", "secret").unwrap();

        assert!(
            etag.is_some(),
            "ETag surfaced from headers before streaming"
        );
        assert_eq!(
            std::fs::read(&dest_path).unwrap(),
            body,
            "streamed file matches"
        );
    }
}
