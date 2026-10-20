use crate::cache;
use crate::catalog::{
    self, items_from_tracks, LiveNeteaseApi, LiveYoutubeFetcher, NeteaseStreamingSource,
    StreamingImportRequest, StreamingImportSession, StreamingImportStatus, StreamingPlaylistOrigin,
    StreamingSource, VideoSource, YoutubeVideoSource, NETEASE_SOURCE_ID, YOUTUBE_SOURCE_ID,
};
use crate::commands::error::{database_error, internal_error, state_lock_error, CommandResult};
use crate::config;
use crate::AppState;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, WindowEvent};

const STREAMING_IMPORT_PROGRESS_EVENT: &str = "streaming-import-progress";

fn app_data_dir(app_handle: &AppHandle) -> CommandResult<std::path::PathBuf> {
    app_handle
        .path()
        .app_data_dir()
        .map_err(|error| internal_error(format!("failed to get app data dir: {error}")))
}

fn load_config(app_handle: &AppHandle) -> CommandResult<crate::config::AppConfig> {
    let app_data_dir = app_data_dir(app_handle)?;
    config::load_config(&app_data_dir)
        .map_err(|error| internal_error(format!("failed to load config: {error}")))
        .map(|config| config.unwrap_or_default())
}

fn require_source(
    app_handle: &AppHandle,
    source_id: &str,
) -> CommandResult<catalog::OnlineSourceSnapshot> {
    let config = load_config(app_handle)?;
    catalog::require_enabled(&config, source_id).map_err(Into::into)
}

fn netease_source(app_handle: &AppHandle) -> CommandResult<NeteaseStreamingSource<LiveNeteaseApi>> {
    require_source(app_handle, NETEASE_SOURCE_ID)?;
    let http = LiveNeteaseApi::new(app_handle)
        .map_err(Into::<crate::commands::error::CommandError>::into)?;
    let cache = app_handle
        .state::<std::sync::Arc<catalog::NeteaseSongCache>>()
        .inner()
        .clone();
    NeteaseStreamingSource::open(http, app_data_dir(app_handle)?)
        .map(|source| source.with_cache(cache))
        .map_err(Into::into)
}

