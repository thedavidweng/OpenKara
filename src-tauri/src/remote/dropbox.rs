use crate::{
    commands::error::{internal_error, CommandError, CommandResult},
    config::RegisteredLibrary,
    library::error::LibraryError,
    remote::errors::{
        remote_error_from_status, verify_content_range, RemoteError, RemoteErrorKind,
        RemoteObjectMetadata, RemoteProviderCapabilities, RemoteResult,
    },
    remote::provider::ConditionalSource,
};
use reqwest::{Method, StatusCode, Url};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    io::Write,
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
        current_unix_time_ms, load_remote_credential, slugify_display_name,
        store_remote_credential, stored_dropbox_app_key, BundledDropboxOAuthClientFile,
        DropboxCreateFolderResponse, DropboxMetadata, DropboxProviderCredentials, DropboxSecret,
        DropboxSessionData, DropboxTokenResponse, RemoteAuthSession, RemoteAuthState,
        StoredDropboxSecret, DROPBOX_APP_KEY_ENV, DROPBOX_APP_SECRET_ENV,
        DROPBOX_FIXED_REDIRECT_PORT, DROPBOX_FIXED_REDIRECT_URI,
        DROPBOX_OAUTH_CLIENT_RESOURCE_PATH,
    },
};

const DROPBOX_REMOTE_LIBRARY_OAUTH_SCOPE: &str =
    "files.metadata.read files.content.read files.content.write";

pub(crate) fn build_dropbox_authorization_url(
    session: &DropboxSessionData,
) -> CommandResult<String> {
    let mut url = Url::parse("https://www.dropbox.com/oauth2/authorize").map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to build Dropbox auth URL: {error}"
        )))
    })?;
    url.query_pairs_mut()
        .append_pair("client_id", &session.app_key)
        .append_pair("redirect_uri", &session.redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("token_access_type", "offline")
        .append_pair("scope", DROPBOX_REMOTE_LIBRARY_OAUTH_SCOPE)
        .append_pair(
            "code_challenge",
            &oauth_pkce_code_challenge(&session.code_verifier),
        )
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &session.state_token);
    Ok(url.to_string())
}

pub(crate) fn dropbox_provider_credentials_from_env(
    app_key: Option<String>,
    app_secret: Option<String>,
) -> CommandResult<DropboxProviderCredentials> {
    let Some(app_key) = app_key.filter(|value| !value.trim().is_empty()) else {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox is not available because the official app credential is missing. Set {DROPBOX_APP_KEY_ENV} before starting OpenKara."
        ))));
    };

    Ok(DropboxProviderCredentials {
        app_key,
        app_secret: app_secret.filter(|value| !value.trim().is_empty()),
    })
}

fn load_dropbox_provider_credentials_from_resource_dir(
    resource_dir: &Path,
) -> CommandResult<Option<DropboxProviderCredentials>> {
    let path = resource_dir.join(DROPBOX_OAUTH_CLIENT_RESOURCE_PATH);
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to read bundled Dropbox OAuth client metadata at {}: {error}",
            path.display()
        )))
    })?;
    let bundled: BundledDropboxOAuthClientFile = serde_json::from_str(&raw).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse bundled Dropbox OAuth client metadata at {}: {error}",
            path.display()
        )))
    })?;

    dropbox_provider_credentials_from_env(Some(bundled.app_key), bundled.app_secret).map(Some)
}

fn resolve_dropbox_provider_credentials(
    resource_dir: &Path,
) -> CommandResult<DropboxProviderCredentials> {
    if let Some(credentials) = load_dropbox_provider_credentials_from_resource_dir(resource_dir)? {
        return Ok(credentials);
    }

    dropbox_provider_credentials_from_env(
        env_optional(DROPBOX_APP_KEY_ENV),
        env_optional(DROPBOX_APP_SECRET_ENV),
    )
}

pub(crate) fn parse_dropbox_payload(
    resource_dir: &Path,
    _payload: Option<serde_json::Value>,
) -> CommandResult<DropboxSessionData> {
    let credentials = resolve_dropbox_provider_credentials(resource_dir)?;

    Ok(DropboxSessionData {
        app_key: credentials.app_key,
        app_secret: credentials.app_secret,
        code_verifier: random_token(64),
        redirect_uri: String::new(),
        state_token: random_token(48),
        access_token: None,
        refresh_token: None,
        access_token_expires_at_ms: None,
    })
}

pub(crate) fn store_dropbox_secret(
    app_data_dir: &Path,
    secret: DropboxSecret,
) -> CommandResult<()> {
    store_remote_credential(
        app_data_dir,
        &secret.library_id,
        &StoredDropboxSecret {
            refresh_token: secret.refresh_token,
            access_token: secret.access_token,
            access_token_expires_at_ms: secret.access_token_expires_at_ms,
            app_secret: secret.app_secret,
        },
    )
}

pub(crate) fn load_dropbox_secret(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<DropboxSecret> {
    if let Some(secret) = load_remote_credential::<StoredDropboxSecret>(app_data_dir, library.id())?
    {
        let app_key = stored_dropbox_app_key(library)?;
        return Ok(DropboxSecret {
            library_id: library.id().to_owned(),
            app_key,
            app_secret: secret.app_secret,
            access_token: secret.access_token,
            refresh_token: secret.refresh_token,
            access_token_expires_at_ms: secret.access_token_expires_at_ms,
        });
    }
    Err(CommandError::from(LibraryError::Internal(
        "missing stored credentials for the remote repository".to_owned(),
    )))
}

fn dropbox_api_url(path: &str) -> CommandResult<Url> {
    Url::parse(&format!("https://api.dropboxapi.com{path}")).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to build Dropbox URL: {error}"
        )))
    })
}

fn dropbox_content_url(path: &str) -> CommandResult<Url> {
    Url::parse(&format!("https://content.dropboxapi.com{path}")).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to build Dropbox content URL: {error}"
        )))
    })
}

