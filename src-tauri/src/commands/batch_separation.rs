//! IPC adapters for batch stem separation.
//!
//! Planning, prerequisite bootstrap, sequential job loop, and per-song status
//! emission live in `services::separation` so single and batch paths share one
//! orchestration layer.

use crate::{
    cache, commands::error::CommandResult, separator::error::SeparationError, services::separation,
    AppState,
};
use std::sync::{atomic::Ordering, Arc};
use tauri::{AppHandle, State};

// Re-export batch event contract for external callers/tests.
pub use crate::services::separation::{
    BatchSeparationProgress, BATCH_SEPARATION_CANCELLED_EVENT, BATCH_SEPARATION_COMPLETE_EVENT,
    BATCH_SEPARATION_PROGRESS_EVENT,
};

/// Batch separate songs. If `song_ids` is empty, separate all songs in the library.
/// Songs are processed sequentially (ONNX Runtime is memory-heavy).
#[tauri::command]
pub fn batch_separate(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_ids: Vec<String>,
) -> CommandResult<()> {
    if state.separation.batch_running.load(Ordering::Relaxed) {
        return Err(
            SeparationError::Failed("A batch separation is already running".to_owned()).into(),
        );
    }

    let execution_context = separation::build_execution_context(&state)?;
    let stem_mode = execution_context.stem_mode;

    // Open database connection once for all queries.
    let connection = cache::open_database(&execution_context.library_root.database_path())
        .map_err(|e| crate::commands::error::database_error(e.to_string()))?;

    let plan = separation::plan_batch(
        &connection,
        &execution_context.library_root,
        song_ids,
        stem_mode,
    )?;
    drop(connection);

    separation::start_batch_job(
        app_handle,
        execution_context,
        plan,
        Arc::clone(&state.separation.batch_running),
        Arc::clone(&state.separation.batch_cancel),
    );

    Ok(())
}

#[tauri::command]
pub fn cancel_batch_separation(state: State<'_, AppState>) -> CommandResult<()> {
    if !state.separation.batch_running.load(Ordering::Relaxed) {
        return Err(
            SeparationError::Failed("No batch separation is currently running".to_owned()).into(),
        );
    }
    state.separation.batch_cancel.store(true, Ordering::Relaxed);
    Ok(())
}
