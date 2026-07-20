use crate::{
    commands::error::{state_lock_error, CommandError, CommandResult},
    remote::control_db::{
        get_operation, list_operations, upsert_operation, OperationKind, OperationPayload,
        OperationRow, OperationState,
    },
    AppState,
};
use tauri::{AppHandle, Emitter};

use super::super::types::{
    UploadCompletePayload, UploadErrorPayload, UploadProgressPayload, UploadState,
    UploadStatusSnapshot,
};

/// Derive the durable operation state from the in-memory `UploadState`.
///
/// The in-memory `UploadState` enum is the IPC-facing projection (Idle /
/// Running / Completed / Failed). The durable `OperationState` is richer and
/// tracks the full outbox lifecycle. This mapping records the projection back
/// into the durable row.
fn upload_state_to_operation_state(upload_state: UploadState) -> OperationState {
    match upload_state {
        UploadState::Idle => OperationState::Pending,
        UploadState::Running => OperationState::Running,
        UploadState::Completed => OperationState::Completed,
        UploadState::Failed => OperationState::Failed,
    }
}

/// Derive the in-memory `UploadState` from a durable `OperationState`.
fn operation_state_to_upload_state(op_state: OperationState) -> UploadState {
    match op_state {
        OperationState::Prepared | OperationState::Pending | OperationState::RetryWait => {
            UploadState::Running
        }
        OperationState::Running | OperationState::Committing | OperationState::Verifying => {
            UploadState::Running
        }
        OperationState::Completed => UploadState::Completed,
        OperationState::Failed => UploadState::Failed,
        OperationState::Conflicted | OperationState::Cancelled => UploadState::Failed,
    }
}

/// Derive the percent value from a durable operation row's payload.
fn percent_from_operation(op: &OperationRow) -> u8 {
    match op.state {
        OperationState::Completed => 100,
        OperationState::Failed | OperationState::Conflicted | OperationState::Cancelled => 0,
        _ => OperationPayload::from_json(&op.payload_json)
            .map(|p| p.percent)
            .unwrap_or(0),
    }
}

/// Derive the detail string from a durable operation row.
fn detail_from_operation(op: &OperationRow) -> Option<String> {
    if let Ok(payload) = OperationPayload::from_json(&op.payload_json) {
        payload.detail
    } else {
        None
    }
}

/// Derive the error from a durable operation row's `error_code`/`error_detail`.
fn error_from_operation(op: &OperationRow) -> Option<CommandError> {
    op.error_code.as_deref().map(|code| {
        CommandError::from(crate::library::error::LibraryError::Internal(
            op.error_detail.clone().unwrap_or_else(|| code.to_owned()),
        ))
    })
}

/// Map a `remote_operations` row to an `UploadStatusSnapshot`.
///
/// For publish operations, `payload_json` carries the song_ids and percent.
/// The first song_id in the payload is used as the snapshot's `song_id` so
/// each song in a multi-song publish gets its own row when listed. When the
/// payload has no song_ids, the operation_id is used as a fallback key.
pub(crate) fn operation_to_snapshot(op: &OperationRow) -> UploadStatusSnapshot {
    let payload = OperationPayload::from_json(&op.payload_json).ok();
    let song_id = payload
        .as_ref()
        .and_then(|p| p.song_ids.first().cloned())
        .unwrap_or_else(|| op.operation_id.clone());
    UploadStatusSnapshot {
        song_id,
        state: operation_state_to_upload_state(op.state),
        percent: percent_from_operation(op),
        remote_library_id: Some(op.library_id.clone()),
        detail: detail_from_operation(op),
        error: error_from_operation(op),
    }
}

