use crate::{
    commands::error::{CommandError, CommandResult},
    config::RegisteredLibrary,
    library::error::LibraryError,
    remote::{
        control_db::{
            delete_transfer_parts, list_transfer_parts, upsert_transfer_part, TransferDirection,
            TransferPartRow,
        },
        errors::{
            RemoteError, RemoteErrorKind, RemoteObjectMetadata, RemoteProviderCapabilities,
            RemoteResult,
        },
    },
};
use reqwest::{Method, Url};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    io::{Seek, SeekFrom},
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    path::Path,
    sync::{Arc, Mutex, OnceLock},
    thread,
};
use tiny_http::Server;

use super::RequestSendExt;
use super::{
    auth::{
        env_optional, form_urlencoded_body, oauth_callback_response, oauth_pkce_code_challenge,
        random_token, remote_auth_session_exists, update_remote_auth_session,
    },
    types::{
        current_unix_time_ms, load_remote_credential, store_remote_credential,
        stored_google_drive_client_id, BundledGoogleDriveOAuthClientFile,
        GoogleDriveFileListResponse, GoogleDriveFileMetadata, GoogleDriveProviderCredentials,
        GoogleDriveSecret, GoogleDriveSessionData, GoogleDriveTokenResponse,
        GoogleDriveUserInfoResponse, RemoteAuthSession, RemoteAuthState, StoredGoogleDriveSecret,
        GOOGLE_DRIVE_CLIENT_ID_ENV, GOOGLE_DRIVE_CLIENT_SECRET_ENV,
        GOOGLE_DRIVE_OAUTH_CLIENT_RESOURCE_PATH, GOOGLE_DRIVE_OAUTH_SCOPE,
    },
};

const GOOGLE_DRIVE_FOLDER_MIME_TYPE: &str = "application/vnd.google-apps.folder";
pub(crate) const GOOGLE_DRIVE_ROOT_ID: &str = "root";

pub(crate) fn build_google_drive_authorization_url(
    session: &GoogleDriveSessionData,
) -> CommandResult<String> {
    let mut url = Url::parse("https://accounts.google.com/o/oauth2/v2/auth").map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to build Google auth URL: {error}"
        )))
    })?;
    url.query_pairs_mut()
        .append_pair("client_id", &session.client_id)
        .append_pair("redirect_uri", &session.redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", GOOGLE_DRIVE_OAUTH_SCOPE)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair(
            "code_challenge",
            &oauth_pkce_code_challenge(&session.code_verifier),
        )
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &session.state_token);
    Ok(url.to_string())
}

pub(crate) fn google_drive_provider_credentials_from_env(
    client_id: Option<String>,
    client_secret: Option<String>,
) -> CommandResult<GoogleDriveProviderCredentials> {
    let Some(client_id) = client_id.filter(|value| !value.trim().is_empty()) else {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive is not available because the official app credential is missing. Set {GOOGLE_DRIVE_CLIENT_ID_ENV} before starting OpenKara."
        ))));
    };

    Ok(GoogleDriveProviderCredentials {
        client_id,
        client_secret: client_secret.filter(|value| !value.trim().is_empty()),
    })
}

fn load_google_drive_provider_credentials_from_resource_dir(
    resource_dir: &Path,
) -> CommandResult<Option<GoogleDriveProviderCredentials>> {
    let path = resource_dir.join(GOOGLE_DRIVE_OAUTH_CLIENT_RESOURCE_PATH);
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to read bundled Google Drive OAuth client metadata at {}: {error}",
            path.display()
        )))
    })?;
    let bundled: BundledGoogleDriveOAuthClientFile =
        serde_json::from_str(&raw).map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to parse bundled Google Drive OAuth client metadata at {}: {error}",
                path.display()
            )))
        })?;

    Ok(Some(GoogleDriveProviderCredentials {
        client_id: bundled.installed.client_id,
        client_secret: bundled.installed.client_secret,
    }))
}

fn resolve_google_drive_provider_credentials(
    resource_dir: &Path,
) -> CommandResult<GoogleDriveProviderCredentials> {
    if let Some(credentials) =
        load_google_drive_provider_credentials_from_resource_dir(resource_dir)?
    {
        return Ok(credentials);
    }

    google_drive_provider_credentials_from_env(
        env_optional(GOOGLE_DRIVE_CLIENT_ID_ENV),
        env_optional(GOOGLE_DRIVE_CLIENT_SECRET_ENV),
    )
}

pub(crate) fn parse_google_drive_payload(
    resource_dir: &Path,
    _payload: Option<serde_json::Value>,
) -> CommandResult<GoogleDriveSessionData> {
    let credentials = resolve_google_drive_provider_credentials(resource_dir)?;

    Ok(GoogleDriveSessionData {
        client_id: credentials.client_id,
        client_secret: credentials.client_secret,
        code_verifier: random_token(64),
        redirect_uri: String::new(),
        state_token: random_token(48),
        root_folder_id: None,
        access_token: None,
        refresh_token: None,
        access_token_expires_at_ms: None,
    })
}

pub(crate) fn store_google_drive_secret(
    app_data_dir: &Path,
    secret: GoogleDriveSecret,
) -> CommandResult<()> {
    store_remote_credential(
        app_data_dir,
        &secret.library_id,
        &StoredGoogleDriveSecret {
            client_secret: secret.client_secret,
            access_token: secret.access_token,
            refresh_token: secret.refresh_token,
            access_token_expires_at_ms: secret.access_token_expires_at_ms,
        },
    )
}

pub(crate) fn load_google_drive_secret(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<GoogleDriveSecret> {
    if let Some(secret) =
        load_remote_credential::<StoredGoogleDriveSecret>(app_data_dir, library.id())?
    {
        let client_id = stored_google_drive_client_id(library)?;
        return Ok(GoogleDriveSecret {
            library_id: library.id().to_owned(),
            client_id,
            client_secret: secret.client_secret,
            access_token: secret.access_token,
            refresh_token: secret.refresh_token,
            access_token_expires_at_ms: secret.access_token_expires_at_ms,
        });
    }
    Err(CommandError::from(LibraryError::Internal(
        "missing stored credentials for the remote repository".to_owned(),
    )))
}

fn google_drive_api_url(path: &str) -> CommandResult<Url> {
    Url::parse(&format!("https://www.googleapis.com{path}")).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to build Google Drive URL: {error}"
        )))
    })
}

