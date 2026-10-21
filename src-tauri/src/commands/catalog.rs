use crate::cache;
use crate::catalog::{self, StreamingSource, VideoSource};
use crate::commands::error::{database_error, internal_error, state_lock_error, CommandResult};
use crate::source_host::{Cancellation, SourceHost, WorkKind, WorkLease};
use crate::{config, AppState};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

const STREAMING_IMPORT_PROGRESS_EVENT: &str = "streaming-import-progress";
#[derive(Default)]
pub struct SourceAuthentication {
    inner: Mutex<HashMap<String, AuthenticationWork>>,
    completed: Mutex<HashMap<String, (String, catalog::StreamingSessionSnapshot)>>,
}
struct AuthenticationWork {
    challenge: String,
    completed: Option<catalog::StreamingSessionSnapshot>,
    lease: Arc<WorkLease>,
    web: bool,
}

pub(crate) fn app_data_dir(app: &AppHandle) -> CommandResult<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|_| internal_error("could not get application data directory"))
}
pub(crate) fn source(
    app: &AppHandle,
    source_id: &str,
) -> CommandResult<catalog::PluginStreamingSource> {
    app.asset_protocol_scope()
        .allow_directory(app_data_dir(app)?.join("source-artwork"), true)
        .map_err(|_| internal_error("could not allow managed source artwork"))?;
    catalog::PluginStreamingSource::open(
        app.state::<SourceHost>().inner().clone(),
        source_id.to_owned(),
        app_data_dir(app)?,
    )
    .map(|source| {
        source.with_cache(
            app.state::<Arc<catalog::TrackMetadataCache>>()
                .inner()
                .clone(),
        )
    })
    .map_err(Into::into)
}
pub(crate) fn host_error(
    error: crate::source_host::HostError,
) -> crate::commands::error::CommandError {
    catalog::CatalogError::SourceFailure {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
    .into()
}
async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| internal_error("source operation failed"))?
}
pub(crate) fn resolve_streaming_preview(
    app: &AppHandle,
    source_id: &str,
    remote_id: &str,
    cancellation: Cancellation,
) -> CommandResult<catalog::StreamingPreviewOutcome> {
    source(app, source_id)?
        .with_cancellation(cancellation)
        .preview(remote_id)
        .map_err(Into::into)
}
#[tauri::command]
pub async fn get_streaming_session(
    app_handle: AppHandle,
    source_id: String,
) -> CommandResult<catalog::StreamingSessionSnapshot> {
    blocking(move || {
        source(&app_handle, &source_id)?
            .session()
            .map_err(Into::into)
    })
    .await
}
fn authentication_window(source_id: &str) -> String {
    format!("streaming-sign-in-{source_id}")
}
#[tauri::command]
pub async fn open_streaming_sign_in(
    app_handle: AppHandle,
    source_id: String,
) -> CommandResult<Value> {
    cancel_authentication(&app_handle, &source_id).await?;
    app_handle
        .state::<SourceAuthentication>()
        .completed
        .lock()
        .map_err(|_| state_lock_error("authentication result lock"))?
        .remove(&source_id);
    let host = app_handle.state::<SourceHost>().inner().clone();
    let description = host.description(&source_id).map_err(host_error)?;
    let lease = Arc::new(
        host.acquire_work(&source_id, WorkKind::Authentication)
            .map_err(host_error)?,
    );
    let cancellation = lease.cancellation();
    let auth_source = source(&app_handle, &source_id)?.with_cancellation(cancellation.clone());
    let challenge = blocking(move || {
        auth_source
            .request("auth.start", json!({}))
            .map_err(Into::into)
    })
    .await?;
    let challenge_id = challenge
        .get("challenge")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| internal_error("source sign-in challenge missing"))?
        .to_owned();
    let expires_at = challenge
        .get("expires_at")
        .and_then(Value::as_str)
        .and_then(|value| {
            time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
        })
        .map(|value| value.unix_timestamp())
        .ok_or_else(|| internal_error("source sign-in expiry is invalid"))?;
    if expires_at <= time::OffsetDateTime::now_utc().unix_timestamp() {
        return Err(internal_error("source sign-in challenge expired"));
    }
    let kind = challenge
        .pointer("/presentation/kind")
        .and_then(Value::as_str)
        .ok_or_else(|| internal_error("source sign-in presentation missing"))?;
    let flow = description
        .manifest
        .authentication
        .get("flow")
        .and_then(Value::as_str)
        .unwrap_or("none");
    if !matches!(
        (flow, kind),
        ("web", "web") | ("plugin", "qr") | ("plugin", "text")
    ) {
        return Err(internal_error(
            "source sign-in presentation is not declared",
        ));
    }
    app_handle
        .state::<SourceAuthentication>()
        .inner
        .lock()
        .map_err(|_| state_lock_error("authentication state lock"))?
        .insert(
            source_id.clone(),
            AuthenticationWork {
                challenge: challenge_id.clone(),
                completed: None,
                lease: lease.clone(),
                web: flow == "web",
            },
        );
    if flow == "web" {
        let auth = description.manifest.authentication;
        let url = auth
            .get("sign_in_url")
            .and_then(Value::as_str)
            .ok_or_else(|| internal_error("official sign-in URL missing"))?
            .parse::<tauri::Url>()
            .map_err(|_| internal_error("official sign-in URL invalid"))?;
        let origins = auth
            .get("navigation_origins")
            .and_then(Value::as_array)
            .ok_or_else(|| internal_error("official sign-in origins missing"))?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let cookies = auth
            .get("cookie_names")
            .and_then(Value::as_array)
            .ok_or_else(|| internal_error("official sign-in cookies missing"))?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let allowed = origins.clone();
        let opened = tauri::WebviewWindowBuilder::new(
            &app_handle,
            authentication_window(&source_id),
            tauri::WebviewUrl::External(url),
        )
        .title(description.manifest.name)
        .inner_size(1100.0, 780.0)
        .incognito(true)
        .on_navigation(move |url| {
            url.scheme() == "https"
                && allowed
                    .iter()
                    .any(|origin| url.origin().ascii_serialization() == *origin)
        })
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .on_download(|_, _| false)
        .build();
        let window = match opened {
            Ok(window) => window,
            Err(_) => {
                cancel_authentication(&app_handle, &source_id).await?;
                return Err(internal_error("could not open official sign-in window"));
            }
        };
        tauri::async_runtime::spawn(watch_web_sign_in(
            app_handle.clone(),
            source_id.clone(),
            challenge_id,
            window,
            origins,
            cookies,
            lease,
            expires_at,
        ));
    } else {
        let app = app_handle.clone();
        let id = source_id.clone();
        let expected_challenge = challenge_id.clone();
        tauri::async_runtime::spawn(async move {
            let remaining =
                expires_at.saturating_sub(time::OffsetDateTime::now_utc().unix_timestamp()) as u64;
            tokio::select! {_=cancellation.cancelled()=>{},_=tokio::time::sleep(std::time::Duration::from_secs(remaining))=>{}}
            let _ = cancel_matching_authentication(&app, &id, Some(&expected_challenge)).await;
        });
    }
    let mut response = challenge;
    response["source_id"] = json!(source_id);
    if response.get("poll_after_ms").is_none() {
        response["poll_after_ms"] = Value::Null;
    }
    Ok(response)
}
fn read_cookies(
    window: &WebviewWindow,
    origins: &[String],
    names: &[String],
) -> CommandResult<Option<catalog::StreamingCredentials>> {
    let mut values = std::collections::BTreeMap::new();
    for origin in origins {
        let url = origin
            .parse()
            .map_err(|_| internal_error("invalid official origin"))?;
        for cookie in window
            .cookies_for_url(url)
            .map_err(|_| internal_error("could not read official sign-in cookies"))?
        {
            if names.iter().any(|name| name == cookie.name()) && !cookie.value().is_empty() {
                values.insert(cookie.name().to_owned(), cookie.value().to_owned());
            }
        }
    }
    Ok((!names.is_empty() && names.iter().all(|name| values.contains_key(name))).then_some(values))
}
async fn watch_web_sign_in(
    app: AppHandle,
    id: String,
    challenge: String,
    window: WebviewWindow,
    origins: Vec<String>,
    names: Vec<String>,
    lease: Arc<WorkLease>,
    expires_at: i64,
) {
    let cancellation = lease.cancellation();
    loop {
        tokio::select! { _=cancellation.cancelled()=>break, _=tokio::time::sleep(std::time::Duration::from_millis(500))=>{} }
        if time::OffsetDateTime::now_utc().unix_timestamp() >= expires_at {
            let _ = app.emit_to(
                "main",
                "streaming-sign-in-error",
                json!({"source_id":id,"challenge":challenge,"error":internal_error("source sign-in challenge expired")}),
            );
            break;
        }
        if app
            .get_webview_window(&authentication_window(&id))
            .is_none()
        {
            break;
        }
        let credentials = match read_cookies(&window, &origins, &names) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(error) => {
                let _ = app.emit_to(
                    "main",
                    "streaming-sign-in-error",
                    json!({"source_id":id,"challenge":challenge,"error":error}),
                );
                break;
            }
        };
        let handle = app.clone();
        let source_id = id.clone();
        let challenge_id = challenge.clone();
        let operation_cancellation = cancellation.clone();
        let result = blocking(move || {
            let source = source(&handle, &source_id)?.with_cancellation(operation_cancellation);
            let value = source.request(
                "auth.complete",
                json!({"challenge":challenge_id,"credentials":credentials}),
            )?;
            let auth = handle.state::<SourceAuthentication>();
            let guard = auth
                .inner
                .lock()
                .map_err(|_| state_lock_error("authentication state lock"))?;
            if !guard.get(&source_id).is_some_and(|w| {
                w.challenge == challenge_id && !w.lease.cancellation().is_cancelled()
            }) {
                return Err(internal_error("sign-in was cancelled"));
            }
            source.save_verified_session(&value).map_err(Into::into)
        })
        .await;
        match result {
            Ok(session) => {
                let current =
                    if let Ok(mut guard) = app.state::<SourceAuthentication>().inner.lock() {
                        if let Some(work) = guard.get_mut(&id) {
                            if work.challenge == challenge {
                                work.completed = Some(session.clone());
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                if current {
                    let _ = app.emit_to("main", "streaming-sign-in-complete", &session);
                }
            }
            Err(error) => {
                let _ = app.emit_to(
                    "main",
                    "streaming-sign-in-error",
                    json!({"source_id":id,"challenge":challenge,"error":error}),
                );
            }
        }
        break;
    }
    let _ = window.close();
    let completed_work = {
        let auth = app.state::<SourceAuthentication>();
        let mut guard = match auth.inner.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if !guard
            .get(&id)
            .is_some_and(|work| work.challenge == challenge)
        {
            None
        } else {
            let work = guard.remove(&id);
            if let Some(session) = work.as_ref().and_then(|work| work.completed.clone()) {
                if let Ok(mut terminal) = auth.completed.lock() {
                    terminal.insert(id.clone(), (challenge.clone(), session));
                }
            }
            work
        }
    };
    if let Some(work) = completed_work {
        work.lease.cancellation().cancel();
        if work.completed.is_none() {
            let _ = app
                .state::<SourceHost>()
                .request(&id, "auth.cancel", json!({"challenge":challenge}))
                .await;
        }
        drop(work);
    }
}
#[tauri::command]
pub async fn poll_streaming_sign_in(
    app_handle: AppHandle,
    source_id: String,
    challenge: String,
) -> CommandResult<Value> {
    let lease = {
        let state = app_handle.state::<SourceAuthentication>();
        let guard = state
            .inner
            .lock()
            .map_err(|_| state_lock_error("authentication state lock"))?;
        if let Some((_, session)) = state
            .completed
            .lock()
            .map_err(|_| state_lock_error("authentication result lock"))?
            .get(&source_id)
            .filter(|(token, _)| token == &challenge)
        {
            return Ok(json!({"state":"complete","session":session}));
        }
        let work = guard
            .get(&source_id)
            .filter(|w| w.challenge == challenge)
            .ok_or_else(|| internal_error("sign-in challenge expired"))?;
        if let Some(session) = &work.completed {
            return Ok(json!({"state":"complete","session":session}));
        }
        if work.web {
            return Ok(json!({"state":"pending","poll_after_ms":500}));
        }
        work.lease.clone()
    };
    let app = app_handle.clone();
    let id = source_id.clone();
    let token = challenge.clone();
    let result = blocking(move || {
        let source = source(&app, &id)?.with_cancellation(lease.cancellation());
        let value = source.request("auth.poll", json!({"challenge":token}))?;
        let auth = app.state::<SourceAuthentication>();
        let guard = auth
            .inner
            .lock()
            .map_err(|_| state_lock_error("authentication state lock"))?;
        if !guard
            .get(&id)
            .is_some_and(|w| w.challenge == token && !w.lease.cancellation().is_cancelled())
        {
            return Err(internal_error("sign-in was cancelled"));
        }
        if value.get("state").and_then(Value::as_str) == Some("complete") {
            let session = source.save_verified_session(&value)?;
            Ok(json!({"state":"complete","session":session}))
        } else {
            Ok(value)
        }
    })
    .await?;
    if result.get("state").and_then(Value::as_str) != Some("pending") {
        let auth = app_handle.state::<SourceAuthentication>();
        let mut guard = auth
            .inner
            .lock()
            .map_err(|_| state_lock_error("authentication state lock"))?;
        if guard
            .get(&source_id)
            .is_some_and(|work| work.challenge == challenge)
        {
            if let Some(work) = guard.remove(&source_id) {
                work.lease.cancellation().cancel();
            }
        }
    }
    Ok(result)
}
#[tauri::command]
pub async fn cancel_streaming_sign_in(
    app_handle: AppHandle,
    source_id: String,
    challenge: String,
) -> CommandResult<()> {
    cancel_matching_authentication(&app_handle, &source_id, Some(&challenge)).await?;
    Ok(())
}
pub(crate) async fn cancel_authentication(app: &AppHandle, id: &str) -> CommandResult<()> {
    cancel_matching_authentication(app, id, None).await
}
async fn cancel_matching_authentication(
    app: &AppHandle,
    id: &str,
    challenge: Option<&str>,
) -> CommandResult<()> {
    let work = {
        let auth = app.state::<SourceAuthentication>();
        let mut guard = auth
            .inner
            .lock()
            .map_err(|_| state_lock_error("authentication state lock"))?;
        if challenge
            .is_none_or(|expected| guard.get(id).is_some_and(|work| work.challenge == expected))
        {
            guard.remove(id)
        } else {
            None
        }
    };
    if let Some(work) = work {
        work.lease.cancellation().cancel();
        if let Some(window) = app.get_webview_window(&authentication_window(id)) {
            window
                .close()
                .map_err(|_| internal_error("could not close source sign-in window"))?;
        }
        if work.completed.is_none() {
            let _ = app
                .state::<SourceHost>()
                .request(id, "auth.cancel", json!({"challenge":work.challenge}))
                .await;
        }
        drop(work);
    }
    Ok(())
}
#[tauri::command]
pub async fn sign_out_streaming_source(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    stop_work: Option<bool>,
) -> CommandResult<catalog::StreamingSessionSnapshot> {
    app_handle
        .state::<SourceAuthentication>()
        .completed
        .lock()
        .map_err(|_| state_lock_error("authentication result lock"))?
        .remove(&source_id);
    let host = app_handle.state::<SourceHost>().inner().clone();
    let snapshot = match host.snapshot(&source_id) {
        Ok(snapshot) => snapshot,
        Err(_) => {
            cancel_authentication(&app_handle, &source_id).await?;
            catalog::clear_session(&app_data_dir(&app_handle)?, &source_id)
                .map_err(|_| internal_error("could not remove source session"))?;
            return Ok(catalog::StreamingSessionSnapshot {
                source_id,
                signed_in: false,
                display_name: None,
                expired: false,
            });
        }
    };
    if !snapshot.active_work.is_empty() && !stop_work.unwrap_or(false) {
        return Err(catalog::CatalogError::SourceFailure{code:"active_work".into(),message:"Active sign-in, preview, or import must stop before sign-out. Choose Stop Work to continue.".into(),retryable:false}.into());
    }
    let enabled = snapshot.enabled;
    if enabled {
        crate::commands::source_host::stop_source_work(&state, &app_handle, &source_id).await?;
        host.set_enabled(&source_id, false, true)
            .await
            .map_err(host_error)?;
    }
    cancel_authentication(&app_handle, &source_id).await?;
    catalog::clear_session(&app_data_dir(&app_handle)?, &source_id)
        .map_err(|_| internal_error("could not remove source session"))?;
    if enabled {
        host.set_enabled(&source_id, true, false)
            .await
            .map_err(host_error)?;
    }
    Ok(catalog::StreamingSessionSnapshot {
        source_id,
        signed_in: false,
        display_name: None,
        expired: false,
    })
}
fn annotate_tracks(
    state: &AppState,
    source_id: &str,
    tracks: &mut [catalog::StreamingTrack],
) -> CommandResult<()> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;
    catalog::annotate_library_songs(&connection, source_id, tracks).map_err(Into::into)
}
#[tauri::command]
pub async fn list_streaming_liked_tracks(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    cursor: Option<String>,
) -> CommandResult<catalog::StreamingTrackListing> {
    let state = state.inner().clone();
    blocking(move || {
        let mut page = source(&app_handle, &source_id)?
            .track_page("liked_tracks", json!({"cursor":cursor}))?;
        annotate_tracks(&state, &source_id, &mut page.tracks)?;
        Ok(page)
    })
    .await
}
#[tauri::command]
pub async fn get_streaming_tracks(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    remote_track_ids: Vec<String>,
) -> CommandResult<Vec<catalog::StreamingTrack>> {
    let state = state.inner().clone();
    blocking(move || {
        let mut tracks = source(&app_handle, &source_id)?.tracks(&remote_track_ids)?;
        annotate_tracks(&state, &source_id, &mut tracks)?;
        Ok(tracks)
    })
    .await
}
#[tauri::command]
pub async fn list_streaming_playlists(
    app_handle: AppHandle,
    source_id: String,
    cursor: Option<String>,
) -> CommandResult<catalog::StreamingPlaylistListing> {
    blocking(move || {
        source(&app_handle, &source_id)?
            .playlist_page(cursor.as_deref())
            .map_err(Into::into)
    })
    .await
}
#[tauri::command]
pub async fn get_streaming_playlist(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    remote_playlist_id: String,
    cursor: Option<String>,
) -> CommandResult<catalog::StreamingPlaylistDetail> {
    let state = state.inner().clone();
    blocking(move || {
        let mut page = source(&app_handle, &source_id)?
            .playlist_tracks_page(&remote_playlist_id, cursor.as_deref())?;
        annotate_tracks(&state, &source_id, &mut page.tracks)?;
        Ok(page)
    })
    .await
}
#[tauri::command]
pub async fn search_streaming_source(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    query: String,
    cursor: Option<String>,
) -> CommandResult<catalog::StreamingTrackListing> {
    let state = state.inner().clone();
    blocking(move || {
        let mut page = source(&app_handle, &source_id)?
            .track_page("search", json!({"query":query,"cursor":cursor}))?;
        annotate_tracks(&state, &source_id, &mut page.tracks)?;
        Ok(page)
    })
    .await
}
#[tauri::command]
pub async fn start_streaming_import(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    remote_track_ids: Vec<String>,
    remote_playlist_id: Option<String>,
    job_id: String,
) -> CommandResult<catalog::StreamingImportProgress> {
    let host = app_handle.state::<SourceHost>().inner().clone();
    if job_id.is_empty() || job_id.len() > 128 {
        return Err(internal_error("invalid streaming import job id"));
    }
    let lease = host
        .acquire_work(&source_id, WorkKind::Import)
        .map_err(host_error)?;
    let cancellation = lease.cancellation();
    let job = catalog::StreamingImportJob::new(job_id, source_id.clone(), Vec::new(), {
        let app = app_handle.clone();
        move |event| {
            let _ = app.emit_to("main", STREAMING_IMPORT_PROGRESS_EVENT, event);
        }
    });
    job.bind_cancellation(cancellation.clone());
    {
        let mut guard = state
            .catalog
            .import_job
            .lock()
            .map_err(|_| state_lock_error("import job lock"))?;
        if guard
            .as_ref()
            .is_some_and(|job| job.snapshot().phase != catalog::StreamingImportJobPhase::Completed)
        {
            return Err(internal_error("a streaming import is already running"));
        }
        *guard = Some(job.clone());
    }
    let state = state.inner().clone();
    blocking(move || {
        let result = (|| {
            let source = source(&app_handle, &source_id)?.with_cancellation(cancellation);
            let playlist = match remote_playlist_id.as_deref() {
                Some(id) => Some(source.playlist(id)?),
                None => None,
            };
            let tracks = match &playlist {
                Some(p) if remote_track_ids.is_empty() => p.tracks.clone(),
                Some(p) => p
                    .tracks
                    .iter()
                    .filter(|t| remote_track_ids.contains(&t.remote_track_id))
                    .cloned()
                    .collect(),
                None => source.import_tracks(&remote_track_ids)?,
            };
            let items = catalog::items_from_tracks(&tracks);
            job.set_tracks(
                tracks
                    .iter()
                    .map(|t| (t.remote_track_id.clone(), t.title.clone(), t.artist.clone()))
                    .collect(),
            );
            let request = catalog::StreamingImportRequest {
                source_id,
                items,
                playlist: playlist.map(|p| catalog::StreamingPlaylistOrigin {
                    remote_playlist_id: p.remote_id,
                    name: p.name,
                }),
            };
            let mut session = catalog::StreamingImportSession::new(request)
                .with_job(job.clone())
                .with_work_lease(lease);
            run_import_on_library(&state, &app_handle, &source, &mut session, None)
        })();
        if result.is_err() {
            job.set_phase(catalog::StreamingImportJobPhase::Completed);
        }
        result
    })
    .await
}
#[tauri::command]
pub async fn continue_streaming_import(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    action: catalog::LibraryDecisionAction,
) -> CommandResult<catalog::StreamingImportProgress> {
    let state = state.inner().clone();
    blocking(move || {
        let mut session = state
            .catalog
            .import_session
            .lock()
            .map_err(|_| state_lock_error("import session lock"))?
            .take()
            .ok_or_else(|| internal_error("no streaming import is waiting"))?;
        let source =
            source(&app_handle, session.source_id())?.with_cancellation(session.cancellation());
        let result =
            run_import_on_library(&state, &app_handle, &source, &mut session, Some(action));
        if result.is_err() {
            if let Some(job) = state
                .catalog
                .import_job
                .lock()
                .map_err(|_| state_lock_error("import job lock"))?
                .as_ref()
            {
                job.set_phase(catalog::StreamingImportJobPhase::Completed);
            }
        }
        result
    })
    .await
}
#[tauri::command]
pub async fn cancel_streaming_import(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> CommandResult<()> {
    let source_id = state
        .catalog
        .import_job
        .lock()
        .map_err(|_| state_lock_error("import job lock"))?
        .as_ref()
        .map(|job| job.snapshot().source_id);
    if let Some(id) = source_id {
        crate::commands::source_host::stop_import_work(&state, &app_handle, &id).await?;
    }
    Ok(())
}
#[tauri::command]
pub fn get_streaming_import_status(
    state: State<'_, AppState>,
) -> CommandResult<Option<catalog::StreamingImportJobSnapshot>> {
    Ok(state
        .catalog
        .import_job
        .lock()
        .map_err(|_| state_lock_error("import job lock"))?
        .as_ref()
        .map(|j| j.snapshot()))
}
fn run_import_on_library(
    state: &AppState,
    app: &AppHandle,
    source: &catalog::PluginStreamingSource,
    session: &mut catalog::StreamingImportSession,
    action: Option<catalog::LibraryDecisionAction>,
) -> CommandResult<catalog::StreamingImportProgress> {
    let library = state.library_root()?;
    let mut connection = cache::open_database(&library.database_path()).map_err(database_error)?;
    let progress =
        catalog::advance_import_session(&mut connection, &library, source, session, action)?;
    let job = session.job();
    if progress.status == catalog::StreamingImportStatus::AwaitingDecision {
        let id = session.source_id().to_owned();
        *state
            .catalog
            .import_session
            .lock()
            .map_err(|_| state_lock_error("import session lock"))? = Some(std::mem::replace(
            session,
            catalog::StreamingImportSession::new(catalog::StreamingImportRequest {
                source_id: id,
                items: Vec::new(),
                playlist: None,
            }),
        ));
        if let Some(job) = job {
            let cancellation = state
                .catalog
                .import_session
                .lock()
                .map_err(|_| state_lock_error("import session lock"))?
                .as_ref()
                .filter(|session| session.job().is_some_and(|owner| Arc::ptr_eq(&owner, &job)))
                .map(|session| session.cancellation());
            if let Some(cancellation) = cancellation {
                let state = state.clone();
                let app = app.clone();
                let source = source.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = cancellation.cancelled() => break,
                            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                                let waiting = state.catalog.import_session.lock().ok()
                                    .is_some_and(|parked| parked.as_ref().is_some_and(|session| {
                                        session.job().is_some_and(|owner| Arc::ptr_eq(&owner, &job))
                                    }));
                                if !waiting { return; }
                            }
                        }
                    }
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        settle_cancelled_import(&state, &app, &source, &job)
                    })
                    .await;
                });
            }
        }
    } else if let Some(job) = job {
        let _ = app.emit_to(
            "main",
            "streaming-import-complete",
            json!({
                "source_id": progress.source_id,
                "job_id": job.job_id(),
                "progress": progress,
            }),
        );
    }
    Ok(progress)
}