/// Record or update an upload status.
///
/// This persists the operation to the durable `remote_operations` table (the
/// source of truth) AND updates the in-memory projection (used for event
/// delivery so we don't re-emit events for unchanged state).
///
/// The `payload_json` shape is:
/// ```json
/// {"song_ids": ["hash-a"], "percent": 42, "detail": "Uploading stems"}
/// ```
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
        remote_library_id: remote_library_id.clone(),
        detail: detail.clone(),
        error: error.clone(),
    };

    // Persist to the durable control DB. The operation_id is derived from the
    // song_id so repeated status updates for the same song update the same row
    // rather than creating duplicates.
    if let Some(ref library_id) = remote_library_id {
        let operation_id = publish_operation_id(song_id);
        let now = control_db_now_ms();

        // Try to load the existing row to preserve created_at_ms and
        // attempt_count across updates.
        let existing = {
            let conn = state
                .remote
                .control_db
                .lock()
                .map_err(|_| state_lock_error("control DB lock was poisoned"))?;
            get_operation(&conn, &operation_id)?
        };

        let payload = OperationPayload {
            song_ids: vec![song_id.to_owned()],
            percent,
            detail: detail.clone(),
        };

        let op_state = upload_state_to_operation_state(snapshot.state.clone());
        let (error_code, error_detail) = sanitize_error(error.as_ref());

        let row = OperationRow {
            operation_id: operation_id.clone(),
            library_id: library_id.clone(),
            operation_kind: OperationKind::Publish,
            state: op_state,
            expected_generation: existing.as_ref().and_then(|e| e.expected_generation),
            target_generation: existing.as_ref().and_then(|e| e.target_generation),
            source_db_digest: existing.as_ref().and_then(|e| e.source_db_digest.clone()),
            candidate_db_digest: existing
                .as_ref()
                .and_then(|e| e.candidate_db_digest.clone()),
            payload_json: payload.to_json()?,
            attempt_count: existing.as_ref().map(|e| e.attempt_count).unwrap_or(0),
            next_attempt_at_ms: existing.as_ref().and_then(|e| e.next_attempt_at_ms),
            error_code,
            error_detail,
            created_at_ms: existing.as_ref().map(|e| e.created_at_ms).unwrap_or(now),
            updated_at_ms: now,
        };

        let conn = state
            .remote
            .control_db
            .lock()
            .map_err(|_| state_lock_error("control DB lock was poisoned"))?;
        upsert_operation(&conn, &row)?;
    }

    // Update the in-memory projection.
    let mut guard = state
        .remote
        .remote_upload_statuses
        .lock()
        .map_err(|_| state_lock_error("remote upload status lock was poisoned"))?;
    guard.insert(song_id.to_owned(), snapshot.clone());
    Ok(snapshot)
}

/// Derive a stable operation_id for a publish operation from the song_id.
/// This ensures repeated `mark_upload_status` calls for the same song update
/// the same durable row.
pub(crate) fn publish_operation_id(song_id: &str) -> String {
    format!("publish-{song_id}")
}

/// Current time in milliseconds. Centralized so tests could inject a clock
/// in the future.
fn control_db_now_ms() -> i64 {
    crate::remote::types::current_unix_time_ms()
}

