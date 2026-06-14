use crate::{
    commands::error::{state_lock_error, CommandError, CommandResult},
    AppState,
};
use tauri::{AppHandle, Emitter};

use super::super::types::{
    UploadCompletePayload, UploadErrorPayload, UploadProgressPayload, UploadState,
    UploadStatusSnapshot,
};

pub(crate) fn mark_upload_status(
    state: &AppState,
    song_id: &str,
    remote_library_id: Option<String>,
    upload_state: UploadState,
    percent: u8,
    detail: Option<String>,
    error: Option<CommandError>,
) -> CommandResult<UploadStatusSnapshot> {
    let snapshot = UploadStatusSnapshot {
        song_id: song_id.to_owned(),
        state: upload_state,
        percent,
        remote_library_id,
        detail,
        error,
    };

    let mut guard = state
        .remote
        .remote_upload_statuses
        .lock()
        .map_err(|_| state_lock_error("remote upload status lock was poisoned"))?;
    guard.insert(song_id.to_owned(), snapshot.clone());
    Ok(snapshot)
}

pub(crate) fn emit_upload_progress<R: tauri::Runtime>(
    app_handle: &AppHandle<R>,
    snapshot: &UploadStatusSnapshot,
) {
    let payload = UploadProgressPayload {
        song_id: snapshot.song_id.clone(),
        percent: snapshot.percent,
        remote_library_id: snapshot.remote_library_id.clone(),
        detail: snapshot.detail.clone(),
    };
    let _ = app_handle.emit("upload-progress", payload);
}

pub(crate) fn emit_upload_complete<R: tauri::Runtime>(
    app_handle: &AppHandle<R>,
    snapshot: &UploadStatusSnapshot,
) {
    let payload = UploadCompletePayload {
        song_id: snapshot.song_id.clone(),
        remote_library_id: snapshot.remote_library_id.clone(),
    };
    let _ = app_handle.emit("upload-complete", payload);
}

pub(crate) fn emit_upload_error<R: tauri::Runtime>(
    app_handle: &AppHandle<R>,
    snapshot: &UploadStatusSnapshot,
    error: CommandError,
) {
    let payload = UploadErrorPayload {
        song_id: snapshot.song_id.clone(),
        remote_library_id: snapshot.remote_library_id.clone(),
        error,
    };
    let _ = app_handle.emit("upload-error", payload);
}

pub fn get_all_upload_statuses(state: &AppState) -> CommandResult<Vec<UploadStatusSnapshot>> {
    let guard = state
        .remote
        .remote_upload_statuses
        .lock()
        .map_err(|_| state_lock_error("remote upload status lock was poisoned"))?;
    Ok(guard.values().cloned().collect())
}