pub(crate) fn settle_cancelled_import(
    state: &AppState,
    app: &AppHandle,
    source: &catalog::PluginStreamingSource,
    job: &Arc<catalog::StreamingImportJob>,
) -> CommandResult<()> {
    let mut session = {
        let mut parked = state
            .catalog
            .import_session
            .lock()
            .map_err(|_| state_lock_error("import session lock"))?;
        let matches = parked.as_ref().is_some_and(|session| {
            session.source_id() == job.snapshot().source_id
                && session.job().is_some_and(|owner| Arc::ptr_eq(&owner, job))
        });
        if !matches {
            return Ok(());
        }
        parked.take().expect("matching parked import")
    };
    job.cancel();
    run_import_on_library(
        state,
        app,
        source,
        &mut session,
        Some(catalog::LibraryDecisionAction::Cancel),
    )?;
    Ok(())
}
#[tauri::command]
pub async fn resolve_video_source_url(
    app_handle: AppHandle,
    source_id: String,
    url: String,
) -> CommandResult<Vec<catalog::VideoQueueItem>> {
    let config = config::load_config(&app_data_dir(&app_handle)?)
        .map_err(internal_error)?
        .unwrap_or_default();
    catalog::require_enabled(&config, &source_id)?;
    if source_id != catalog::YOUTUBE_SOURCE_ID {
        return Err(internal_error("unknown video source"));
    }
    if url.contains("/player") {
        return Err(internal_error("YouTube /player stream URLs are not used"));
    }
    blocking(move || {
        let fetcher = catalog::LiveYoutubeFetcher::new()?;
        catalog::YoutubeVideoSource::new(fetcher)
            .resolve(&url)
            .map_err(Into::into)
    })
    .await
}
#[tauri::command]
pub fn get_reveal_targets(
    state: State<'_, AppState>,
    song_id: String,
) -> CommandResult<catalog::RevealTargets> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;
    catalog::get_reveal_targets(&connection, &library, &song_id).map_err(Into::into)
}
#[tauri::command]
pub fn reveal_in_folder(path: String) -> CommandResult<()> {
    catalog::reveal_path(&path).map_err(Into::into)
}