/// Asks NetEase whether `remote_track_id` can be streamed in full, and from
/// where. Blocking: call it off the async runtime.
pub(crate) fn resolve_netease_preview(
    app_handle: &AppHandle,
    remote_track_id: &str,
) -> CommandResult<catalog::StreamingPreviewOutcome> {
    netease_source(app_handle)?
        .preview(remote_track_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_streaming_session(
    app_handle: AppHandle,
    source_id: String,
) -> CommandResult<catalog::StreamingSessionSnapshot> {
    require_source(&app_handle, &source_id)?;
    if source_id != NETEASE_SOURCE_ID {
        return Err(internal_error(format!(
            "unknown streaming source: {source_id}"
        )));
    }
    tauri::async_runtime::spawn_blocking(
        move || -> CommandResult<catalog::StreamingSessionSnapshot> {
            netease_source(&app_handle)?.session().map_err(Into::into)
        },
    )
    .await
    .map_err(|_| internal_error("NetEase session check failed"))?
}

const NETEASE_SIGN_IN_WINDOW: &str = "netease-sign-in";
const NETEASE_SIGN_IN_URL: &str = "https://music.163.com/#/login";
const NETEASE_SIGN_IN_COMPLETE_EVENT: &str = "netease-sign-in-complete";
const NETEASE_SIGN_IN_ERROR_EVENT: &str = "netease-sign-in-error";

fn session_cookies(
    cookies: impl IntoIterator<Item = (String, String)>,
) -> Option<Vec<(String, String)>> {
    let mut music_u = None;
    let mut csrf = None;
    for (name, value) in cookies {
        match name.as_str() {
            "MUSIC_U" if !value.is_empty() => music_u = Some(value),
            "__csrf" if !value.is_empty() => csrf = Some(value),
            _ => {}
        }
    }
    Some(vec![
        ("MUSIC_U".to_owned(), music_u?),
        ("__csrf".to_owned(), csrf?),
    ])
}

#[cfg(test)]
mod sign_in_tests {
    use super::session_cookies;

    #[test]
    fn waits_for_both_session_cookies_and_discards_the_rest() {
        assert!(session_cookies([("MUSIC_U".to_owned(), "session".to_owned())]).is_none());
        assert_eq!(
            session_cookies([
                ("tracking".to_owned(), "private".to_owned()),
                ("MUSIC_U".to_owned(), "session".to_owned()),
                ("__csrf".to_owned(), "csrf".to_owned()),
            ]),
            Some(vec![
                ("MUSIC_U".to_owned(), "session".to_owned()),
                ("__csrf".to_owned(), "csrf".to_owned()),
            ])
        );
    }
}

fn read_session_cookies(window: &WebviewWindow) -> CommandResult<Option<Vec<(String, String)>>> {
    let cookies = window
        .cookies_for_url("https://music.163.com/".parse().expect("NetEase URL"))
        .map_err(|_| internal_error("Could not read the NetEase session."))?
        .into_iter()
        .map(|cookie| (cookie.name().to_owned(), cookie.value().to_owned()));
    Ok(session_cookies(cookies))
}

async fn watch_netease_sign_in(app_handle: AppHandle, window: WebviewWindow) {
    let closed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let on_destroyed = closed.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            on_destroyed.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if closed.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        let cookies = match read_session_cookies(&window) {
            Ok(Some(cookies)) => cookies,
            Ok(None) => continue,
            Err(error) => {
                let _ = window.close();
                let _ = app_handle.emit_to("main", NETEASE_SIGN_IN_ERROR_EVENT, error);
                break;
            }
        };
        let handle = app_handle.clone();
        let result = tauri::async_runtime::spawn_blocking(move || -> CommandResult<_> {
            netease_source(&handle)?
                .complete_web_sign_in(cookies)
                .map_err(Into::into)
        })
        .await
        .map_err(|_| internal_error("NetEase sign-in check failed"))
        .and_then(std::convert::identity);
        let _ = window.close();
        match result {
            Ok(session) => {
                let _ = app_handle.emit_to("main", NETEASE_SIGN_IN_COMPLETE_EVENT, session);
            }
            Err(error) => {
                let _ = app_handle.emit_to("main", NETEASE_SIGN_IN_ERROR_EVENT, error);
            }
        }
        break;
    }
}

#[tauri::command]
pub async fn open_streaming_sign_in(app_handle: AppHandle, source_id: String) -> CommandResult<()> {
    require_source(&app_handle, &source_id)?;
    if source_id != NETEASE_SOURCE_ID {
        return Err(internal_error("unknown streaming source"));
    }
    if let Some(window) = app_handle.get_webview_window(NETEASE_SIGN_IN_WINDOW) {
        window
            .set_focus()
            .map_err(|error| internal_error(error.to_string()))?;
        return Ok(());
    }
    let window = tauri::WebviewWindowBuilder::new(
        &app_handle,
        NETEASE_SIGN_IN_WINDOW,
        tauri::WebviewUrl::External(NETEASE_SIGN_IN_URL.parse().expect("NetEase URL")),
    )
    .title("NetEase Music")
    .inner_size(1100.0, 780.0)
    .incognito(true)
    .on_navigation(|url| {
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|host| host == "163.com" || host.ends_with(".163.com"))
    })
    .build()
    .map_err(|error| internal_error(error.to_string()))?;
    tauri::async_runtime::spawn(watch_netease_sign_in(app_handle, window));
    Ok(())
}

/// Runs blocking provider work (sidecar and network calls) off the async
/// command thread so the UI stays responsive.
async fn run_blocking<T: Send + 'static>(
    task: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| internal_error("online source task failed"))?
}

#[tauri::command]
pub async fn sign_out_streaming_source(
    app_handle: AppHandle,
    source_id: String,
) -> CommandResult<catalog::StreamingSessionSnapshot> {
    require_source(&app_handle, &source_id)?;
    run_blocking(move || {
        let session = netease_source(&app_handle)?.sign_out()?;
        if let Some(window) = app_handle.get_webview_window(NETEASE_SIGN_IN_WINDOW) {
            window
                .close()
                .map_err(|error| internal_error(error.to_string()))?;
        }
        Ok(session)
    })
    .await
}

fn annotate_tracks(
    state: &AppState,
    source_id: &str,
    tracks: &mut [catalog::StreamingTrack],
) -> CommandResult<()> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;
    catalog::annotate_library_songs(&connection, source_id, tracks)
        .map_err(Into::<crate::commands::error::CommandError>::into)
}

#[tauri::command]
pub async fn list_streaming_liked_tracks(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
) -> CommandResult<catalog::StreamingTrackListing> {
    require_source(&app_handle, &source_id)?;
    let state = state.inner().clone();
    run_blocking(move || {
        let mut listing = netease_source(&app_handle)?.liked_head()?;
        annotate_tracks(&state, &source_id, &mut listing.tracks)?;
        Ok(listing)
    })
    .await
}