/// Process-wide lock that serializes Google Drive token refresh. Without
/// this, concurrent provider instances would all fire refresh requests
/// simultaneously when the token expires.
static GOOGLE_DRIVE_REFRESH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn google_drive_refresh_access_token(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
) -> CommandResult<String> {
    // Fast path: the token is still valid. No lock needed.
    if let Some(expires_at_ms) = secret.access_token_expires_at_ms {
        if expires_at_ms > current_unix_time_ms() + 60_000 && !secret.access_token.is_empty() {
            return Ok(secret.access_token.clone());
        }
    } else if !secret.access_token.is_empty() {
        return Ok(secret.access_token.clone());
    }

    // Slow path: acquire the process-wide refresh lock so only one thread
    // performs the network refresh.
    let lock = GOOGLE_DRIVE_REFRESH_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().map_err(|_| {
        CommandError::from(LibraryError::Internal(
            "Google Drive refresh lock was poisoned".to_owned(),
        ))
    })?;

    // Re-check after acquiring the lock — another thread may have refreshed
    // while we waited. Reload the stored credential from disk to pick up the
    // refreshed token. The in-memory secret is per-provider-instance, so
    // without this reload the waiter would see its stale copy and fire a
    // redundant refresh request.
    if let Ok(Some(stored)) =
        load_remote_credential::<StoredGoogleDriveSecret>(app_data_dir, &secret.library_id)
    {
        if let Some(expires_at_ms) = stored.access_token_expires_at_ms {
            if expires_at_ms > current_unix_time_ms() + 60_000 && !stored.access_token.is_empty() {
                secret.access_token = stored.access_token;
                secret.access_token_expires_at_ms = stored.access_token_expires_at_ms;
                return Ok(secret.access_token.clone());
            }
        }
    }

    let mut params = vec![
        ("client_id", secret.client_id.clone()),
        ("refresh_token", secret.refresh_token.clone()),
        ("grant_type", "refresh_token".to_owned()),
    ];
    if let Some(client_secret) = secret.client_secret.clone() {
        params.push(("client_secret", client_secret));
    }

    let body = form_urlencoded_body(&params)?;

    let response = crate::remote::net_policy::shared_http_client()
        .post("https://oauth2.googleapis.com/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .map_err(|e| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to refresh Google Drive access token: {}",
                e.without_url()
            )))
        })?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive token refresh failed with status {}",
            response.status()
        ))));
    }
    let body: GoogleDriveTokenResponse = response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse Google Drive token response: {error}"
        )))
    })?;
    secret.access_token = body.access_token.clone();
    secret.access_token_expires_at_ms = body
        .expires_in
        .map(|seconds| current_unix_time_ms() + seconds * 1000);
    store_google_drive_secret(app_data_dir, secret.clone())?;
    Ok(secret.access_token.clone())
}

/// Used as a callback by `ProviderFetcher` for automatic token renewal on 403.
fn refresh_google_drive_token(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> Result<String, crate::audio::remote_source::FetchError> {
    let mut secret = load_google_drive_secret(app_data_dir, library)
        .map_err(|e| crate::audio::remote_source::FetchError::Cache(e.message))?;
    google_drive_refresh_access_token(app_data_dir, &mut secret)
        .map_err(|e| crate::audio::remote_source::FetchError::Cache(e.message))
}

fn google_drive_authorized_request(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    method: Method,
    url: Url,
) -> CommandResult<reqwest::blocking::RequestBuilder> {
    let token = google_drive_refresh_access_token(app_data_dir, secret)?;
    Ok(crate::remote::net_policy::shared_http_client()
        .request(method, url)
        .bearer_auth(token))
}

fn google_drive_request_with_access_token(
    access_token: &str,
    method: Method,
    url: Url,
) -> reqwest::blocking::RequestBuilder {
    crate::remote::net_policy::shared_http_client()
        .request(method, url)
        .bearer_auth(access_token)
}

fn google_drive_escape_query_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn google_drive_find_child(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    parent_id: &str,
    name: &str,
    mime_type: Option<&str>,
) -> CommandResult<Option<GoogleDriveFileMetadata>> {
    let mut url = google_drive_api_url("/drive/v3/files")?;
    let mut query = format!(
        "name = '{}' and '{}' in parents and trashed = false",
        google_drive_escape_query_value(name),
        google_drive_escape_query_value(parent_id)
    );
    if let Some(mime_type) = mime_type {
        query.push_str(&format!(
            " and mimeType = '{}'",
            google_drive_escape_query_value(mime_type)
        ));
    }
    url.query_pairs_mut()
        .append_pair("q", &query)
        .append_pair(
            "fields",
            "files(id,name,mimeType,headRevisionId,modifiedTime,size)",
        )
        .append_pair("spaces", "drive");

    let response = google_drive_authorized_request(app_data_dir, secret, Method::GET, url)?
        .send_network("Google Drive lookup")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive lookup failed with status {}",
            response.status()
        ))));
    }
    let body: GoogleDriveFileListResponse = response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse Google Drive file list: {error}"
        )))
    })?;
    Ok(body.files.into_iter().next())
}

fn google_drive_find_child_with_token(
    access_token: &str,
    parent_id: &str,
    name: &str,
    mime_type: Option<&str>,
) -> CommandResult<Option<GoogleDriveFileMetadata>> {
    let mut url = google_drive_api_url("/drive/v3/files")?;
    let mut query = format!(
        "name = '{}' and '{}' in parents and trashed = false",
        google_drive_escape_query_value(name),
        google_drive_escape_query_value(parent_id)
    );
    if let Some(mime_type) = mime_type {
        query.push_str(&format!(
            " and mimeType = '{}'",
            google_drive_escape_query_value(mime_type)
        ));
    }
    url.query_pairs_mut()
        .append_pair("q", &query)
        .append_pair(
            "fields",
            "files(id,name,mimeType,headRevisionId,modifiedTime,size)",
        )
        .append_pair("spaces", "drive");

    let response = google_drive_request_with_access_token(access_token, Method::GET, url)
        .send_network("Google Drive lookup")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive lookup failed with status {}",
            response.status()
        ))));
    }
    let body: GoogleDriveFileListResponse = response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse Google Drive file list: {error}"
        )))
    })?;
    Ok(body.files.into_iter().next())
}

fn google_drive_create_folder(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    parent_id: &str,
    name: &str,
) -> CommandResult<GoogleDriveFileMetadata> {
    let url = google_drive_api_url("/drive/v3/files?fields=id,name,mimeType")?;
    let response = google_drive_authorized_request(app_data_dir, secret, Method::POST, url)?
        .json(&serde_json::json!({
            "name": name,
            "mimeType": GOOGLE_DRIVE_FOLDER_MIME_TYPE,
            "parents": [parent_id],
        }))
        .send_network("create Google Drive folder")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive folder creation failed with status {}",
            response.status()
        ))));
    }
    response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse folder creation response: {error}"
        )))
    })
}

fn google_drive_get_or_create_folder(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    parent_id: &str,
    name: &str,
) -> CommandResult<GoogleDriveFileMetadata> {
    if let Some(existing) = google_drive_find_child(
        app_data_dir,
        secret,
        parent_id,
        name,
        Some(GOOGLE_DRIVE_FOLDER_MIME_TYPE),
    )? {
        return Ok(existing);
    }
    google_drive_create_folder(app_data_dir, secret, parent_id, name)
}

fn google_drive_create_folder_with_token(
    access_token: &str,
    parent_id: &str,
    name: &str,
) -> CommandResult<GoogleDriveFileMetadata> {
    let url = google_drive_api_url("/drive/v3/files?fields=id,name,mimeType")?;
    let response = google_drive_request_with_access_token(access_token, Method::POST, url)
        .json(&serde_json::json!({
            "name": name,
            "mimeType": GOOGLE_DRIVE_FOLDER_MIME_TYPE,
            "parents": [parent_id],
        }))
        .send_network("create Google Drive folder")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive folder creation failed with status {}",
            response.status()
        ))));
    }
    response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse folder creation response: {error}"
        )))
    })
}