/// Process-wide lock that serializes Dropbox token refresh. Without this,
/// concurrent provider instances (each with their own secret copy loaded from
/// disk) would all fire refresh requests simultaneously when the token
/// expires, wasting network round-trips and risking rate limits.
static DROPBOX_REFRESH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn dropbox_refresh_access_token(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
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
    // performs the network refresh. Other threads wait for the lock; when
    // they acquire it, they re-check the token expiry (the first thread may
    // have already refreshed and stored the new token to disk).
    let lock = DROPBOX_REFRESH_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().map_err(|_| {
        CommandError::from(LibraryError::Internal(
            "Dropbox refresh lock was poisoned".to_owned(),
        ))
    })?;

    // Re-check after acquiring the lock — another thread may have refreshed
    // while we waited. Reload the stored credential from disk to pick up the
    // refreshed token. The in-memory secret is per-provider-instance, so
    // without this reload the waiter would see its stale copy and fire a
    // redundant refresh request.
    if let Ok(Some(stored)) =
        load_remote_credential::<StoredDropboxSecret>(app_data_dir, &secret.library_id)
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
        ("client_id", secret.app_key.clone()),
        ("refresh_token", secret.refresh_token.clone()),
        ("grant_type", "refresh_token".to_owned()),
    ];
    if let Some(app_secret) = secret.app_secret.clone() {
        params.push(("client_secret", app_secret));
    }

    let body = form_urlencoded_body(&params)?;

    let response = crate::remote::net_policy::shared_http_client()
        .post("https://api.dropboxapi.com/oauth2/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .map_err(|e| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to refresh Dropbox access token: {}",
                e.without_url()
            )))
        })?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox token refresh failed with status {}",
            response.status()
        ))));
    }

    let body: DropboxTokenResponse = response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse Dropbox token response: {error}"
        )))
    })?;
    secret.access_token = body.access_token;
    secret.access_token_expires_at_ms = body
        .expires_in
        .map(|seconds| current_unix_time_ms() + seconds * 1000);
    store_dropbox_secret(app_data_dir, secret.clone())?;
    Ok(secret.access_token.clone())
}

/// Used as a callback by `ProviderFetcher` for automatic token renewal on 403.
fn refresh_dropbox_token(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> Result<String, crate::audio::remote_source::FetchError> {
    let mut secret = load_dropbox_secret(app_data_dir, library)
        .map_err(|e| crate::audio::remote_source::FetchError::Cache(e.message))?;
    dropbox_refresh_access_token(app_data_dir, &mut secret)
        .map_err(|e| crate::audio::remote_source::FetchError::Cache(e.message))
}

fn dropbox_authorized_request(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    method: Method,
    url: Url,
) -> CommandResult<reqwest::blocking::RequestBuilder> {
    let token = dropbox_refresh_access_token(app_data_dir, secret)?;
    Ok(crate::remote::net_policy::shared_http_client()
        .request(method, url)
        .bearer_auth(token))
}

fn dropbox_request_with_access_token(
    access_token: &str,
    method: Method,
    url: Url,
) -> reqwest::blocking::RequestBuilder {
    crate::remote::net_policy::shared_http_client()
        .request(method, url)
        .bearer_auth(access_token)
}

fn dropbox_exchange_code_for_tokens(
    session: &DropboxSessionData,
    code: &str,
) -> CommandResult<DropboxTokenResponse> {
    let mut params = vec![
        ("client_id", session.app_key.clone()),
        ("code", code.to_owned()),
        ("code_verifier", session.code_verifier.clone()),
        ("grant_type", "authorization_code".to_owned()),
        ("redirect_uri", session.redirect_uri.clone()),
    ];
    if let Some(app_secret) = session.app_secret.clone() {
        params.push(("client_secret", app_secret));
    }

    let body = form_urlencoded_body(&params)?;

    let response = crate::remote::net_policy::shared_http_client()
        .post("https://api.dropboxapi.com/oauth2/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to exchange Dropbox auth code: {error}"
            )))
        })?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox auth code exchange failed with status {}",
            response.status()
        ))));
    }
    response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse Dropbox token response: {error}"
        )))
    })
}

pub(crate) fn spawn_dropbox_auth_worker(
    sessions: Arc<Mutex<HashMap<String, RemoteAuthSession>>>,
    session_id: String,
    session: DropboxSessionData,
) -> CommandResult<DropboxSessionData> {
    let listener = TcpListener::bind(SocketAddrV4::new(
        Ipv4Addr::LOCALHOST,
        DROPBOX_FIXED_REDIRECT_PORT,
    ))
    .map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to bind Dropbox OAuth listener on {DROPBOX_FIXED_REDIRECT_URI}: {error}"
        )))
    })?;
    let server = Server::from_listener(listener, None).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to start Dropbox OAuth listener: {error}"
        )))
    })?;

    let mut session = session;
    session.redirect_uri = DROPBOX_FIXED_REDIRECT_URI.to_owned();
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
                            state.error = Some(CommandError::from(LibraryError::Internal("Dropbox sign-in timed out before the browser returned to OpenKara."
                                    .to_owned(),)));
                        });
                        return;
                    }
                }
                Err(error) => {
                    update_remote_auth_session(&sessions, &session_id, |state| {
                        state.state = RemoteAuthState::Failed;
                        state.error = Some(CommandError::from(LibraryError::Internal(format!(
                            "Dropbox sign-in listener failed: {error}"
                        ))));
                    });
                    return;
                }
            }
        };

        let callback_url = format!(
            "http://127.0.0.1:{}{}",
            DROPBOX_FIXED_REDIRECT_PORT,
            request.url()
        );
        let parsed = match Url::parse(&callback_url) {
            Ok(parsed) => parsed,
            Err(error) => {
                let _ = request.respond(oauth_callback_response("Invalid OAuth callback."));
                update_remote_auth_session(&sessions, &session_id, |state| {
                    state.state = RemoteAuthState::Failed;
                    state.error = Some(CommandError::from(LibraryError::Internal(format!(
                        "failed to parse Dropbox OAuth callback: {error}"
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
                    "Dropbox sign-in failed because the OAuth state token did not match."
                        .to_owned(),
                )));
            });
            return;
        }

        if let Some(error) = query.get("error") {
            let _ = request.respond(oauth_callback_response(
                "Dropbox sign-in was cancelled or denied.",
            ));
            update_remote_auth_session(&sessions, &session_id, |state| {
                state.state = RemoteAuthState::Failed;
                state.error = Some(CommandError::from(LibraryError::Internal(format!(
                    "Dropbox sign-in failed: {error}"
                ))));
            });
            return;
        }

        let Some(code) = query.get("code") else {
            let _ = request.respond(oauth_callback_response(
                "Missing Dropbox authorization code.",
            ));
            update_remote_auth_session(&sessions, &session_id, |state| {
                state.state = RemoteAuthState::Failed;
                state.error = Some(CommandError::from(LibraryError::Internal(
                    "Dropbox sign-in did not return an authorization code.".to_owned(),
                )));
            });
            return;
        };

        match dropbox_exchange_code_for_tokens(&worker_session, code).and_then(|tokens| {
            let account_id = tokens.account_id.clone().ok_or_else(|| {
                CommandError::from(LibraryError::Internal(
                    "Dropbox token response did not include an account ID.".to_owned(),
                ))
            })?;
            Ok((tokens, account_id))
        }) {
            Ok((tokens, account_id)) => {
                let _ = request.respond(oauth_callback_response(
                    "OpenKara connected to Dropbox. You can return to the app.",
                ));
                update_remote_auth_session(&sessions, &session_id, |state| {
                    state.state = RemoteAuthState::Ready;
                    state.account_id = account_id;
                    state.session =
                        super::types::ProviderSessionData::Dropbox(DropboxSessionData {
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
                    "OpenKara could not finish Dropbox sign-in.",
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

pub(crate) fn normalize_dropbox_root_path(
    raw: Option<&str>,
    fallback_display_name: &str,
) -> String {
    let candidate = raw.unwrap_or_default().trim().trim_matches('/');
    let value = if candidate.is_empty() {
        slugify_display_name(fallback_display_name)
    } else {
        candidate.to_owned()
    };
    format!("/{}", value)
}

pub(crate) fn dropbox_join_path(root_path: &str, relative_path: &str) -> String {
    let relative = relative_path.trim_matches('/');
    if relative.is_empty() {
        root_path.to_owned()
    } else {
        format!("{}/{}", root_path.trim_end_matches('/'), relative)
    }
}

pub(crate) fn dropbox_metadata_revision(metadata: &DropboxMetadata) -> Option<String> {
    metadata.rev.clone().or(metadata.server_modified.clone())
}

pub(crate) fn dropbox_get_metadata(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    path: &str,
) -> CommandResult<Option<DropboxMetadata>> {
    let url = dropbox_api_url("/2/files/get_metadata")?;
    let response = dropbox_authorized_request(app_data_dir, secret, Method::POST, url)?
        .json(&serde_json::json!({ "path": path }))
        .send_network("Dropbox metadata lookup")?;
    match response.status() {
        StatusCode::OK => response.json().map(Some).map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to parse Dropbox metadata: {error}"
            )))
        }),
        StatusCode::CONFLICT => Ok(None),
        status => Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox metadata lookup failed with status {}",
            status
        )))),
    }
}