/// One follow-up page of a partial listing: tracks for `remote_track_ids`, in
/// order. Served from the session song cache when already fetched.
#[tauri::command]
pub async fn get_streaming_tracks(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    remote_track_ids: Vec<String>,
) -> CommandResult<Vec<catalog::StreamingTrack>> {
    require_source(&app_handle, &source_id)?;
    let state = state.inner().clone();
    run_blocking(move || {
        let mut tracks = netease_source(&app_handle)?.tracks(&remote_track_ids)?;
        annotate_tracks(&state, &source_id, &mut tracks)?;
        Ok(tracks)
    })
    .await
}

#[tauri::command]
pub async fn list_streaming_playlists(
    app_handle: AppHandle,
    source_id: String,
) -> CommandResult<Vec<catalog::StreamingPlaylistSummary>> {
    require_source(&app_handle, &source_id)?;
    run_blocking(move || netease_source(&app_handle)?.playlists().map_err(Into::into)).await
}

#[tauri::command]
pub async fn get_streaming_playlist(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    remote_playlist_id: String,
) -> CommandResult<catalog::StreamingPlaylistDetail> {
    require_source(&app_handle, &source_id)?;
    let state = state.inner().clone();
    run_blocking(move || {
        let mut detail = netease_source(&app_handle)?.playlist_head(&remote_playlist_id)?;
        annotate_tracks(&state, &source_id, &mut detail.tracks)?;
        Ok(detail)
    })
    .await
}

#[tauri::command]
pub async fn search_streaming_source(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    query: String,
) -> CommandResult<Vec<catalog::StreamingTrack>> {
    require_source(&app_handle, &source_id)?;
    let state = state.inner().clone();
    run_blocking(move || {
        let mut tracks = netease_source(&app_handle)?.search(&query)?;
        annotate_tracks(&state, &source_id, &mut tracks)?;
        Ok(tracks)
    })
    .await
}

fn import_job_is_active(state: &AppState) -> CommandResult<bool> {
    let guard = state
        .catalog
        .import_job
        .lock()
        .map_err(|_| state_lock_error("catalog import job lock was poisoned"))?;
    Ok(guard
        .as_ref()
        .is_some_and(|job| job.snapshot().phase != catalog::StreamingImportJobPhase::Completed))
}

fn emit_import_event(app_handle: &AppHandle, event: &catalog::StreamingImportEvent) {
    let _ = app_handle.emit_to("main", STREAMING_IMPORT_PROGRESS_EVENT, event);
}

#[tauri::command]
pub async fn start_streaming_import(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    remote_track_ids: Vec<String>,
    remote_playlist_id: Option<String>,
) -> CommandResult<catalog::StreamingImportProgress> {
    require_source(&app_handle, &source_id)?;
    if import_job_is_active(&state)? {
        return Err(internal_error("a streaming import is already running"));
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = netease_source(&app_handle)?;
        let playlist = match remote_playlist_id.as_deref() {
            Some(remote_id) => {
                let detail = source
                    .playlist(remote_id)
                    .map_err(Into::<crate::commands::error::CommandError>::into)?;
                Some((detail.name.clone(), remote_id.to_owned(), detail.tracks))
            }
            None => None,
        };
        let items = if let Some((_, _, tracks)) = playlist.as_ref() {
            if remote_track_ids.is_empty() {
                items_from_tracks(tracks)
            } else {
                items_from_tracks(
                    &tracks
                        .iter()
                        .filter(|track| {
                            remote_track_ids
                                .iter()
                                .any(|id| id == &track.remote_track_id)
                        })
                        .cloned()
                        .collect::<Vec<_>>(),
                )
            }
        } else {
            // Name the songs from the session cache (one lookup for any not yet
            // seen) so skipped and failed rows are never blank.
            let known = source.tracks(&remote_track_ids).unwrap_or_default();
            remote_track_ids
                .into_iter()
                .map(|remote_track_id| {
                    let track = known
                        .iter()
                        .find(|track| track.remote_track_id == remote_track_id);
                    catalog::StreamingImportItem {
                        title: track.map(|track| track.title.clone()).unwrap_or_default(),
                        artist: track.map(|track| track.artist.clone()).unwrap_or_default(),
                        remote_track_id,
                    }
                })
                .collect()
        };
        let job = catalog::StreamingImportJob::new(
            new_import_job_id(),
            items
                .iter()
                .map(|item| {
                    (
                        item.remote_track_id.clone(),
                        item.title.clone(),
                        item.artist.clone(),
                    )
                })
                .collect(),
            {
                let app_handle = app_handle.clone();
                move |event| emit_import_event(&app_handle, event)
            },
        );
        *state
            .catalog
            .import_job
            .lock()
            .map_err(|_| state_lock_error("catalog import job lock was poisoned"))? =
            Some(job.clone());
        let request = StreamingImportRequest {
            source_id,
            items,
            playlist: playlist.map(|(name, remote_id, _)| StreamingPlaylistOrigin {
                remote_playlist_id: remote_id,
                name,
            }),
        };
        let mut session = StreamingImportSession::new(request).with_job(job.clone());
        let result = run_import_on_library(&state, &source, &mut session, None);
        if result.is_err() {
            job.set_phase(catalog::StreamingImportJobPhase::Completed);
        }
        result
    })
    .await
    .map_err(|_| internal_error("streaming import task failed"))?
}