pub(crate) fn google_drive_get_or_create_folder_with_token(
    access_token: &str,
    parent_id: &str,
    name: &str,
) -> CommandResult<GoogleDriveFileMetadata> {
    if let Some(existing) = google_drive_find_child_with_token(
        access_token,
        parent_id,
        name,
        Some(GOOGLE_DRIVE_FOLDER_MIME_TYPE),
    )? {
        return Ok(existing);
    }
    google_drive_create_folder_with_token(access_token, parent_id, name)
}

fn google_drive_create_empty_file(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    parent_id: &str,
    name: &str,
) -> CommandResult<GoogleDriveFileMetadata> {
    let url = google_drive_api_url(
        "/drive/v3/files?fields=id,name,mimeType,headRevisionId,modifiedTime,size",
    )?;
    let response = google_drive_authorized_request(app_data_dir, secret, Method::POST, url)?
        .json(&serde_json::json!({
            "name": name,
            "parents": [parent_id],
        }))
        .send_network("create Google Drive file metadata")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive file metadata creation failed with status {}",
            response.status()
        ))));
    }
    response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse file creation response: {error}"
        )))
    })
}

fn google_drive_upload_file_bytes(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    file_id: &str,
    bytes: Vec<u8>,
) -> CommandResult<GoogleDriveFileMetadata> {
    let url = google_drive_api_url(&format!(
        "/upload/drive/v3/files/{file_id}?uploadType=media&fields=id,name,mimeType,headRevisionId,modifiedTime,size"
    ))?;
    let response = google_drive_authorized_request(app_data_dir, secret, Method::PATCH, url)?
        .header("Content-Type", "application/octet-stream")
        .body(bytes)
        .send_network("upload Google Drive file bytes")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive file upload failed with status {}",
            response.status()
        ))));
    }
    response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse upload response: {error}"
        )))
    })
}

// ---------------------------------------------------------------------------
// Resumable upload (PR#5)
//
// Google Drive supports resumable uploads via `uploadType=resumable`:
//   1. POST metadata to the resumable URI with `X-Upload-Content-Type` and
//      `X-Upload-Content-Length` headers. The response's `Location` header is
//      the session URL (persisted in `remote_transfer_parts.provider_session_id`).
//   2. PUT chunks to the session URL with `Content-Range: bytes <start>-<end>/<total>`.
//   3. Status query: PUT with `Content-Range: bytes */*` returns 308 with a
//      `Range: bytes=0-<committed>` header indicating the committed offset.
//
// On resume after restart, query the committed offset and resume from there.
// A changed provider_revision invalidates the partial transfer.
//
// See: https://developers.google.com/drive/api/guides/manage-uploads#resumable
// ---------------------------------------------------------------------------

/// Chunk size for Google Drive resumable uploads. Google recommends 8 MiB for
/// most files; we use 8 MiB to match the Dropbox chunk size.
const GOOGLE_DRIVE_RESUMABLE_CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// Initiate a resumable upload session for a new file. Returns the session URL
/// (from the `Location` header) to persist in
/// `remote_transfer_parts.provider_session_id`.
pub(crate) fn google_drive_begin_resumable_upload(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    parent_id: &str,
    file_name: &str,
    total_size: u64,
) -> CommandResult<String> {
    let url = google_drive_api_url("/upload/drive/v3/files?uploadType=resumable")?;
    let metadata = serde_json::json!({
        "name": file_name,
        "parents": [parent_id]
    });
    let response = google_drive_authorized_request(app_data_dir, secret, Method::POST, url)?
        .header("Content-Type", "application/json; charset=UTF-8")
        .header("X-Upload-Content-Type", "application/octet-stream")
        .header("X-Upload-Content-Length", total_size.to_string())
        .body(metadata.to_string())
        .send_network("Google Drive resumable upload initiate")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive resumable upload initiate failed with status {}",
            response.status()
        ))));
    }
    response
        .headers()
        .get("Location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "Google Drive resumable upload response missing Location header".to_owned(),
            ))
        })
}

/// Initiate a resumable upload session for updating an existing file (by
/// `file_id`). Used when the file already exists and we want to overwrite it
/// resumably.
pub(crate) fn google_drive_begin_resumable_upload_existing(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    file_id: &str,
    total_size: u64,
) -> CommandResult<String> {
    let url = google_drive_api_url(&format!(
        "/upload/drive/v3/files/{file_id}?uploadType=resumable"
    ))?;
    let response = google_drive_authorized_request(app_data_dir, secret, Method::PATCH, url)?
        .header("Content-Type", "application/json; charset=UTF-8")
        .header("X-Upload-Content-Type", "application/octet-stream")
        .header("X-Upload-Content-Length", total_size.to_string())
        .body("{}")
        .send_network("Google Drive resumable upload initiate (existing)")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive resumable upload initiate (existing) failed with status {}",
            response.status()
        ))));
    }
    response
        .headers()
        .get("Location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "Google Drive resumable upload response missing Location header".to_owned(),
            ))
        })
}

/// Query the committed offset of a resumable upload session. Sends a PUT with
/// `Content-Range: bytes */*` and an empty body. Google Drive responds with
/// 308 (Permanent Redirect) and a `Range: bytes=0-<committed>` header, or 200
/// if the upload is complete.
pub(crate) fn google_drive_query_resumable_offset(
    session_url: &str,
    access_token: &str,
    total_size: u64,
) -> CommandResult<u64> {
    let response = crate::remote::net_policy::shared_http_client()
        .put(session_url)
        .bearer_auth(access_token)
        .header("Content-Range", format!("bytes */{total_size}"))
        .header("Content-Length", "0")
        .send()
        .map_err(|e| {
            CommandError::from(LibraryError::Internal(format!(
                "Google Drive resumable status query failed: {}",
                e.without_url()
            )))
        })?;
    let status = response.status().as_u16();
    // 200/201 = upload complete; 308 = partial upload with Range header.
    if status == 200 || status == 201 {
        return Ok(total_size);
    }
    if status != 308 {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive resumable status query returned unexpected status {status}"
        ))));
    }
    // Parse the Range header: "bytes=0-<committed>".
    let range_header = response
        .headers()
        .get("Range")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "Google Drive resumable status query returned 308 without Range header".to_owned(),
            ))
        })?;
    // Format: "bytes=0-123" → committed offset is 124 (last byte + 1).
    let committed = range_header
        .strip_prefix("bytes=0-")
        .and_then(|s| s.parse::<u64>().ok())
        .map(|last| last + 1)
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "Google Drive resumable status query returned unparseable Range header: {range_header}"
            )))
        })?;
    Ok(committed)
}