fn dropbox_get_metadata_with_token(
    access_token: &str,
    path: &str,
) -> CommandResult<Option<DropboxMetadata>> {
    let url = dropbox_api_url("/2/files/get_metadata")?;
    let response = dropbox_request_with_access_token(access_token, Method::POST, url)
        .json(&serde_json::json!({ "path": path }))
        .send_network("Dropbox metadata lookup")?;
    match response.status() {
        StatusCode::OK => response.json().map(Some).map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to parse Dropbox metadata: {error}"
            )))
        }),
        StatusCode::CONFLICT => Ok(None),
        status => Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox metadata lookup failed with status {}",
            status
        )))),
    }
}

fn dropbox_create_folder(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    path: &str,
) -> CommandResult<DropboxMetadata> {
    let url = dropbox_api_url("/2/files/create_folder_v2")?;
    let response = dropbox_authorized_request(app_data_dir, secret, Method::POST, url)?
        .json(&serde_json::json!({ "path": path, "autorename": false }))
        .send_network("create Dropbox folder")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox folder creation failed with status {}",
            response.status()
        ))));
    }
    response
        .json::<DropboxCreateFolderResponse>()
        .map(|body| body.metadata)
        .map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to parse Dropbox folder creation response: {error}"
            )))
        })
}

fn dropbox_create_folder_with_token(
    access_token: &str,
    path: &str,
) -> CommandResult<DropboxMetadata> {
    let url = dropbox_api_url("/2/files/create_folder_v2")?;
    let response = dropbox_request_with_access_token(access_token, Method::POST, url)
        .json(&serde_json::json!({ "path": path, "autorename": false }))
        .send_network("create Dropbox folder")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox folder creation failed with status {}",
            response.status()
        ))));
    }
    response
        .json::<DropboxCreateFolderResponse>()
        .map(|body| body.metadata)
        .map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to parse Dropbox folder creation response: {error}"
            )))
        })
}

fn dropbox_ensure_folder(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    path: &str,
) -> CommandResult<()> {
    let mut current = String::new();
    for segment in path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        current.push('/');
        current.push_str(segment);
        if dropbox_get_metadata(app_data_dir, secret, &current)?.is_none() {
            let _ = dropbox_create_folder(app_data_dir, secret, &current)?;
        }
    }
    Ok(())
}

pub(crate) fn dropbox_ensure_folder_with_token(
    access_token: &str,
    path: &str,
) -> CommandResult<()> {
    let mut current = String::new();
    for segment in path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        current.push('/');
        current.push_str(segment);
        if dropbox_get_metadata_with_token(access_token, &current)?.is_none() {
            let _ = dropbox_create_folder_with_token(access_token, &current)?;
        }
    }
    Ok(())
}

fn dropbox_upload_file_bytes(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    path: &str,
    bytes: Vec<u8>,
) -> CommandResult<DropboxMetadata> {
    let url = dropbox_content_url("/2/files/upload")?;
    let response = dropbox_authorized_request(app_data_dir, secret, Method::POST, url)?
        .header(
            "Dropbox-API-Arg",
            serde_json::json!({
                "path": path,
                "mode": "overwrite",
                "autorename": false,
                "mute": true,
                "strict_conflict": false
            })
            .to_string(),
        )
        .header("Content-Type", "application/octet-stream")
        .body(bytes)
        .send_network("upload Dropbox file bytes")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox file upload failed with status {}",
            response.status()
        ))));
    }
    response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse Dropbox upload response: {error}"
        )))
    })
}

