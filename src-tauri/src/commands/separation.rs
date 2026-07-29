use crate::{
    commands::error::CommandResult,
    config::StemMode,
    services::separation::{self, SeparationStatusSnapshot},
    AppState,
};
use tauri::{AppHandle, State};

pub use crate::services::separation::{
    completed_status, completed_status_with_model, failed_status, get_separation_status_from_map,
    idle_status, running_status, SeparationCancelledEvent, SeparationCompleteEvent,
    SeparationErrorEvent, SeparationProgressEvent, SeparationState, SEPARATION_CANCELLED_EVENT,
    SEPARATION_COMPLETE_EVENT, SEPARATION_ERROR_EVENT, SEPARATION_PROGRESS_EVENT,
};

#[tauri::command]
pub fn separate(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
) -> CommandResult<SeparationStatusSnapshot> {
    separation::ensure_song_can_be_separated(&state, &song_id)?;

    let initial_status =
        separation::reserve_running_status(&state.separation.separation_statuses, &song_id, true)?;
    let execution_context = separation::build_execution_context(&state)?;
    let stem_mode = execution_context.stem_mode;

    separation::start_job(app_handle, execution_context, song_id, stem_mode);

    Ok(initial_status)
}

#[tauri::command]
pub fn upgrade_to_four_stem(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
) -> CommandResult<SeparationStatusSnapshot> {
    separation::ensure_song_can_be_separated(&state, &song_id)?;

    if let Some(completed) = separation::try_completed_four_stem_status(&state, &song_id)? {
        return Ok(completed);
    }

    let initial_status =
        separation::reserve_running_status(&state.separation.separation_statuses, &song_id, true)?;
    let execution_context = separation::build_execution_context(&state)?;

    separation::start_job(app_handle, execution_context, song_id, StemMode::FourStem);

    Ok(initial_status)
}

#[tauri::command]
pub fn re_separate(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
    stem_mode: StemMode,
) -> CommandResult<SeparationStatusSnapshot> {
    separation::ensure_song_can_be_separated(&state, &song_id)?;

    separation::clear_stem_cache_for_song(&state, &song_id)?;
    separation::clear_in_memory_status(&state, &song_id)?;

    let initial_status =
        separation::reserve_running_status(&state.separation.separation_statuses, &song_id, false)?;
    let execution_context = separation::build_execution_context(&state)?;

    separation::start_job(app_handle, execution_context, song_id, stem_mode);

    Ok(initial_status)
}

#[tauri::command]
pub fn downgrade_single_to_two_stem(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
) -> CommandResult<SeparationStatusSnapshot> {
    separation::downgrade_to_two_stem_and_publish(&state, &app_handle, &song_id)
}

#[tauri::command]
pub fn get_separation_status(
    state: State<'_, AppState>,
    song_id: String,
) -> CommandResult<SeparationStatusSnapshot> {
    separation::get_separation_status_from_map(&state.separation.separation_statuses, &song_id)
}

#[tauri::command]
pub fn cancel_separation(state: State<'_, AppState>, song_id: String) -> CommandResult<()> {
    separation::request_cancel(&state.separation.separation_cancels, &song_id);
    Ok(())
}

#[tauri::command]
pub fn get_all_separation_statuses(
    state: State<'_, AppState>,
) -> CommandResult<Vec<SeparationStatusSnapshot>> {
    separation::get_all_separation_statuses(&state)
}