/// Upload a single chunk to a resumable upload session. `start` is the byte
/// offset within the file; `chunk` is the chunk bytes.
pub(crate) fn google_drive_upload_chunk(
    session_url: &str,
    access_token: &str,
    start: u64,
    total_size: u64,
    chunk: &[u8],
) -> CommandResult<()> {
    let end = start + chunk.len() as u64 - 1;
    let response = crate::remote::net_policy::shared_http_client()
        .put(session_url)
        .bearer_auth(access_token)
        .header("Content-Range", format!("bytes {start}-{end}/{total_size}"))
        .header("Content-Length", chunk.len().to_string())
        .body(chunk.to_vec())
        .send()
        .map_err(|e| {
            CommandError::from(LibraryError::Internal(format!(
                "Google Drive resumable chunk upload failed: {}",
                e.without_url()
            )))
        })?;
    let status = response.status().as_u16();
    // 200/201 = upload complete; 308 = chunk accepted, more pending.
    if status == 200 || status == 201 || status == 308 {
        Ok(())
    } else {
        Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive resumable chunk upload failed with status {status}"
        ))))
    }
}

/// Upload `bytes` to a Google Drive file using a resumable upload session.
/// `existing_session` is `(session_url, offset)` from a prior interrupted run.
/// `progress` is called after each chunk with `(session_url, committed_offset)`.
///
/// For a new file, pass `parent_id` + `file_name`. For an existing file,
/// pass `file_id` (the session is initiated against the existing file).
pub(crate) fn google_drive_resumable_upload(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    bytes: &[u8],
    existing_session: Option<(&str, u64)>,
    parent_id: Option<&str>,
    file_name: Option<&str>,
    file_id: Option<&str>,
    progress: &dyn Fn(&str, u64),
) -> CommandResult<GoogleDriveFileMetadata> {
    let total = bytes.len() as u64;
    let token = google_drive_refresh_access_token(app_data_dir, secret)?;

    let (session_url, mut offset) = match existing_session {
        Some((url, off)) if off > 0 && off < total => {
            // Resume: query the committed offset from the server to verify
            // our persisted offset is correct.
            let server_offset = google_drive_query_resumable_offset(url, &token, total)?;
            (url.to_owned(), server_offset)
        }
        _ => {
            // Start a new session.
            let url = if let Some(fid) = file_id {
                google_drive_begin_resumable_upload_existing(app_data_dir, secret, fid, total)?
            } else {
                let pid = parent_id.ok_or_else(|| {
                    CommandError::from(LibraryError::Internal(
                        "Google Drive resumable upload requires parent_id for new files".to_owned(),
                    ))
                })?;
                let fname = file_name.ok_or_else(|| {
                    CommandError::from(LibraryError::Internal(
                        "Google Drive resumable upload requires file_name for new files".to_owned(),
                    ))
                })?;
                google_drive_begin_resumable_upload(app_data_dir, secret, pid, fname, total)?
            };
            progress(&url, 0);
            (url, 0)
        }
    };

    // Upload chunks.
    while offset < total {
        let chunk_end = (offset as usize + GOOGLE_DRIVE_RESUMABLE_CHUNK_SIZE).min(bytes.len());
        let chunk = &bytes[offset as usize..chunk_end];
        google_drive_upload_chunk(&session_url, &token, offset, total, chunk)?;
        offset = chunk_end as u64;
        progress(&session_url, offset);
    }

    // The final chunk (or a status query) returns the file metadata. Query
    // the session to retrieve the committed file metadata.
    // For a complete upload, a final status query returns 200 with metadata.
    let final_response = crate::remote::net_policy::shared_http_client()
        .put(&session_url)
        .bearer_auth(&token)
        .header("Content-Range", format!("bytes */{total}"))
        .header("Content-Length", "0")
        .send()
        .map_err(|e| {
            CommandError::from(LibraryError::Internal(format!(
                "Google Drive resumable finish query failed: {}",
                e.without_url()
            )))
        })?;
    if !final_response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive resumable finish query failed with status {}",
            final_response.status()
        ))));
    }
    final_response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse Google Drive resumable finish response: {error}"
        )))
    })
}

pub(crate) fn google_drive_download_file(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    file_id: &str,
    destination: &Path,
) -> CommandResult<()> {
    use crate::remote::errors::{remote_error_from_status, RemoteError, RemoteErrorKind};
    use crate::remote::send_with_retry;

    let url = google_drive_api_url(&format!("/drive/v3/files/{file_id}?alt=media"))?;
    let mut response = send_with_retry("download Google Drive file", || {
        let builder =
            google_drive_authorized_request(app_data_dir, secret, Method::GET, url.clone())
                .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
        Ok(builder)
    })
    .map_err(|e| e.to_command_error())?;
    if !response.status().is_success() {
        return Err(
            remote_error_from_status(response.status(), "Google Drive download").to_command_error(),
        );
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to create {}: {error}",
                parent.display()
            )))
        })?;
    }
    // Stream to disk as bytes arrive rather than buffering the whole file with
    // `response.bytes()`, so the client timeout acts as a per-read idle timeout
    // and slow links complete instead of failing at a single total-body
    // deadline (issue #205).
    let mut file = fs::File::create(destination).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to create {}: {error}",
            destination.display()
        )))
    })?;
    crate::remote::net_policy::stream_response_body(&mut response, &mut file).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to stream Google Drive response: {error}"
        )))
    })?;
    Ok(())
}

pub(crate) fn google_drive_root_display_name(display_name: &str) -> String {
    format!("My Drive/{display_name}")
}

fn google_drive_exchange_code_for_tokens(
    session: &GoogleDriveSessionData,
    code: &str,
) -> CommandResult<GoogleDriveTokenResponse> {
    let mut params = vec![
        ("client_id", session.client_id.clone()),
        ("code", code.to_owned()),
        ("code_verifier", session.code_verifier.clone()),
        ("grant_type", "authorization_code".to_owned()),
        ("redirect_uri", session.redirect_uri.clone()),
    ];
    if let Some(client_secret) = session.client_secret.clone() {
        params.push(("client_secret", client_secret));
    }

    let body = form_urlencoded_body(&params)?;

    let response = crate::remote::net_policy::shared_http_client()
        .post("https://oauth2.googleapis.com/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to exchange Google auth code: {error}"
            )))
        })?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google auth code exchange failed with status {}",
            response.status()
        ))));
    }
    response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse Google token response: {error}"
        )))
    })
}

fn google_drive_fetch_account_id(access_token: &str) -> CommandResult<String> {
    let url = Url::parse("https://openidconnect.googleapis.com/v1/userinfo").map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to build Google userinfo URL: {error}"
        )))
    })?;
    let response = crate::remote::net_policy::shared_http_client()
        .get(url)
        .bearer_auth(access_token)
        .send()
        .map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to fetch Google Drive account info: {error}"
            )))
        })?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive account lookup failed with status {}",
            response.status()
        ))));
    }
    let body: GoogleDriveUserInfoResponse = response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse Google Drive account info: {error}"
        )))
    })?;
    Ok(body.email.unwrap_or(body.sub))
}