/// Upload bytes to a Dropbox path with compare-and-swap semantics.
///
/// - `expected_revision = Some(rev)`: uses `mode: { ".tag": "update", "rev": rev }`.
///   Dropbox returns HTTP 409 with a `conflict` payload when the current rev
///   differs — mapped to [`RemoteErrorKind::RemoteConflict`].
/// - `expected_revision = None`: uses `mode: add` (conditional-create). A
///   pre-existing file yields HTTP 409 → `RemoteConflict`.
///
/// Returns the metadata (size + rev) of the committed object.
fn dropbox_conditional_upload(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    path: &str,
    bytes: Vec<u8>,
    expected_revision: Option<&str>,
) -> RemoteResult<DropboxMetadata> {
    let url = dropbox_content_url("/2/files/upload")
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
    let mode = match expected_revision {
        Some(rev) => serde_json::json!({ ".tag": "update", "rev": rev }),
        // `mode: add` fails with HTTP 409 if the file already exists — this is
        // the conditional-create semantics (first publication / migration).
        None => serde_json::json!({ ".tag": "add" }),
    };
    let response = dropbox_authorized_request(app_data_dir, secret, Method::POST, url)
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?
        .header(
            "Dropbox-API-Arg",
            serde_json::json!({
                "path": path,
                "mode": mode,
                "autorename": false,
                "mute": true,
                "strict_conflict": true
            })
            .to_string(),
        )
        .header("Content-Type", "application/octet-stream")
        .body(bytes)
        .send_network("Dropbox conditional upload")
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;

    if !response.status().is_success() {
        let status = response.status();
        // HTTP 409 is Dropbox's conflict signal for mode=add/update mismatches.
        if status == StatusCode::CONFLICT {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteConflict,
                format!("Dropbox conditional upload conflict for {path}"),
            ));
        }
        return Err(remote_error_from_status(
            status,
            "Dropbox conditional upload",
        ));
    }

    let metadata: DropboxMetadata = response.json().map_err(|e| {
        RemoteError::new(
            RemoteErrorKind::NetworkUnavailable,
            format!("failed to parse Dropbox upload response: {e}"),
        )
    })?;
    Ok(metadata)
}

// ---------------------------------------------------------------------------
// Resumable upload session (PR#5)
//
// Dropbox supports multi-part uploads via the upload_session endpoints:
//   start  → returns a session_id
//   append → appends chunks with a cursor {session_id, offset}
//   finish → final chunk + commit metadata (path, mode, etc.)
//
// The session_id and committed offset are persisted in
// `remote_transfer_parts` so a restart can resume from the verified offset
// (append_v2 with the correct cursor offset). A changed provider_revision
// invalidates the partial transfer — a new session is started.
//
// See: https://www.dropbox.com/developers/documentation/http/documentation
// ---------------------------------------------------------------------------

/// Chunk size for Dropbox resumable uploads. Dropbox recommends chunks between
/// 150 KB and 150 MB; we use 8 MiB to balance memory and round-trip count.
const DROPBOX_RESUMABLE_CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// Start a Dropbox upload session. Returns the `session_id` to persist in
/// `remote_transfer_parts.provider_session_id`.
pub(crate) fn dropbox_upload_session_start(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    first_chunk: &[u8],
) -> CommandResult<String> {
    let url = dropbox_content_url("/2/files/upload_session/start")?;
    let response = dropbox_authorized_request(app_data_dir, secret, Method::POST, url)?
        .header(
            "Dropbox-API-Arg",
            serde_json::json!({ "close": false }).to_string(),
        )
        .header("Content-Type", "application/octet-stream")
        .body(first_chunk.to_vec())
        .send_network("Dropbox upload session start")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox upload_session/start failed with status {}",
            response.status()
        ))));
    }
    let body: serde_json::Value = response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse Dropbox upload_session/start response: {error}"
        )))
    })?;
    body.get("session_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "Dropbox upload_session/start response missing session_id".to_owned(),
            ))
        })
}

/// Append a chunk to an existing Dropbox upload session. `offset` is the
/// committed byte offset within the session (must match the server's view).
pub(crate) fn dropbox_upload_session_append(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    session_id: &str,
    offset: u64,
    chunk: &[u8],
) -> CommandResult<()> {
    let url = dropbox_content_url("/2/files/upload_session/append_v2")?;
    let response = dropbox_authorized_request(app_data_dir, secret, Method::POST, url)?
        .header(
            "Dropbox-API-Arg",
            serde_json::json!({
                "cursor": {
                    "session_id": session_id,
                    "offset": offset
                },
                "close": false
            })
            .to_string(),
        )
        .header("Content-Type", "application/octet-stream")
        .body(chunk.to_vec())
        .send_network("Dropbox upload session append")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox upload_session/append_v2 failed with status {}",
            response.status()
        ))));
    }
    Ok(())
}

/// Finish a Dropbox upload session: upload the final chunk and commit the
/// file at `path` with `mode: overwrite`. Returns the committed metadata.
pub(crate) fn dropbox_upload_session_finish(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    session_id: &str,
    offset: u64,
    final_chunk: &[u8],
    commit_path: &str,
) -> CommandResult<DropboxMetadata> {
    let url = dropbox_content_url("/2/files/upload_session/finish")?;
    let response = dropbox_authorized_request(app_data_dir, secret, Method::POST, url)?
        .header(
            "Dropbox-API-Arg",
            serde_json::json!({
                "cursor": {
                    "session_id": session_id,
                    "offset": offset
                },
                "commit": {
                    "path": commit_path,
                    "mode": "overwrite",
                    "autorename": false,
                    "mute": true,
                    "strict_conflict": false
                }
            })
            .to_string(),
        )
        .header("Content-Type", "application/octet-stream")
        .body(final_chunk.to_vec())
        .send_network("Dropbox upload session finish")?;
    if !response.status().is_success() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox upload_session/finish failed with status {}",
            response.status()
        ))));
    }
    response.json().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to parse Dropbox upload_session/finish response: {error}"
        )))
    })
}

/// Upload `bytes` to `commit_path` using a resumable upload session. Splits
/// the data into chunks, persists progress via `progress` after each chunk,
/// and returns the committed metadata.
///
/// `existing_session` is `(session_id, offset)` from a prior interrupted run
/// (read from `remote_transfer_parts`). When present, the upload resumes from
/// the verified offset instead of starting a new session.
pub(crate) fn dropbox_resumable_upload(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    commit_path: &str,
    bytes: &[u8],
    existing_session: Option<(&str, u64)>,
    progress: &dyn Fn(&str, u64),
) -> CommandResult<DropboxMetadata> {
    let total = bytes.len() as u64;

    let (session_id, mut offset) = match existing_session {
        Some((sid, off)) if off > 0 && off < total => {
            // Resume from the verified offset. The caller is responsible for
            // verifying the provider_revision has not changed before calling.
            (sid.to_owned(), off)
        }
        _ => {
            // Start a new session with the first chunk.
            let first_chunk_end = (DROPBOX_RESUMABLE_CHUNK_SIZE).min(bytes.len());
            let session_id =
                dropbox_upload_session_start(app_data_dir, secret, &bytes[..first_chunk_end])?;
            let offset = first_chunk_end as u64;
            progress(&session_id, offset);
            (session_id, offset)
        }
    };

    // Append intermediate chunks.
    while offset < total {
        let chunk_end = (offset as usize + DROPBOX_RESUMABLE_CHUNK_SIZE).min(bytes.len());
        let chunk = &bytes[offset as usize..chunk_end];

        if chunk_end == bytes.len() {
            // Final chunk — finish the session and commit.
            let metadata = dropbox_upload_session_finish(
                app_data_dir,
                secret,
                &session_id,
                offset,
                chunk,
                commit_path,
            )?;
            return Ok(metadata);
        }

        dropbox_upload_session_append(app_data_dir, secret, &session_id, offset, chunk)?;
        offset = chunk_end as u64;
        progress(&session_id, offset);
    }

    // Edge case: the file was exactly one chunk. The session was started with
    // the full content but never finished. Finish with an empty final chunk.
    let metadata =
        dropbox_upload_session_finish(app_data_dir, secret, &session_id, offset, &[], commit_path)?;
    Ok(metadata)
}

