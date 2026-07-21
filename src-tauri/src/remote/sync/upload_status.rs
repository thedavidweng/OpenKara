use crate::{
    commands::error::{state_lock_error, CommandError, CommandResult},
    remote::control_db::{
        list_operations, upsert_operation, OperationKind, OperationPayload, OperationRow,
        OperationState,
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
///
/// Callers that have already written a control-plane state (for example the
/// executor writing `RetryWait` for a network fault) must not use this path
/// to demote that state back to `Failed`. See
/// [`resolve_durable_state_for_status_update`].
fn upload_state_to_operation_state(upload_state: UploadState) -> OperationState {
    match upload_state {
        UploadState::Idle => OperationState::Pending,
        UploadState::Running => OperationState::Running,
        UploadState::Completed => OperationState::Completed,
        UploadState::Failed => OperationState::Failed,
    }
}

/// Choose the durable state for a status write without demoting control-plane
/// retry state that the executor already persisted.
fn resolve_durable_state_for_status_update(
    existing: Option<&OperationRow>,
    upload_state: UploadState,
    error: Option<&CommandError>,
) -> OperationState {
    // Retryable failures must land as RetryWait so recovery can reschedule.
    // Non-retryable failures become terminal Failed.
    if matches!(upload_state, UploadState::Failed) {
        if let Some(existing) = existing {
            // Executor already recorded RetryWait — the UI path is
            // projection-only and must not demote control plane to Failed.
            if matches!(existing.state, OperationState::RetryWait) {
                return OperationState::RetryWait;
            }
            // Preserve Conflicted/Cancelled if somehow re-marked.
            if matches!(
                existing.state,
                OperationState::Conflicted | OperationState::Cancelled
            ) {
                return existing.state;
            }
        }
        if error.is_some_and(|e| e.retryable) {
            return OperationState::RetryWait;
        }
        return OperationState::Failed;
    }
    upload_state_to_operation_state(upload_state)
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

    // Persist to the durable control DB. We look up the most recent Publish
    // operation for this library+song to update it in place. If none exists
    // (e.g. status update before the publish row was created), we create a
    // new row with a UUID operation_id.
    if let Some(ref library_id) = remote_library_id {
        let now = control_db_now_ms();

        // Try to load the existing row to preserve created_at_ms and
        // attempt_count across updates. Look up by library+song rather than
        // a fixed operation_id derived from the song_id, because each
        // publish now gets a unique UUID operation_id.
        let existing = {
            let conn = state
                .remote
                .control_db
                .lock()
                .map_err(|_| state_lock_error("control DB lock was poisoned"))?;
            crate::remote::control_db::get_latest_publish_operation_for_song(
                &conn, library_id, song_id,
            )?
        };

        let operation_id = existing
            .as_ref()
            .map(|e| e.operation_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Preserve multi-song payloads written by the mutation layer. Only
        // replace song_ids when the existing row has none (legacy / new row).
        let mut existing_payload = existing
            .as_ref()
            .and_then(|e| OperationPayload::from_json(&e.payload_json).ok())
            .unwrap_or_default();
        if existing_payload.song_ids.is_empty() {
            existing_payload.song_ids = vec![song_id.to_owned()];
        } else if !existing_payload.song_ids.iter().any(|id| id == song_id) {
            existing_payload.song_ids.push(song_id.to_owned());
        }
        existing_payload.percent = percent;
        existing_payload.detail = detail.clone();

        let op_state = resolve_durable_state_for_status_update(
            existing.as_ref(),
            snapshot.state.clone(),
            error.as_ref(),
        );
        let (error_code, error_detail) = sanitize_error(error.as_ref());

        // Preserve a scheduled retry window when we keep RetryWait (either
        // from the executor or from a retryable failure write).
        let next_attempt_at_ms = if matches!(op_state, OperationState::RetryWait) {
            existing
                .as_ref()
                .and_then(|e| e.next_attempt_at_ms)
                .or_else(|| {
                    // First transition to RetryWait from this path (e.g. asset
                    // upload network fault before the executor ran).
                    Some(now + 30_000)
                })
        } else {
            existing.as_ref().and_then(|e| e.next_attempt_at_ms)
        };

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
            payload_json: existing_payload.to_json()?,
            attempt_count: existing.as_ref().map(|e| e.attempt_count).unwrap_or(0),
            next_attempt_at_ms,
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
        // Skip batch outbox entries that have no song_ids — these are
        // internal recovery bookkeeping (e.g. prepare_and_mutate records
        // a placeholder before song_ids are known). Surfacing them as
        // user-visible uploads would show a phantom "Running" entry that
        // never completes.
        .filter(|op| {
            OperationPayload::from_json(&op.payload_json)
                .map(|p| !p.song_ids.is_empty())
                .unwrap_or(false)
        })
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
        open_control_db, upsert_operation, OperationKind, OperationRow, OperationState,
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
        let op = crate::remote::control_db::get_latest_publish_operation_for_song(
            &conn, "lib-1", "song-1",
        )
        .unwrap()
        .expect("operation row should exist");
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

    #[test]
    fn mark_upload_status_failed_does_not_demote_retry_wait() {
        let (state, _dir) = test_state_with_control_db();
        let now = control_db_now_ms();
        // Executor already scheduled a durable retry for a network fault.
        upsert_operation(
            &state.remote.control_db.lock().unwrap(),
            &OperationRow {
                operation_id: "op-retry".to_owned(),
                library_id: "lib-1".to_owned(),
                operation_kind: OperationKind::Publish,
                state: OperationState::RetryWait,
                expected_generation: Some(1),
                target_generation: None,
                source_db_digest: None,
                candidate_db_digest: None,
                payload_json: r#"{"song_ids":["song-1"],"percent":40}"#.to_owned(),
                attempt_count: 1,
                next_attempt_at_ms: Some(now + 30_000),
                error_code: Some("NetworkUnavailable".to_owned()),
                error_detail: Some("connection reset".to_owned()),
                created_at_ms: now,
                updated_at_ms: now,
            },
        )
        .unwrap();

        // UI path after executor Err — must not overwrite RetryWait with Failed.
        mark_upload_status(
            &state,
            "song-1",
            Some("lib-1".to_owned()),
            UploadState::Failed,
            0,
            None,
            Some(crate::commands::error::CommandError::new(
                crate::commands::error::ErrorCode::NetworkUnavailable,
                "connection reset",
                true,
                crate::commands::error::FallbackAction::Retry,
            )),
        )
        .unwrap();

        let conn = state.remote.control_db.lock().unwrap();
        let op = crate::remote::control_db::get_latest_publish_operation_for_song(
            &conn, "lib-1", "song-1",
        )
        .unwrap()
        .expect("operation row");
        assert_eq!(
            op.state,
            OperationState::RetryWait,
            "retryable executor state must not be demoted to Failed"
        );
        assert!(
            op.next_attempt_at_ms.is_some(),
            "scheduled retry window must be preserved"
        );
    }

    #[test]
    fn mark_upload_status_retryable_failure_writes_retry_wait() {
        let (state, _dir) = test_state_with_control_db();
        mark_upload_status(
            &state,
            "song-1",
            Some("lib-1".to_owned()),
            UploadState::Running,
            10,
            None,
            None,
        )
        .unwrap();

        mark_upload_status(
            &state,
            "song-1",
            Some("lib-1".to_owned()),
            UploadState::Failed,
            0,
            None,
            Some(crate::commands::error::CommandError::new(
                crate::commands::error::ErrorCode::NetworkUnavailable,
                "timeout",
                true,
                crate::commands::error::FallbackAction::Retry,
            )),
        )
        .unwrap();

        let conn = state.remote.control_db.lock().unwrap();
        let op = crate::remote::control_db::get_latest_publish_operation_for_song(
            &conn, "lib-1", "song-1",
        )
        .unwrap()
        .expect("operation row");
        assert_eq!(op.state, OperationState::RetryWait);
        assert!(op.next_attempt_at_ms.is_some());
    }

    #[test]
    fn get_all_upload_statuses_filters_batch_rows_with_empty_song_ids() {
        let (state, _dir) = test_state_with_control_db();

        // Insert a batch outbox entry with empty song_ids (as produced by
        // prepare_and_mutate before song_ids are known).
        let now = control_db_now_ms();
        upsert_operation(
            &state.remote.control_db.lock().unwrap(),
            &OperationRow {
                operation_id: "publish-batch-123".to_owned(),
                library_id: "lib-1".to_owned(),
                operation_kind: OperationKind::Publish,
                state: OperationState::Pending,
                expected_generation: None,
                target_generation: None,
                source_db_digest: None,
                candidate_db_digest: None,
                payload_json: r#"{"song_ids":[],"percent":0}"#.to_owned(),
                attempt_count: 0,
                next_attempt_at_ms: None,
                error_code: None,
                error_detail: None,
                created_at_ms: now,
                updated_at_ms: now,
            },
        )
        .unwrap();

        // Insert a real per-song upload row.
        mark_upload_status(
            &state,
            "song-1",
            Some("lib-1".to_owned()),
            UploadState::Running,
            50,
            None,
            None,
        )
        .unwrap();

        let statuses = get_all_upload_statuses(&state).unwrap();
        // Only the per-song row should appear; the batch placeholder is
        // filtered out.
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].song_id, "song-1");
    }
}