pub(crate) fn spawn_google_drive_auth_worker(
    sessions: Arc<Mutex<HashMap<String, RemoteAuthSession>>>,
    session_id: String,
    session: GoogleDriveSessionData,
) -> CommandResult<GoogleDriveSessionData> {
    let listener =
        TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to bind Google OAuth loopback listener: {error}"
            )))
        })?;
    let port = listener
        .local_addr()
        .map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to read Google OAuth listener address: {error}"
            )))
        })?
        .port();
    let server = Server::from_listener(listener, None).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to start Google OAuth listener: {error}"
        )))
    })?;

    let mut session = session;
    session.redirect_uri = format!("http://127.0.0.1:{port}/oauth2/callback");
    let worker_session = session.clone();

    thread::spawn(move || {
        let started_at = std::time::Instant::now();
        let request = loop {
            if !remote_auth_session_exists(&sessions, &session_id) {
                return;
            }

            match server.recv_timeout(std::time::Duration::from_secs(1)) {
                Ok(Some(request)) => break request,
                Ok(None) => {
                    if started_at.elapsed() >= std::time::Duration::from_secs(300) {
                        update_remote_auth_session(&sessions, &session_id, |state| {
                            state.state = RemoteAuthState::Failed;
                            state.error = Some(CommandError::from(LibraryError::Internal(
                                "Google sign-in timed out before the browser returned to OpenKara."
                                    .to_owned(),
                            )));
                        });
                        return;
                    }
                }
                Err(error) => {
                    update_remote_auth_session(&sessions, &session_id, |state| {
                        state.state = RemoteAuthState::Failed;
                        state.error = Some(CommandError::from(LibraryError::Internal(format!(
                            "Google sign-in listener failed: {error}"
                        ))));
                    });
                    return;
                }
            }
        };

        let callback_url = format!("http://127.0.0.1:{port}{}", request.url());
        let parsed = match Url::parse(&callback_url) {
            Ok(parsed) => parsed,
            Err(error) => {
                let _ = request.respond(oauth_callback_response("Invalid OAuth callback."));
                update_remote_auth_session(&sessions, &session_id, |state| {
                    state.state = RemoteAuthState::Failed;
                    state.error = Some(CommandError::from(LibraryError::Internal(format!(
                        "failed to parse Google OAuth callback: {error}"
                    ))));
                });
                return;
            }
        };
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        if query.get("state") != Some(&worker_session.state_token) {
            let _ = request.respond(oauth_callback_response("OAuth state mismatch."));
            update_remote_auth_session(&sessions, &session_id, |state| {
                state.state = RemoteAuthState::Failed;
                state.error = Some(CommandError::from(LibraryError::Internal(
                    "Google sign-in failed because the OAuth state token did not match.".to_owned(),
                )));
            });
            return;
        }

        if let Some(error) = query.get("error") {
            let _ = request.respond(oauth_callback_response(
                "Google sign-in was cancelled or denied.",
            ));
            update_remote_auth_session(&sessions, &session_id, |state| {
                state.state = RemoteAuthState::Failed;
                state.error = Some(CommandError::from(LibraryError::Internal(format!(
                    "Google sign-in failed: {error}"
                ))));
            });
            return;
        }

        let Some(code) = query.get("code") else {
            let _ = request.respond(oauth_callback_response(
                "Missing Google authorization code.",
            ));
            update_remote_auth_session(&sessions, &session_id, |state| {
                state.state = RemoteAuthState::Failed;
                state.error = Some(CommandError::from(LibraryError::Internal(
                    "Google sign-in did not return an authorization code.".to_owned(),
                )));
            });
            return;
        };

        match google_drive_exchange_code_for_tokens(&worker_session, code).and_then(|tokens| {
            let account_id = google_drive_fetch_account_id(&tokens.access_token)?;
            Ok((tokens, account_id))
        }) {
            Ok((tokens, account_id)) => {
                let _ = request.respond(oauth_callback_response(
                    "OpenKara connected to Google Drive. You can return to the app.",
                ));
                update_remote_auth_session(&sessions, &session_id, |state| {
                    state.state = RemoteAuthState::Ready;
                    state.account_id = account_id;
                    state.session =
                        super::types::ProviderSessionData::GoogleDrive(GoogleDriveSessionData {
                            access_token: Some(tokens.access_token.clone()),
                            refresh_token: tokens.refresh_token.clone(),
                            access_token_expires_at_ms: tokens
                                .expires_in
                                .map(|seconds| current_unix_time_ms() + seconds * 1000),
                            ..worker_session.clone()
                        });
                    state.error = None;
                });
            }
            Err(error) => {
                let _ = request.respond(oauth_callback_response(
                    "OpenKara could not finish Google Drive sign-in.",
                ));
                update_remote_auth_session(&sessions, &session_id, |state| {
                    state.state = RemoteAuthState::Failed;
                    state.error = Some(error);
                });
            }
        }
    });

    Ok(session)
}

pub(crate) fn google_drive_find_relative_entry(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    root_folder_id: &str,
    relative_path: &str,
) -> CommandResult<Option<GoogleDriveFileMetadata>> {
    let segments: Vec<&str> = relative_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return Ok(None);
    }

    let mut parent_id = root_folder_id.to_owned();
    for (index, segment) in segments.iter().enumerate() {
        let is_last = index == segments.len() - 1;
        let entry = google_drive_find_child(
            app_data_dir,
            secret,
            &parent_id,
            segment,
            if is_last {
                None
            } else {
                Some(GOOGLE_DRIVE_FOLDER_MIME_TYPE)
            },
        )?;
        let Some(entry) = entry else {
            return Ok(None);
        };
        if !is_last {
            parent_id = entry.id.clone();
        } else {
            return Ok(Some(entry));
        }
    }

    Ok(None)
}

pub(crate) fn google_drive_upload_relative_file_to_remote(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    secret: &GoogleDriveSecret,
    relative_path: &str,
    root_folder_id: &str,
) -> CommandResult<()> {
    let local_root = library.working_copy_root().ok_or_else(|| {
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
    let mut secret = secret.clone();

    let segments: Vec<&str> = relative_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return Ok(());
    }
    let file_name = segments.last().copied().unwrap_or_default();
    let mut parent_id = root_folder_id.to_owned();
    for segment in &segments[..segments.len() - 1] {
        let folder =
            google_drive_get_or_create_folder(app_data_dir, &mut secret, &parent_id, segment)?;
        parent_id = folder.id;
    }

    let file =
        match google_drive_find_child(app_data_dir, &mut secret, &parent_id, file_name, None)? {
            Some(file) => file,
            None => {
                google_drive_create_empty_file(app_data_dir, &mut secret, &parent_id, file_name)?
            }
        };
    let _ = google_drive_upload_file_bytes(app_data_dir, &mut secret, &file.id, bytes)?;
    Ok(())
}

struct GoogleDriveBootstrapStorage<'a> {
    app_data_dir: &'a Path,
    library: &'a RegisteredLibrary,
    secret: GoogleDriveSecret,
    root_folder_id: &'a str,
}

impl<'a> GoogleDriveBootstrapStorage<'a> {
    fn new(
        app_data_dir: &'a Path,
        library: &'a RegisteredLibrary,
        secret: &GoogleDriveSecret,
    ) -> CommandResult<Self> {
        let root_folder_id = library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        Ok(Self {
            app_data_dir,
            library,
            secret: secret.clone(),
            root_folder_id,
        })
    }
}