pub(crate) fn dropbox_download_file(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    path: &str,
    destination: &Path,
) -> CommandResult<()> {
    use crate::remote::errors::{remote_error_from_status, RemoteError, RemoteErrorKind};
    use crate::remote::send_with_retry;

    let url = dropbox_content_url("/2/files/download")?;
    let path_owned = path.to_owned();
    // Rebuild the authorized request on every attempt so the shared network
    // policy can retry transport failures and rate-limits.
    let response = send_with_retry("download Dropbox file", || {
        let builder = dropbox_authorized_request(app_data_dir, secret, Method::POST, url.clone())
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?
            .header(
                "Dropbox-API-Arg",
                serde_json::json!({ "path": path_owned }).to_string(),
            );
        Ok(builder)
    })
    .map_err(|e| e.to_command_error())?;
    if !response.status().is_success() {
        return Err(
            remote_error_from_status(response.status(), "Dropbox download").to_command_error(),
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
    let bytes = response.bytes().map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to read Dropbox response: {error}"
        )))
    })?;
    let mut file = fs::File::create(destination).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to create {}: {error}",
            destination.display()
        )))
    })?;
    file.write_all(bytes.as_ref()).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to write {}: {error}",
            destination.display()
        )))
    })?;
    Ok(())
}

pub(crate) fn dropbox_upload_relative_file_to_remote(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    secret: &DropboxSecret,
    relative_path: &str,
    root_path: &str,
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
    if let Some(parent) = Path::new(relative_path).parent() {
        let parent_path = parent.to_string_lossy().replace('\\', "/");
        if !parent_path.is_empty() {
            dropbox_ensure_folder(
                app_data_dir,
                &mut secret,
                &dropbox_join_path(root_path, &parent_path),
            )?;
        }
    }
    let remote_path = dropbox_join_path(root_path, relative_path);
    let _ = dropbox_upload_file_bytes(app_data_dir, &mut secret, &remote_path, bytes)?;
    Ok(())
}

// Kept for RemoteProvider::upload_directory; CAS publish no longer bulk-uploads.
#[allow(dead_code)]
pub(crate) fn dropbox_upload_directory_to_remote(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    secret: &DropboxSecret,
    relative_directory: &str,
    root_path: &str,
) -> CommandResult<()> {
    let local_root = library.working_copy_root().ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "remote repository is missing a cached working copy".to_string(),
        ))
    })?;
    let base = local_root.join(relative_directory);
    if !base.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&base).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to read {}: {error}",
            base.display()
        )))
    })? {
        let entry = entry.map_err(internal_error)?;
        let path = entry.path();
        let relative = path
            .strip_prefix(&local_root)
            .map_err(internal_error)?
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            dropbox_upload_directory_to_remote(
                app_data_dir,
                library,
                secret,
                &relative,
                root_path,
            )?;
        } else {
            dropbox_upload_relative_file_to_remote(
                app_data_dir,
                library,
                secret,
                &relative,
                root_path,
            )?;
        }
    }
    Ok(())
}

struct DropboxBootstrapStorage<'a> {
    app_data_dir: &'a Path,
    library: &'a RegisteredLibrary,
    secret: DropboxSecret,
    remote_root_path: &'a str,
}

impl<'a> DropboxBootstrapStorage<'a> {
    fn new(
        app_data_dir: &'a Path,
        library: &'a RegisteredLibrary,
        secret: &DropboxSecret,
    ) -> CommandResult<Self> {
        let remote_root_path = library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        Ok(Self {
            app_data_dir,
            library,
            secret: secret.clone(),
            remote_root_path,
        })
    }
}

