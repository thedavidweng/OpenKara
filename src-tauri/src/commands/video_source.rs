use crate::catalog::{self, VideoSource};
use crate::commands::error::{internal_error, CommandResult};
use crate::config;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn resolve_video_source_url(
    app_handle: AppHandle,
    source_id: String,
    url: String,
) -> CommandResult<Vec<catalog::VideoQueueItem>> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|_| internal_error("could not get application data directory"))?;
    let config = config::load_config(&app_data_dir)
        .map_err(internal_error)?
        .unwrap_or_default();
    catalog::require_enabled(&config, &source_id)?;
    if source_id != catalog::YOUTUBE_SOURCE_ID {
        return Err(internal_error("unknown video source"));
    }
    if url.contains("/player") {
        return Err(internal_error("YouTube /player stream URLs are not used"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let fetcher = catalog::LiveYoutubeFetcher::new()?;
        catalog::YoutubeVideoSource::new(fetcher)
            .resolve(&url)
            .map_err(Into::into)
    })
    .await
    .map_err(|_| internal_error("source operation failed"))?
}