#[tauri::command]
pub async fn continue_streaming_import(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    action: catalog::LibraryDecisionAction,
) -> CommandResult<catalog::StreamingImportProgress> {
    require_source(&app_handle, NETEASE_SOURCE_ID)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = netease_source(&app_handle)?;
        let mut session = state
            .catalog
            .import_session
            .lock()
            .map_err(|_| state_lock_error("catalog import session lock was poisoned"))?
            .take()
            .ok_or_else(|| internal_error("no streaming import is waiting"))?;
        let result = run_import_on_library(&state, &source, &mut session, Some(action));
        if result.is_err() {
            if let Ok(guard) = state.catalog.import_job.lock() {
                if let Some(job) = guard.as_ref() {
                    job.set_phase(catalog::StreamingImportJobPhase::Completed);
                }
            }
        }
        result
    })
    .await
    .map_err(|_| internal_error("streaming import task failed"))?
}

/// Stops the running Streaming Import after the current download. Pending
/// items are reported as `cancelled`. While a Library Decision is open the
/// flag is applied when the decision resumes the import.
#[tauri::command]
pub fn cancel_streaming_import(state: State<'_, AppState>) -> CommandResult<()> {
    let guard = state
        .catalog
        .import_job
        .lock()
        .map_err(|_| state_lock_error("catalog import job lock was poisoned"))?;
    if let Some(job) = guard.as_ref() {
        job.cancel();
    }
    Ok(())
}

#[tauri::command]
pub fn get_streaming_import_status(
    state: State<'_, AppState>,
) -> CommandResult<Option<catalog::StreamingImportJobSnapshot>> {
    let guard = state
        .catalog
        .import_job
        .lock()
        .map_err(|_| state_lock_error("catalog import job lock was poisoned"))?;
    Ok(guard.as_ref().map(|job| job.snapshot()))
}

fn new_import_job_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("import-{nanos}")
}

fn run_import_on_library<S: StreamingSource>(
    state: &AppState,
    source: &S,
    session: &mut StreamingImportSession,
    action: Option<catalog::LibraryDecisionAction>,
) -> CommandResult<catalog::StreamingImportProgress> {
    let library = state.library_root()?;
    let mut connection = cache::open_database(&library.database_path()).map_err(database_error)?;
    let progress =
        catalog::advance_import_session(&mut connection, &library, source, session, action)
            .map_err(Into::<crate::commands::error::CommandError>::into)?;
    if progress.status == StreamingImportStatus::AwaitingDecision {
        *state
            .catalog
            .import_session
            .lock()
            .map_err(|_| state_lock_error("catalog import session lock was poisoned"))? =
            Some(std::mem::replace(
                session,
                StreamingImportSession::new(StreamingImportRequest {
                    source_id: NETEASE_SOURCE_ID.to_owned(),
                    items: Vec::new(),
                    playlist: None,
                }),
            ));
    }
    Ok(progress)
}

#[tauri::command]
pub async fn resolve_video_source_url(
    app_handle: AppHandle,
    source_id: String,
    url: String,
) -> CommandResult<Vec<catalog::VideoQueueItem>> {
    require_source(&app_handle, &source_id)?;
    if source_id != YOUTUBE_SOURCE_ID {
        return Err(internal_error(format!("unknown video source: {source_id}")));
    }
    if url.contains("/player") {
        return Err(internal_error(
            "YouTube /player stream URLs are not used".to_owned(),
        ));
    }
    run_blocking(move || {
        let fetcher = LiveYoutubeFetcher::new()
            .map_err(Into::<crate::commands::error::CommandError>::into)?;
        YoutubeVideoSource::new(fetcher)
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