impl super::bootstrap::RemoteBootstrapStorage for DropboxBootstrapStorage<'_> {
    fn location_label(&self) -> &'static str {
        "Dropbox folder"
    }

    fn ensure_layout(&mut self) -> CommandResult<()> {
        dropbox_ensure_folder(self.app_data_dir, &mut self.secret, self.remote_root_path)?;
        for directory in ["media", "media-g", "stems", "artwork"] {
            dropbox_ensure_folder(
                self.app_data_dir,
                &mut self.secret,
                &dropbox_join_path(self.remote_root_path, directory),
            )?;
        }
        Ok(())
    }

    fn marker_exists(&mut self) -> CommandResult<bool> {
        let marker_remote_path = dropbox_join_path(self.remote_root_path, ".openkara-library");
        Ok(
            dropbox_get_metadata(self.app_data_dir, &mut self.secret, &marker_remote_path)?
                .is_some(),
        )
    }

    fn upload_marker(&mut self, _marker_bytes: &[u8]) -> CommandResult<()> {
        dropbox_upload_relative_file_to_remote(
            self.app_data_dir,
            self.library,
            &self.secret,
            ".openkara-library",
            self.remote_root_path,
        )
    }

    fn probe_committed_database(
        &mut self,
    ) -> CommandResult<Option<super::bootstrap::CommittedDatabaseProbe>> {
        use super::bootstrap::CommittedDatabaseProbe;
        use crate::remote::manifest::{RepositoryManifest, MANIFEST_PATH};

        // Prefer the repository manifest: the generation-specific database is
        // the committed source of truth once a repository has been published
        // through the transactional protocol.
        let manifest_remote_path = dropbox_join_path(self.remote_root_path, MANIFEST_PATH);
        if let Some(metadata) =
            dropbox_get_metadata(self.app_data_dir, &mut self.secret, &manifest_remote_path)?
        {
            let temp_path = std::env::temp_dir().join(format!(
                "openkara-manifest-probe-{}.json",
                uuid::Uuid::new_v4()
            ));
            dropbox_download_file(
                self.app_data_dir,
                &mut self.secret,
                &manifest_remote_path,
                &temp_path,
            )?;
            let content = fs::read_to_string(&temp_path).map_err(|error| {
                let _ = fs::remove_file(&temp_path);
                CommandError::from(LibraryError::Internal(format!(
                    "failed to read Dropbox manifest: {error}"
                )))
            })?;
            let _ = fs::remove_file(&temp_path);
            let manifest: RepositoryManifest = serde_json::from_str(&content).map_err(|error| {
                CommandError::from(LibraryError::Internal(format!(
                    "failed to parse Dropbox repository manifest: {error}"
                )))
            })?;
            return Ok(Some(CommittedDatabaseProbe {
                revision: dropbox_metadata_revision(&metadata),
                database_path: manifest.database_path,
                generation: manifest.generation,
                database_size: Some(manifest.database_size),
                database_sha256: Some(manifest.database_sha256),
            }));
        }

        // Legacy repositories without a manifest: root openkara.db.
        let database_remote_path = dropbox_join_path(self.remote_root_path, "openkara.db");
        Ok(
            dropbox_get_metadata(self.app_data_dir, &mut self.secret, &database_remote_path)?.map(
                |metadata| CommittedDatabaseProbe {
                    revision: dropbox_metadata_revision(&metadata),
                    database_path: "openkara.db".to_owned(),
                    generation: 0,
                    database_size: metadata.size,
                    database_sha256: None,
                },
            ),
        )
    }

    fn download_database(&mut self, database_path: &str, destination: &Path) -> CommandResult<()> {
        let database_remote_path = dropbox_join_path(self.remote_root_path, database_path);
        dropbox_download_file(
            self.app_data_dir,
            &mut self.secret,
            &database_remote_path,
            destination,
        )
    }

    fn upload_database(&mut self, _source: &Path) -> CommandResult<Option<String>> {
        // One-time empty-repository seed only. Ongoing publication uses the
        // executor and generation-specific paths.
        dropbox_upload_relative_file_to_remote(
            self.app_data_dir,
            self.library,
            &self.secret,
            "openkara.db",
            self.remote_root_path,
        )?;
        let database_remote_path = dropbox_join_path(self.remote_root_path, "openkara.db");
        let metadata =
            dropbox_get_metadata(self.app_data_dir, &mut self.secret, &database_remote_path)?
                .ok_or_else(|| {
                    CommandError::from(LibraryError::Internal(
                        "Dropbox database upload succeeded but the file was not found afterwards"
                            .to_owned(),
                    ))
                })?;
        Ok(dropbox_metadata_revision(&metadata))
    }
}

pub(crate) fn initialize_or_sync_dropbox_library(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    secret: &DropboxSecret,
) -> CommandResult<Option<String>> {
    let mut storage = DropboxBootstrapStorage::new(app_data_dir, library, secret)?;
    super::bootstrap::bootstrap_remote_library(
        super::bootstrap::BootstrapMode::CreateOrOpen,
        library,
        &mut storage,
    )
}

pub(crate) fn refresh_existing_dropbox_library(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    secret: &DropboxSecret,
) -> CommandResult<Option<String>> {
    let mut storage = DropboxBootstrapStorage::new(app_data_dir, library, secret)?;
    super::bootstrap::bootstrap_remote_library(
        super::bootstrap::BootstrapMode::RequireExisting,
        library,
        &mut storage,
    )
}

pub(crate) fn dropbox_delete_path(
    app_data_dir: &Path,
    secret: &mut DropboxSecret,
    path: &str,
) -> CommandResult<()> {
    let url = dropbox_api_url("/2/files/delete_v2")?;
    let response = dropbox_authorized_request(app_data_dir, secret, Method::POST, url)?
        .json(&serde_json::json!({ "path": path }))
        .send_network("delete Dropbox path")?;
    match response.status() {
        StatusCode::OK | StatusCode::CONFLICT => Ok(()),
        status => Err(CommandError::from(LibraryError::Internal(format!(
            "Dropbox delete failed with status {status}"
        )))),
    }
}

pub(crate) struct DropboxProvider<'a> {
    app_data_dir: &'a Path,
    secret: RefCell<DropboxSecret>,
    library: &'a RegisteredLibrary,
}

impl<'a> DropboxProvider<'a> {
    pub(crate) fn new(
        app_data_dir: &'a Path,
        secret: DropboxSecret,
        library: &'a RegisteredLibrary,
    ) -> Self {
        Self {
            app_data_dir,
            secret: RefCell::new(secret),
            library,
        }
    }
}