impl super::bootstrap::RemoteBootstrapStorage for GoogleDriveBootstrapStorage<'_> {
    fn location_label(&self) -> &'static str {
        "Google Drive folder"
    }

    fn ensure_layout(&mut self) -> CommandResult<()> {
        for directory in ["media", "media-g", "stems", "artwork"] {
            let _ = google_drive_get_or_create_folder(
                self.app_data_dir,
                &mut self.secret,
                self.root_folder_id,
                directory,
            )?;
        }
        Ok(())
    }

    fn marker_exists(&mut self) -> CommandResult<bool> {
        Ok(google_drive_find_relative_entry(
            self.app_data_dir,
            &mut self.secret,
            self.root_folder_id,
            ".openkara-library",
        )?
        .is_some())
    }

    fn upload_marker(&mut self, _marker_bytes: &[u8]) -> CommandResult<()> {
        // Marker bytes are written to the Local Working Copy by the shared
        // protocol; upload the local relative path to Drive.
        google_drive_upload_relative_file_to_remote(
            self.app_data_dir,
            self.library,
            &self.secret,
            ".openkara-library",
            self.root_folder_id,
        )
    }

    fn probe_committed_database(
        &mut self,
    ) -> CommandResult<Option<super::bootstrap::CommittedDatabaseProbe>> {
        use super::bootstrap::CommittedDatabaseProbe;
        use crate::remote::manifest::{RepositoryManifest, MANIFEST_PATH};

        // Prefer the repository manifest when present.
        if let Some(manifest_entry) = google_drive_find_relative_entry(
            self.app_data_dir,
            &mut self.secret,
            self.root_folder_id,
            MANIFEST_PATH,
        )? {
            let temp_path = std::env::temp_dir().join(format!(
                "openkara-manifest-probe-{}.json",
                uuid::Uuid::new_v4()
            ));
            google_drive_download_file(
                self.app_data_dir,
                &mut self.secret,
                &manifest_entry.id,
                &temp_path,
            )?;
            let content = fs::read_to_string(&temp_path).map_err(|error| {
                let _ = fs::remove_file(&temp_path);
                CommandError::from(LibraryError::Internal(format!(
                    "failed to read Google Drive manifest: {error}"
                )))
            })?;
            let _ = fs::remove_file(&temp_path);
            let manifest: RepositoryManifest = serde_json::from_str(&content).map_err(|error| {
                CommandError::from(LibraryError::Internal(format!(
                    "failed to parse Google Drive repository manifest: {error}"
                )))
            })?;
            return Ok(Some(CommittedDatabaseProbe {
                revision: manifest_entry
                    .head_revision_id
                    .or(manifest_entry.modified_time),
                database_path: manifest.database_path,
                generation: manifest.generation,
                database_size_bytes: Some(manifest.database_size_bytes),
                database_sha256: Some(manifest.database_sha256),
            }));
        }

        // Legacy repositories without a manifest: root openkara.db.
        Ok(google_drive_find_relative_entry(
            self.app_data_dir,
            &mut self.secret,
            self.root_folder_id,
            "openkara.db",
        )?
        .map(|entry| CommittedDatabaseProbe {
            revision: entry.head_revision_id.or(entry.modified_time),
            database_path: "openkara.db".to_owned(),
            generation: 0,
            database_size_bytes: entry.size_bytes,
            database_sha256: None,
        }))
    }

    fn download_database(&mut self, database_path: &str, destination: &Path) -> CommandResult<()> {
        let entry = google_drive_find_relative_entry(
            self.app_data_dir,
            &mut self.secret,
            self.root_folder_id,
            database_path,
        )?
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "Google Drive database {database_path} was not found"
            )))
        })?;
        google_drive_download_file(self.app_data_dir, &mut self.secret, &entry.id, destination)
    }

    fn upload_database(&mut self, _source: &Path) -> CommandResult<Option<String>> {
        // One-time empty-repository seed only.
        google_drive_upload_relative_file_to_remote(
            self.app_data_dir,
            self.library,
            &self.secret,
            "openkara.db",
            self.root_folder_id,
        )?;
        let uploaded = google_drive_find_relative_entry(
            self.app_data_dir,
            &mut self.secret,
            self.root_folder_id,
            "openkara.db",
        )?
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "Google Drive database upload succeeded but the file was not found afterwards"
                    .to_owned(),
            ))
        })?;
        Ok(uploaded.head_revision_id.or(uploaded.modified_time))
    }
}

pub(crate) fn initialize_or_sync_google_drive_library(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    secret: &GoogleDriveSecret,
) -> CommandResult<Option<String>> {
    let mut storage = GoogleDriveBootstrapStorage::new(app_data_dir, library, secret)?;
    super::bootstrap::bootstrap_remote_library(
        super::bootstrap::BootstrapMode::CreateOrOpen,
        library,
        &mut storage,
    )
}

pub(crate) fn refresh_existing_google_drive_library(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    secret: &GoogleDriveSecret,
) -> CommandResult<Option<String>> {
    let mut storage = GoogleDriveBootstrapStorage::new(app_data_dir, library, secret)?;
    super::bootstrap::bootstrap_remote_library(
        super::bootstrap::BootstrapMode::RequireExisting,
        library,
        &mut storage,
    )
}

pub(crate) fn google_drive_delete_entry(
    app_data_dir: &Path,
    secret: &mut GoogleDriveSecret,
    file_id: &str,
) -> CommandResult<()> {
    let url = google_drive_api_url(&format!("/drive/v3/files/{file_id}"))?;
    let response = google_drive_authorized_request(app_data_dir, secret, Method::DELETE, url)?
        .send_network("delete Google Drive entry")?;
    match response.status() {
        reqwest::StatusCode::NO_CONTENT | reqwest::StatusCode::NOT_FOUND => Ok(()),
        status => Err(CommandError::from(LibraryError::Internal(format!(
            "Google Drive delete failed with status {status}"
        )))),
    }
}

pub(crate) struct GoogleDriveProvider<'a> {
    app_data_dir: &'a Path,
    secret: RefCell<GoogleDriveSecret>,
    library: &'a RegisteredLibrary,
}

impl<'a> GoogleDriveProvider<'a> {
    pub(crate) fn new(
        app_data_dir: &'a Path,
        secret: GoogleDriveSecret,
        library: &'a RegisteredLibrary,
    ) -> Self {
        Self {
            app_data_dir,
            secret: RefCell::new(secret),
            library,
        }
    }
}

