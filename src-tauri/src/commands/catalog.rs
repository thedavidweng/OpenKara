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
    NeteaseStreamingSource::open(http, app_data_dir(app_handle)?).map_err(Into::into)
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

#[tauri::command]
pub fn sign_out_streaming_source(
    app_handle: AppHandle,
    source_id: String,
) -> CommandResult<catalog::StreamingSessionSnapshot> {
    require_source(&app_handle, &source_id)?;
    let session = netease_source(&app_handle)?.sign_out()?;
    if let Some(window) = app_handle.get_webview_window(NETEASE_SIGN_IN_WINDOW) {
        window
            .close()
            .map_err(|error| internal_error(error.to_string()))?;
    }
    Ok(session)
}

#[tauri::command]
pub fn list_streaming_liked_tracks(
    app_handle: AppHandle,
    source_id: String,
) -> CommandResult<Vec<catalog::StreamingTrack>> {
    require_source(&app_handle, &source_id)?;
    netease_source(&app_handle)?
        .liked_tracks()
        .map_err(Into::into)
}

#[tauri::command]
pub fn list_streaming_playlists(
    app_handle: AppHandle,
    source_id: String,
) -> CommandResult<Vec<catalog::StreamingPlaylistSummary>> {
    require_source(&app_handle, &source_id)?;
    netease_source(&app_handle)?.playlists().map_err(Into::into)
}

#[tauri::command]
pub fn get_streaming_playlist(
    app_handle: AppHandle,
    source_id: String,
    remote_playlist_id: String,
) -> CommandResult<catalog::StreamingPlaylistDetail> {
    require_source(&app_handle, &source_id)?;
    netease_source(&app_handle)?
        .playlist(&remote_playlist_id)
        .map_err(Into::into)
}

#[tauri::command]
pub fn search_streaming_source(
    app_handle: AppHandle,
    source_id: String,
    query: String,
) -> CommandResult<Vec<catalog::StreamingTrack>> {
    require_source(&app_handle, &source_id)?;
    netease_source(&app_handle)?
        .search(&query)
        .map_err(Into::into)
}

#[tauri::command]
pub fn start_streaming_import(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    remote_track_ids: Vec<String>,
    remote_playlist_id: Option<String>,
) -> CommandResult<catalog::StreamingImportProgress> {
    require_source(&app_handle, &source_id)?;
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
        remote_track_ids
            .into_iter()
            .map(|remote_track_id| catalog::StreamingImportItem {
                remote_track_id,
                title: String::new(),
                artist: String::new(),
            })
            .collect()
    };
    let request = StreamingImportRequest {
        source_id,
        items,
        playlist: playlist.map(|(name, remote_id, _)| StreamingPlaylistOrigin {
            remote_playlist_id: remote_id,
            name,
        }),
    };
    let mut session = StreamingImportSession::new(request);
    run_import_on_library(&state, &source, &mut session, None)
}

#[tauri::command]
pub fn continue_streaming_import(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    action: catalog::LibraryDecisionAction,
) -> CommandResult<catalog::StreamingImportProgress> {
    require_source(&app_handle, NETEASE_SOURCE_ID)?;
    let source = netease_source(&app_handle)?;
    let mut session = state
        .catalog
        .import_session
        .lock()
        .map_err(|_| state_lock_error("catalog import session lock was poisoned"))?
        .take()
        .ok_or_else(|| internal_error("no streaming import is waiting"))?;
    run_import_on_library(&state, &source, &mut session, Some(action))
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
pub fn resolve_video_source_url(
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
    let fetcher =
        LiveYoutubeFetcher::new().map_err(Into::<crate::commands::error::CommandError>::into)?;
    YoutubeVideoSource::new(fetcher)
        .resolve(&url)
        .map_err(Into::into)
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