impl RemoteProvider for DropboxProvider<'_> {
    fn capabilities(&self) -> RemoteProviderCapabilities {
        RemoteProviderCapabilities {
            conditional_replace: true,
            // Dropbox upload_session (start/append/finish) supports resumable
            // uploads with a persisted session_id + offset, and the content
            // download endpoint honors the Range header. The trait methods
            // `resumable_upload_bytes` and `download_range` are wired to the
            // helper functions below, so we advertise both capabilities.
            resumable_upload: true,
            range_download: true,
            revision_metadata: true,
            server_side_move: false,
        }
    }

    fn stat(&self, relative_path: &str) -> CommandResult<Option<RemoteObjectMetadata>> {
        let mut secret = self.secret.borrow_mut();
        let root_path = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        let remote_path = dropbox_join_path(root_path, relative_path);
        Ok(
            dropbox_get_metadata(self.app_data_dir, &mut secret, &remote_path)?.map(|m| {
                RemoteObjectMetadata {
                    size: m.size,
                    revision: dropbox_metadata_revision(&m),
                }
            }),
        )
    }

    fn conditional_replace(
        &self,
        relative_path: &str,
        source: ConditionalSource,
        expected_revision: Option<&str>,
    ) -> RemoteResult<RemoteObjectMetadata> {
        let mut secret = self.secret.borrow_mut();
        let root_path = self.library.remote_root_locator().ok_or_else(|| {
            RemoteError::new(
                RemoteErrorKind::ProviderCapabilityUnavailable,
                "remote repository is missing a remote locator",
            )
        })?;
        let remote_path = dropbox_join_path(root_path, relative_path);
        let bytes = source
            .read_bytes()
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
        let metadata = dropbox_conditional_upload(
            self.app_data_dir,
            &mut secret,
            &remote_path,
            bytes,
            expected_revision,
        )?;
        Ok(RemoteObjectMetadata {
            size: metadata.size,
            revision: dropbox_metadata_revision(&metadata),
        })
    }

    fn resumable_upload_bytes(
        &self,
        relative_path: &str,
        bytes: &[u8],
        operation_id: &str,
        control_db: &rusqlite::Connection,
    ) -> RemoteResult<()> {
        use crate::remote::control_db::{
            delete_transfer_parts, list_transfer_parts, upsert_transfer_part, TransferDirection,
            TransferPartRow,
        };

        let root_path = self.library.remote_root_locator().ok_or_else(|| {
            RemoteError::new(
                RemoteErrorKind::ProviderCapabilityUnavailable,
                "remote repository is missing a remote locator",
            )
        })?;
        let commit_path = dropbox_join_path(root_path, relative_path);
        let total = bytes.len() as u64;

        // Content identity for this upload. A session started for candidate X
        // must never be resumed with bytes from candidate Y.
        let expected_digest = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(bytes);
            crate::hash::hex_lower(hasher.finalize())
        };

        // Resume only when the persisted row matches size + digest identity.
        let existing_session = list_transfer_parts(control_db, operation_id)
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?
            .into_iter()
            .find(|row| {
                row.direction == TransferDirection::Upload && row.relative_path == relative_path
            })
            .and_then(|row| {
                let digest_ok = row.expected_digest.as_deref() == Some(expected_digest.as_str());
                let size_ok = row.expected_size == Some(total as i64);
                if !digest_ok || !size_ok {
                    // Invalidate the mismatched session before starting fresh.
                    let _ = delete_transfer_parts(control_db, operation_id);
                    return None;
                }
                match (row.provider_session_id, row.transferred_bytes) {
                    (Some(sid), off) if off > 0 => Some((sid, off as u64)),
                    _ => None,
                }
            });

        let mut secret = self.secret.borrow_mut();
        let digest_for_progress = expected_digest.clone();
        let progress = move |session_id: &str, offset: u64| {
            let row = TransferPartRow {
                operation_id: operation_id.to_owned(),
                relative_path: relative_path.to_owned(),
                direction: TransferDirection::Upload,
                expected_size: Some(total as i64),
                expected_digest: Some(digest_for_progress.clone()),
                provider_revision: None,
                provider_session_id: Some(session_id.to_owned()),
                transferred_bytes: offset as i64,
                state: "in_progress".to_owned(),
                updated_at_ms: current_unix_time_ms(),
            };
            let _ = upsert_transfer_part(control_db, &row);
        };

        dropbox_resumable_upload(
            self.app_data_dir,
            &mut secret,
            &commit_path,
            bytes,
            existing_session
                .as_ref()
                .map(|(sid, off)| (sid.as_str(), *off)),
            &progress,
        )
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;

        // Clear the transfer part so a future restart does not resume against a
        // non-existent remote partial.
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
        use std::io::{Seek, SeekFrom};

        if length == 0 {
            return Ok(0);
        }

        let mut secret = self.secret.borrow_mut();
        let root_path = self.library.remote_root_locator().ok_or_else(|| {
            RemoteError::new(
                RemoteErrorKind::ProviderCapabilityUnavailable,
                "remote repository is missing a remote locator",
            )
        })?;
        let remote_path = dropbox_join_path(root_path, relative_path);
        let url = dropbox_content_url("/2/files/download")
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
        // Dropbox's content download endpoint honors a standard HTTP Range
        // header: bytes=<start>-<end> (inclusive end).
        let range_value = format!("bytes={}-{}", offset, offset + length - 1);
        let response =
            dropbox_authorized_request(self.app_data_dir, &mut secret, Method::POST, url)
                .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?
                .header(
                    "Dropbox-API-Arg",
                    serde_json::json!({ "path": remote_path }).to_string(),
                )
                .header("Range", range_value)
                .send_network("download Dropbox range")
                .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;

        // Validate the response status. A Range request must return 206
        // Partial Content. A 200 OK means the server ignored the Range
        // header — only acceptable when offset == 0 (full-body fallback).
        let status = response.status();
        if status == reqwest::StatusCode::OK && offset > 0 {
            return Err(RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!(
                    "Dropbox range download returned 200 OK for nonzero offset {offset} \
                     — server ignored Range header"
                ),
            ));
        }
        if !status.is_success() {
            return Err(RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("Dropbox range download failed with status {status}"),
            ));
        }

        // A 206 Partial Content response MUST include a matching Content-Range.
        // Treating the header as optional allowed silent mis-ranged bodies.
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
            verify_content_range(cr_str, offset, length)?;
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!("failed to create {}: {error}", parent.display()),
                )
            })?;
        }
        let bytes = response.bytes().map_err(|error| {
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("failed to read Dropbox response: {error}"),
            )
        })?;

        // Validate body length: must match the requested range length.
        // A short response would leave gaps; an oversized response would
        // write beyond the requested range.
        let actual_len = bytes.len() as u64;
        if actual_len != length {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!(
                    "Dropbox range download body length mismatch: \
                     requested {length} bytes at offset {offset}, got {actual_len} bytes"
                ),
            ));
        }

        // Open for write at a specific offset. We intentionally do NOT
        // truncate — the file may already contain bytes from a prior range
        // download, and truncating would destroy them.
        #[allow(clippy::suspicious_open_options)]
        let mut file = std::fs::OpenOptions::new()
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
        file.write_all(bytes.as_ref()).map_err(|error| {
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("failed to write {}: {error}", destination.display()),
            )
        })?;
        Ok(actual_len)
    }

    fn get_revision(&self, relative_path: &str) -> CommandResult<Option<String>> {
        let mut secret = self.secret.borrow_mut();
        let root_path = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        let remote_path = dropbox_join_path(root_path, relative_path);
        Ok(
            dropbox_get_metadata(self.app_data_dir, &mut secret, &remote_path)?
                .as_ref()
                .and_then(dropbox_metadata_revision),
        )
    }

    fn download_file(&self, relative_path: &str, destination: &Path) -> CommandResult<()> {
        let mut secret = self.secret.borrow_mut();
        let root_path = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        let remote_path = dropbox_join_path(root_path, relative_path);
        if dropbox_get_metadata(self.app_data_dir, &mut secret, &remote_path)?.is_none() {
            return Err(CommandError::from(LibraryError::Internal(format!(
                "remote file {relative_path} was not found"
            ))));
        }
        dropbox_download_file(self.app_data_dir, &mut secret, &remote_path, destination)
    }

    fn upload_file(&self, relative_path: &str) -> CommandResult<()> {
        let secret = self.secret.borrow();
        let root_path = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        dropbox_upload_relative_file_to_remote(
            self.app_data_dir,
            self.library,
            &secret,
            relative_path,
            root_path,
        )
    }

    fn upload_directory(&self, relative_path: &str) -> CommandResult<()> {
        let secret = self.secret.borrow();
        let root_path = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        dropbox_upload_directory_to_remote(
            self.app_data_dir,
            self.library,
            &secret,
            relative_path,
            root_path,
        )
    }

    fn delete_path(&self, relative_path: &str) -> CommandResult<()> {
        let mut secret = self.secret.borrow_mut();
        let root_path = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        dropbox_delete_path(
            self.app_data_dir,
            &mut secret,
            &dropbox_join_path(root_path, relative_path),
        )
    }

    fn initialize_or_sync(&self) -> CommandResult<Option<String>> {
        let secret = self.secret.borrow();
        initialize_or_sync_dropbox_library(self.app_data_dir, self.library, &secret)
    }

    fn get_file_size(&self, relative_path: &str) -> CommandResult<Option<u64>> {
        let mut secret = self.secret.borrow_mut();
        let root_path = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        let remote_path = dropbox_join_path(root_path, relative_path);
        Ok(
            dropbox_get_metadata(self.app_data_dir, &mut secret, &remote_path)?
                .and_then(|m| m.size),
        )
    }

    fn create_range_fetcher(
        &self,
        relative_path: &str,
    ) -> CommandResult<Option<Box<dyn crate::audio::remote_source::HttpFetcher>>> {
        let mut secret = self.secret.borrow_mut();
        let root_path = self.library.remote_root_locator().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a remote locator".to_owned(),
            ))
        })?;
        let remote_path = dropbox_join_path(root_path, relative_path);
        let token = dropbox_refresh_access_token(self.app_data_dir, &mut secret)?;

        let url = "https://content.dropboxapi.com/2/files/download".to_owned();
        let headers = vec![("Authorization".to_owned(), format!("Bearer {token}"))];
        let api_arg = serde_json::json!({ "path": remote_path }).to_string();

        let app_data_dir = self.app_data_dir.to_path_buf();
        let library = self.library.clone();
        Ok(Some(Box::new(
            crate::audio::remote_source::ProviderFetcher::new(url, headers)
                .with_post(api_arg)
                .with_token_refresh(move || refresh_dropbox_token(&app_data_dir, &library)),
        )))
    }

    fn refresh_existing(&self) -> CommandResult<Option<String>> {
        let secret = self.secret.borrow();
        refresh_existing_dropbox_library(self.app_data_dir, self.library, &secret)
    }
}