impl RemoteProvider for GoogleDriveProvider<'_> {
    fn capabilities(&self) -> RemoteProviderCapabilities {
        // Google Drive API v3 does NOT support server-enforced conditional
        // updates: the `etag` field is "n/a" in v3 (it was present in v2), and
        // `If-Match` headers are ignored by the `files.update` endpoint. The
        // `headRevisionId` is read-only metadata with no server-enforced
        // precondition check. See:
        // https://stackoverflow.com/questions/79865579/is-raceless-optimistic-concurrency-possible-in-google-drive-v3-interface
        //
        // Because we cannot enforce compare-and-swap, Google Drive is
        // READ-ONLY for safe writes: reads + caching still work, but
        // publication is blocked with `ProviderCapabilityUnavailable` rather
        // than silently downgrading to last-writer-wins.
        RemoteProviderCapabilities {
            conditional_replace: false,
            // Google Drive supports resumable uploads via
            // `uploadType=resumable` (session URL + offset query) and Range
            // requests on the `files.get?alt=media` endpoint. The trait
            // methods `resumable_upload_bytes` and `download_range` are wired
            // to the helper functions below.
            resumable_upload: true,
            range_download: true,
            revision_metadata: true,
            server_side_move: false,
        }
    }

    fn stat(&self, relative_path: &str) -> CommandResult<Option<RemoteObjectMetadata>> {
        let mut secret = self.secret.borrow_mut();
        let root_folder_id = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        Ok(google_drive_find_relative_entry(
            self.app_data_dir,
            &mut secret,
            root_folder_id,
            relative_path,
        )?
        .map(|metadata| RemoteObjectMetadata {
            size_bytes: metadata.size_bytes,
            revision: metadata.head_revision_id.or(metadata.modified_time),
        }))
    }

    fn get_revision(&self, relative_path: &str) -> CommandResult<Option<String>> {
        let mut secret = self.secret.borrow_mut();
        let root_folder_id = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        Ok(google_drive_find_relative_entry(
            self.app_data_dir,
            &mut secret,
            root_folder_id,
            relative_path,
        )?
        .and_then(|metadata| metadata.head_revision_id.or(metadata.modified_time)))
    }

    fn download_file(&self, relative_path: &str, destination: &Path) -> CommandResult<()> {
        let mut secret = self.secret.borrow_mut();
        let root_folder_id = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        let entry = google_drive_find_relative_entry(
            self.app_data_dir,
            &mut secret,
            root_folder_id,
            relative_path,
        )?
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "remote file {relative_path} was not found"
            )))
        })?;
        google_drive_download_file(self.app_data_dir, &mut secret, &entry.id, destination)
    }

    fn upload_file(&self, relative_path: &str) -> CommandResult<()> {
        let secret = self.secret.borrow();
        let root_folder_id = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        google_drive_upload_relative_file_to_remote(
            self.app_data_dir,
            self.library,
            &secret,
            relative_path,
            root_folder_id,
        )
    }

    fn delete_path(&self, relative_path: &str) -> CommandResult<()> {
        let mut secret = self.secret.borrow_mut();
        let root_folder_id = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        if relative_path.is_empty() {
            return google_drive_delete_entry(self.app_data_dir, &mut secret, root_folder_id);
        }
        let Some(entry) = google_drive_find_relative_entry(
            self.app_data_dir,
            &mut secret,
            root_folder_id,
            relative_path,
        )?
        else {
            return Ok(());
        };
        google_drive_delete_entry(self.app_data_dir, &mut secret, &entry.id)
    }

    fn initialize_or_sync(&self) -> CommandResult<Option<String>> {
        let secret = self.secret.borrow();
        initialize_or_sync_google_drive_library(self.app_data_dir, self.library, &secret)
    }

    fn create_range_fetcher(
        &self,
        relative_path: &str,
    ) -> CommandResult<Option<Box<dyn crate::audio::remote_source::HttpFetcher>>> {
        let mut secret = self.secret.borrow_mut();
        let root_folder_id = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        let entry = google_drive_find_relative_entry(
            self.app_data_dir,
            &mut secret,
            root_folder_id,
            relative_path,
        )?
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "remote file {relative_path} was not found"
            )))
        })?;

        let url = format!(
            "https://www.googleapis.com/drive/v3/files/{}?alt=media",
            entry.id
        );
        let token = google_drive_refresh_access_token(self.app_data_dir, &mut secret)?;
        let headers = vec![("Authorization".to_owned(), format!("Bearer {token}"))];

        let app_data_dir = self.app_data_dir.to_path_buf();
        let library = self.library.clone();
        Ok(Some(Box::new(
            crate::audio::remote_source::ProviderFetcher::new(url, headers)
                .with_token_refresh(move || refresh_google_drive_token(&app_data_dir, &library)),
        )))
    }

    fn get_file_size(&self, relative_path: &str) -> CommandResult<Option<u64>> {
        let mut secret = self.secret.borrow_mut();
        let root_folder_id = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        let entry = google_drive_find_relative_entry(
            self.app_data_dir,
            &mut secret,
            root_folder_id,
            relative_path,
        )?;
        Ok(entry.and_then(|e| e.size_bytes))
    }

    fn refresh_existing(&self) -> CommandResult<Option<String>> {
        let secret = self.secret.borrow();
        refresh_existing_google_drive_library(self.app_data_dir, self.library, &secret)
    }

    fn resumable_upload_bytes(
        &self,
        relative_path: &str,
        bytes: &[u8],
        operation_id: &str,
        control_db: &rusqlite::Connection,
    ) -> RemoteResult<()> {
        let mut secret = self.secret.borrow_mut();
        let root_folder_id = self.library.remote_root_locator().ok_or_else(|| {
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                "remote repository is missing a remote locator",
            )
        })?;

        // Resolve or create the remote file: walk parent folders (creating
        // them as needed), then find-or-create the leaf file metadata. If the
        // file already exists we overwrite it resumably via its file_id;
        // otherwise we start a new-file resumable session against the parent
        // folder.
        let segments: Vec<&str> = relative_path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        if segments.is_empty() {
            return Ok(());
        }
        let file_name = segments.last().copied().unwrap_or_default();
        let mut parent_id = root_folder_id.to_owned();
        for segment in &segments[..segments.len() - 1] {
            let folder = google_drive_get_or_create_folder(
                self.app_data_dir,
                &mut secret,
                &parent_id,
                segment,
            )
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
            parent_id = folder.id;
        }
        let existing_file =
            google_drive_find_child(self.app_data_dir, &mut secret, &parent_id, file_name, None)
                .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
        let file_id = existing_file.as_ref().map(|f| f.id.clone());

        // Look for an interrupted session persisted in the control DB so a
        // restart can resume from the committed offset instead of restarting
        // the upload from byte 0.
        let existing_session = list_transfer_parts(control_db, operation_id)
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?
            .into_iter()
            .find(|row| {
                row.relative_path == relative_path && row.direction == TransferDirection::Upload
            })
            .and_then(|row| {
                row.provider_session_id
                    .map(|url| (url, row.transferred_bytes as u64))
            });

        let total = bytes.len() as u64;
        let progress = |session_url: &str, committed: u64| {
            let _ = upsert_transfer_part(
                control_db,
                &TransferPartRow {
                    operation_id: operation_id.to_owned(),
                    relative_path: relative_path.to_owned(),
                    direction: TransferDirection::Upload,
                    expected_size: Some(total as i64),
                    expected_digest: None,
                    provider_revision: None,
                    provider_session_id: Some(session_url.to_owned()),
                    transferred_bytes: committed as i64,
                    state: if committed >= total {
                        "complete".to_owned()
                    } else {
                        "in_progress".to_owned()
                    },
                    updated_at_ms: current_unix_time_ms(),
                },
            );
        };

        google_drive_resumable_upload(
            self.app_data_dir,
            &mut secret,
            bytes,
            existing_session
                .as_ref()
                .map(|(url, off)| (url.as_str(), *off)),
            if file_id.is_some() {
                None
            } else {
                Some(&parent_id)
            },
            if file_id.is_some() {
                None
            } else {
                Some(file_name)
            },
            file_id.as_deref(),
            &progress,
        )
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;

        // The upload committed successfully; drop the persisted progress so a
        // future restart does not resume against a stale partial.
        delete_transfer_parts(control_db, operation_id)
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
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

        let mut secret = self.secret.borrow_mut();
        let root_folder_id = self.library.remote_root_locator().ok_or_else(|| {
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                "remote repository is missing a remote locator",
            )
        })?;
        let entry = google_drive_find_relative_entry(
            self.app_data_dir,
            &mut secret,
            root_folder_id,
            relative_path,
        )
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?
        .ok_or_else(|| {
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("remote file {relative_path} was not found"),
            )
        })?;

        let token = google_drive_refresh_access_token(self.app_data_dir, &mut secret)
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
        let url = google_drive_api_url(&format!("/drive/v3/files/{}?alt=media", entry.id))
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
        let end = offset + length - 1;
        let mut response = google_drive_request_with_access_token(&token, Method::GET, url)
            .header("Range", format!("bytes={offset}-{end}"))
            .send_network("download Google Drive file range")
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;

        // Validate the response status. A Range request must return 206
        // Partial Content. A 200 OK means the server ignored the Range
        // header — only acceptable when offset == 0 (full-body fallback).
        let status = response.status();
        if status == reqwest::StatusCode::OK && offset > 0 {
            return Err(RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!(
                    "Google Drive range download returned 200 OK for nonzero offset {offset} \
                     — server ignored Range header"
                ),
            ));
        }
        if !status.is_success() {
            return Err(RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("Google Drive range download failed with status {status}"),
            ));
        }

        // 206 Partial Content MUST include a matching Content-Range.
        if status == reqwest::StatusCode::PARTIAL_CONTENT {
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
                    format!("failed to create {}: {}", parent.display(), error),
                )
            })?;
        }
        // Open for write at a specific offset. We intentionally do NOT
        // truncate — the file may already contain bytes from a prior range
        // download, and truncating would destroy them.
        #[allow(clippy::suspicious_open_options)]
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(destination)
            .map_err(|error| {
                RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!("failed to open {}: {}", destination.display(), error),
                )
            })?;
        file.seek(SeekFrom::Start(offset)).map_err(|error| {
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("failed to seek {}: {}", destination.display(), error),
            )
        })?;

        // Stream the body straight to disk as it arrives instead of buffering
        // the whole chunk with `response.bytes()`. The buffered call imposed
        // one total-body deadline per 8 MiB chunk; streaming makes the client
        // timeout a per-read idle timeout so slow links make progress, and an
        // interruption leaves the received bytes durable for sub-chunk resume
        // (issue #205).
        let written = crate::remote::net_policy::stream_response_body(&mut response, &mut file)
            .map_err(|error| {
                RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!("failed to stream Google Drive range response: {error}"),
                )
            })?;

        // Validate body length. For a 206 the body must be exactly `length`
        // bytes; for a 200 full-body fallback (offset == 0 only) it must be at
        // least `length`. A truncated transfer is a transport failure, not
        // corruption, so it stays retryable and the partial bytes on disk are
        // preserved for resume.
        if status == reqwest::StatusCode::PARTIAL_CONTENT {
            if written != length {
                return Err(RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!(
                        "Google Drive range download body length mismatch: \
                         requested {length} bytes at offset {offset}, got {written} bytes"
                    ),
                ));
            }
        } else if written < length {
            return Err(RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!(
                    "Google Drive full-body response shorter than requested range: \
                     requested {length} bytes, got {written} bytes"
                ),
            ));
        }
        Ok(written)
    }
}

