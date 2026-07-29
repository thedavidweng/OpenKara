use crate::{
    commands::error::{state_lock_error, CommandError, CommandResult},
    config::RemoteLibraryProvider,
    library::error::LibraryError,
    AppState,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::distr::{Alphanumeric, SampleString};
use reqwest::{Method, StatusCode, Url};
use sha2::Digest;
use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
};
use tiny_http::Response as TinyHttpResponse;

use super::{
    dropbox, google_drive,
    types::{
        current_unix_time_ms, session_id_for_provider, ProviderSessionData, RemoteAuthSession,
        RemoteAuthStart, RemoteAuthState, RemoteAuthStatus,
    },
    webdav,
};

pub(crate) fn begin_remote_auth(
    state: &AppState,
    provider: RemoteLibraryProvider,
    payload: Option<serde_json::Value>,
) -> CommandResult<RemoteAuthStart> {
    let session_id = session_id_for_provider(provider);
    let started = start_provider_auth_session(
        &state.shell.app_resource_dir,
        Arc::clone(&state.remote.remote_auth_sessions),
        &session_id,
        provider,
        payload,
    )?;

    let session = RemoteAuthSession {
        provider: started.provider,
        state: RemoteAuthState::Pending,
        remote_root_locator: None,
        display_name: None,
        account_id: started.account_id,
        error: None,
        session: started.session,
    };

    state
        .remote
        .remote_auth_sessions
        .lock()
        .map_err(|_| state_lock_error("remote auth session lock was poisoned"))?
        .insert(session_id.clone(), session);

    Ok(RemoteAuthStart {
        session_id,
        provider: started.provider,
        authorization_url: started.authorization_url,
        expires_at_ms: Some(current_unix_time_ms() + 15 * 60 * 1000),
    })
}

struct StartedProviderAuth {
    provider: RemoteLibraryProvider,
    session: ProviderSessionData,
    account_id: String,
    authorization_url: Option<String>,
}

fn start_provider_auth_session(
    app_resource_dir: &std::path::Path,
    sessions: Arc<Mutex<HashMap<String, RemoteAuthSession>>>,
    session_id: &str,
    provider: RemoteLibraryProvider,
    payload: Option<serde_json::Value>,
) -> CommandResult<StartedProviderAuth> {
    match provider {
        RemoteLibraryProvider::GoogleDrive => {
            let google = google_drive::parse_google_drive_payload(app_resource_dir, payload)?;
            let google = google_drive::spawn_google_drive_auth_worker(
                sessions,
                session_id.to_owned(),
                google,
            )?;
            let authorization_url =
                Some(google_drive::build_google_drive_authorization_url(&google)?);
            Ok(StartedProviderAuth {
                provider,
                session: ProviderSessionData::GoogleDrive(google),
                account_id: session_id.to_owned(),
                authorization_url,
            })
        }
        RemoteLibraryProvider::Dropbox => {
            let dropbox = dropbox::parse_dropbox_payload(app_resource_dir, payload)?;
            let dropbox =
                dropbox::spawn_dropbox_auth_worker(sessions, session_id.to_owned(), dropbox)?;
            let authorization_url = Some(dropbox::build_dropbox_authorization_url(&dropbox)?);
            Ok(StartedProviderAuth {
                provider,
                session: ProviderSessionData::Dropbox(dropbox),
                account_id: session_id.to_owned(),
                authorization_url,
            })
        }
        RemoteLibraryProvider::WebDav => {
            let webdav_session = webdav::parse_webdav_payload(payload)?;
            let client = webdav::webdav_client()?;
            let response = webdav::webdav_send(
                &client,
                Method::HEAD,
                &webdav_session.server_url,
                &webdav_session.username,
                &webdav_session.password,
                None,
                None,
            )?;
            match response.status() {
                StatusCode::OK
                | StatusCode::NO_CONTENT
                | StatusCode::METHOD_NOT_ALLOWED
                | StatusCode::FOUND
                | StatusCode::MOVED_PERMANENTLY => {}
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    return Err(CommandError::from(LibraryError::Internal(
                        "WebDAV authentication failed. Double-check the server URL, username, and password."
                            .to_owned(),
                    )));
                }
                status => {
                    return Err(CommandError::from(LibraryError::Internal(format!(
                        "WebDAV server check failed with status {status}"
                    ))));
                }
            }
            let account_id = format!(
                "{}@{}",
                webdav_session.username,
                Url::parse(&webdav_session.server_url)
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_owned))
                    .unwrap_or_else(|| "webdav".to_owned())
            );
            Ok(StartedProviderAuth {
                provider,
                session: ProviderSessionData::WebDav(webdav_session),
                account_id,
                authorization_url: None,
            })
        }
    }
}

pub(crate) fn poll_remote_auth(
    state: &AppState,
    session_id: String,
) -> CommandResult<RemoteAuthStatus> {
    let sessions = state
        .remote
        .remote_auth_sessions
        .lock()
        .map_err(|_| state_lock_error("remote auth session lock was poisoned"))?;
    let session = sessions.get(&session_id).ok_or_else(|| {
        CommandError::from(LibraryError::Internal(format!(
            "remote auth session {session_id} was not found"
        )))
    })?;

    Ok(RemoteAuthStatus {
        session_id,
        provider: session.provider,
        state: session.state.clone(),
        remote_root_locator: session.remote_root_locator.clone(),
        display_name: session.display_name.clone(),
        error: session.error.clone(),
    })
}

pub(crate) fn cancel_remote_auth(state: &AppState, session_id: String) -> CommandResult<()> {
    state
        .remote
        .remote_auth_sessions
        .lock()
        .map_err(|_| state_lock_error("remote auth session lock was poisoned"))?
        .remove(&session_id);
    Ok(())
}

pub(crate) fn open_external_url(url: String) -> CommandResult<()> {
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|_error| {
        tracing::trace!("failed to open external URL {url}");
        CommandError::from(LibraryError::Internal(
            "Failed to open browser for authentication.".to_owned(),
        ))
    })
}

pub(crate) fn random_token(length: usize) -> String {
    Alphanumeric.sample_string(&mut rand::rng(), length)
}

pub(crate) fn oauth_pkce_code_challenge(code_verifier: &str) -> String {
    let digest = sha2::Sha256::digest(code_verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

pub(crate) fn form_urlencoded_body(params: &[(&str, String)]) -> CommandResult<String> {
    let mut encoded = Url::parse("https://example.invalid").map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to build token body: {error}"
        )))
    })?;
    {
        let mut pairs = encoded.query_pairs_mut();
        for (key, value) in params {
            pairs.append_pair(key, value);
        }
    }
    Ok(encoded.query().unwrap_or_default().to_owned())
}

pub(crate) fn oauth_callback_response(body: &str) -> TinyHttpResponse<std::io::Cursor<Vec<u8>>> {
    TinyHttpResponse::from_string(body.to_owned())
}

pub(crate) fn env_optional(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub(crate) fn update_remote_auth_session(
    sessions: &Arc<Mutex<HashMap<String, RemoteAuthSession>>>,
    session_id: &str,
    update: impl FnOnce(&mut RemoteAuthSession),
) {
    if let Ok(mut guard) = sessions.lock() {
        if let Some(session) = guard.get_mut(session_id) {
            update(session);
        }
    }
}

pub(crate) fn remote_auth_session_exists(
    sessions: &Arc<Mutex<HashMap<String, RemoteAuthSession>>>,
    session_id: &str,
) -> bool {
    sessions
        .lock()
        .ok()
        .map(|guard| guard.contains_key(session_id))
        .unwrap_or(false)
}