/// Sanitize an error into machine-readable code + detail strings for durable
/// storage. Never persists OAuth tokens, passwords, or raw provider responses.
fn sanitize_error(error: Option<&CommandError>) -> (Option<String>, Option<String>) {
    match error {
        Some(err) => {
            let code = format!("{:?}", err.code);
            // The error message is already sanitized by the command error
            // layer (static user-facing messages). We store it as the detail.
            (Some(code), Some(err.message.clone()))
        }
        None => (None, None),
    }
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

/// Read all upload statuses from the durable `remote_operations` table.
///
/// The durable table is the source of truth. The in-memory `HashMap` is only a
/// projection for event delivery, so after a restart this command still
/// returns meaningful statuses.
pub fn get_all_upload_statuses(state: &AppState) -> CommandResult<Vec<UploadStatusSnapshot>> {
    let conn = state
        .remote
        .control_db
        .lock()
        .map_err(|_| state_lock_error("control DB lock was poisoned"))?;

    let operations = list_operations(&conn)?;
    let snapshots: Vec<UploadStatusSnapshot> = operations
        .iter()
        .filter(|op| op.operation_kind == OperationKind::Publish)
        .map(operation_to_snapshot)
        .collect();

    // Sync the in-memory projection so subsequent event delivery has a
    // baseline to diff against.
    let mut guard = state
        .remote
        .remote_upload_statuses
        .lock()
        .map_err(|_| state_lock_error("remote upload status lock was poisoned"))?;
    for snapshot in &snapshots {
        guard.insert(snapshot.song_id.clone(), snapshot.clone());
    }

    Ok(snapshots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::control_db::{
        get_operation, open_control_db, upsert_operation, OperationKind, OperationRow,
        OperationState,
    };
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    fn test_state_with_control_db() -> (AppState, TempDir) {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("remote-state.db");
        let conn = open_control_db(&path).expect("open control DB");

        let mut state = AppState::test_fixture();
        state.remote.control_db = Arc::new(Mutex::new(conn));
        (state, dir)
    }

    #[test]
    fn mark_upload_status_persists_to_control_db() {
        let (state, _dir) = test_state_with_control_db();

        mark_upload_status(
            &state,
            "song-1",
            Some("lib-1".to_owned()),
            UploadState::Running,
            42,
            Some("Uploading".to_owned()),
            None,
        )
        .unwrap();

        let conn = state.remote.control_db.lock().unwrap();
        let op = get_operation(&conn, "publish-song-1").unwrap().unwrap();
        assert_eq!(op.operation_kind, OperationKind::Publish);
        assert_eq!(op.state, OperationState::Running);
        let payload = OperationPayload::from_json(&op.payload_json).unwrap();
        assert_eq!(payload.song_ids, vec!["song-1".to_owned()]);
        assert_eq!(payload.percent, 42);
    }

    #[test]
    fn get_all_upload_statuses_reads_from_control_db() {
        let (state, _dir) = test_state_with_control_db();

        mark_upload_status(
            &state,
            "song-1",
            Some("lib-1".to_owned()),
            UploadState::Completed,
            100,
            None,
            None,
        )
        .unwrap();

        // Simulate restart: clear the in-memory projection.
        state.remote.remote_upload_statuses.lock().unwrap().clear();

        let statuses = get_all_upload_statuses(&state).unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].song_id, "song-1");
        assert_eq!(statuses[0].state, UploadState::Completed);
        assert_eq!(statuses[0].percent, 100);
    }

    #[test]
    fn get_all_upload_statuses_returns_rows_after_close_and_reopen() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("remote-state.db");

        // Write a row using one connection (simulates a prior session).
        {
            let conn = open_control_db(&path).unwrap();
            upsert_operation(
                &conn,
                &OperationRow {
                    operation_id: "publish-song-x".to_owned(),
                    library_id: "lib-1".to_owned(),
                    operation_kind: OperationKind::Publish,
                    state: OperationState::Failed,
                    expected_generation: None,
                    target_generation: None,
                    source_db_digest: None,
                    candidate_db_digest: None,
                    payload_json: r#"{"song_ids":["song-x"],"percent":0}"#.to_owned(),
                    attempt_count: 1,
                    next_attempt_at_ms: None,
                    error_code: Some("Internal".to_owned()),
                    error_detail: Some("network error".to_owned()),
                    created_at_ms: 1000,
                    updated_at_ms: 2000,
                },
            )
            .unwrap();
        }

        // Reopen (simulates restart) and read.
        let conn = open_control_db(&path).unwrap();
        let mut state = AppState::test_fixture();
        state.remote.control_db = Arc::new(Mutex::new(conn));

        let statuses = get_all_upload_statuses(&state).unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].song_id, "song-x");
        assert_eq!(statuses[0].state, UploadState::Failed);
        assert!(statuses[0].error.is_some());
    }

    #[test]
    fn mark_upload_status_without_library_does_not_persist() {
        let (state, _dir) = test_state_with_control_db();

        // No remote_library_id — should not create a durable row.
        mark_upload_status(&state, "song-1", None, UploadState::Running, 0, None, None).unwrap();

        let conn = state.remote.control_db.lock().unwrap();
        let ops = list_operations(&conn).unwrap();
        assert!(ops.is_empty());
    }
}