use super::provider::RemoteProvider;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolves_dropbox_credentials_from_non_ui_source() {
        let credentials = dropbox_provider_credentials_from_env(
            Some("dropbox-app-key".to_owned()),
            Some("dropbox-app-secret".to_owned()),
        )
        .expect("credentials should resolve");
        assert_eq!(credentials.app_key, "dropbox-app-key");
        assert_eq!(
            credentials.app_secret.as_deref(),
            Some("dropbox-app-secret")
        );
    }

    #[test]
    fn resolves_dropbox_credentials_from_bundled_resource_before_env() {
        let temp_dir = tempdir().expect("temp dir should create");
        let oauth_dir = temp_dir.path().join("oauth");
        fs::create_dir_all(&oauth_dir).expect("oauth directory should create");
        fs::write(
            oauth_dir.join("dropbox-client.json"),
            serde_json::to_vec(&BundledDropboxOAuthClientFile {
                app_key: "stored-dropbox-app-key".to_owned(),
                app_secret: Some("stored-dropbox-app-secret".to_owned()),
            })
            .expect("oauth file should serialize"),
        )
        .expect("oauth file should write");

        let credentials = resolve_dropbox_provider_credentials(temp_dir.path())
            .expect("credentials should resolve");
        assert_eq!(credentials.app_key, "stored-dropbox-app-key");
        assert_eq!(
            credentials.app_secret.as_deref(),
            Some("stored-dropbox-app-secret")
        );
    }

    #[test]
    fn dropbox_credentials_require_an_app_key() {
        let error = dropbox_provider_credentials_from_env(None, None)
            .expect_err("missing app key should fail");
        assert!(error.message.contains(DROPBOX_APP_KEY_ENV));
    }

    #[test]
    fn dropbox_auth_url_uses_loopback_pkce_and_offline_access() {
        let session = DropboxSessionData {
            app_key: "dropbox-app-key".to_owned(),
            app_secret: Some("dropbox-app-secret".to_owned()),
            code_verifier: "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJK".to_owned(),
            redirect_uri: DROPBOX_FIXED_REDIRECT_URI.to_owned(),
            state_token: "state-123".to_owned(),
            access_token: None,
            refresh_token: None,
            access_token_expires_at_ms: None,
        };

        let url = build_dropbox_authorization_url(&session).expect("url should build");
        let parsed = Url::parse(&url).expect("auth url should parse");
        let query: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some("www.dropbox.com"));
        assert_eq!(query.get("client_id"), Some(&session.app_key));
        assert_eq!(query.get("response_type"), Some(&"code".to_owned()));
        assert_eq!(query.get("redirect_uri"), Some(&session.redirect_uri));
        assert_eq!(query.get("token_access_type"), Some(&"offline".to_owned()));
        assert_eq!(query.get("code_challenge_method"), Some(&"S256".to_owned()));
        assert_eq!(query.get("state"), Some(&session.state_token));
    }

    #[test]
    fn dropbox_auth_url_requests_only_required_remote_library_scopes() {
        let session = DropboxSessionData {
            app_key: "dropbox-app-key".to_owned(),
            app_secret: None,
            code_verifier: "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJK".to_owned(),
            redirect_uri: DROPBOX_FIXED_REDIRECT_URI.to_owned(),
            state_token: "state-123".to_owned(),
            access_token: None,
            refresh_token: None,
            access_token_expires_at_ms: None,
        };

        let url = build_dropbox_authorization_url(&session).expect("url should build");
        let parsed = Url::parse(&url).expect("auth url should parse");
        let query: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(
            query.get("scope"),
            Some(&"files.metadata.read files.content.read files.content.write".to_owned())
        );
    }

    #[test]
    fn dropbox_token_response_accepts_account_id_from_oauth_exchange() {
        let body = serde_json::json!({
            "access_token": "sl.short-lived",
            "expires_in": 14_400,
            "refresh_token": "refresh-token",
            "account_id": "dbid:account-1"
        });

        let token: DropboxTokenResponse =
            serde_json::from_value(body).expect("token response should parse");

        assert_eq!(token.account_id.as_deref(), Some("dbid:account-1"));
    }
}