use super::provider::RemoteProvider;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::types::BundledGoogleDriveInstalledClient;
    use tempfile::tempdir;

    #[test]
    fn resolves_google_drive_credentials_from_non_ui_source() {
        let credentials = google_drive_provider_credentials_from_env(
            Some("client-123.apps.googleusercontent.com".to_owned()),
            Some("secret-456".to_owned()),
        )
        .expect("credentials should resolve");
        assert_eq!(
            credentials.client_id,
            "client-123.apps.googleusercontent.com"
        );
        assert_eq!(credentials.client_secret.as_deref(), Some("secret-456"));
    }

    #[test]
    fn resolves_google_drive_credentials_from_bundled_resource_before_env() {
        let temp_dir = tempdir().expect("temp dir should create");
        let oauth_dir = temp_dir.path().join("oauth");
        fs::create_dir_all(&oauth_dir).expect("oauth directory should create");
        fs::write(
            oauth_dir.join("google-drive-client.json"),
            serde_json::to_vec(&BundledGoogleDriveOAuthClientFile {
                installed: BundledGoogleDriveInstalledClient {
                    client_id: "stored-client.apps.googleusercontent.com".to_owned(),
                    client_secret: Some("stored-secret".to_owned()),
                },
            })
            .expect("oauth file should serialize"),
        )
        .expect("oauth file should write");

        let credentials = resolve_google_drive_provider_credentials(temp_dir.path())
            .expect("credentials should resolve");
        assert_eq!(
            credentials.client_id,
            "stored-client.apps.googleusercontent.com"
        );
        assert_eq!(credentials.client_secret.as_deref(), Some("stored-secret"));
    }

    #[test]
    fn google_drive_credentials_require_a_client_id() {
        let error = google_drive_provider_credentials_from_env(None, None)
            .expect_err("missing client id should fail");
        assert!(error.message.contains(GOOGLE_DRIVE_CLIENT_ID_ENV));
    }

    #[test]
    fn google_drive_auth_url_uses_loopback_pkce_and_offline_access() {
        let session = GoogleDriveSessionData {
            client_id: "client-123.apps.googleusercontent.com".to_owned(),
            client_secret: Some("secret-456".to_owned()),
            code_verifier: "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJK".to_owned(),
            redirect_uri: "http://127.0.0.1:43123/oauth2/callback".to_owned(),
            state_token: "state-123".to_owned(),
            root_folder_id: None,
            access_token: None,
            refresh_token: None,
            access_token_expires_at_ms: None,
        };

        let url = build_google_drive_authorization_url(&session).expect("url should build");
        let parsed = Url::parse(&url).expect("auth url should parse");
        let query: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some("accounts.google.com"));
        assert_eq!(query.get("client_id"), Some(&session.client_id));
        assert_eq!(query.get("response_type"), Some(&"code".to_owned()));
        assert_eq!(query.get("redirect_uri"), Some(&session.redirect_uri));
        assert_eq!(
            query.get("scope"),
            Some(&GOOGLE_DRIVE_OAUTH_SCOPE.to_owned())
        );
        assert_eq!(query.get("access_type"), Some(&"offline".to_owned()));
        assert_eq!(query.get("prompt"), Some(&"consent".to_owned()));
        assert_eq!(query.get("code_challenge_method"), Some(&"S256".to_owned()));
        assert_eq!(query.get("state"), Some(&session.state_token));
    }
}
