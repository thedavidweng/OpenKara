use crate::{
    commands::error::{model_bootstrap_error, state_lock_error, CommandError, CommandResult},
    AppState,
};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;

pub const MODEL_BOOTSTRAP_PROGRESS_EVENT: &str = "model-bootstrap-progress";
pub const MODEL_BOOTSTRAP_READY_EVENT: &str = "model-bootstrap-ready";
pub const MODEL_BOOTSTRAP_ERROR_EVENT: &str = "model-bootstrap-error";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelBootstrapState {
    Pending,
    Downloading,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelBootstrapStatusSnapshot {
    pub state: ModelBootstrapState,
    pub model_path: String,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub error: Option<CommandError>,
}

#[tauri::command]
pub fn get_model_bootstrap_status(
    state: State<'_, AppState>,
) -> CommandResult<ModelBootstrapStatusSnapshot> {
    get_model_bootstrap_status_from_state(&state.model_bootstrap_status)
}

pub fn get_model_bootstrap_status_from_state(
    status: &Arc<Mutex<ModelBootstrapStatusSnapshot>>,
) -> CommandResult<ModelBootstrapStatusSnapshot> {
    status
        .lock()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| state_lock_error("model bootstrap status lock was poisoned"))
}

pub fn ensure_model_ready(status: &Arc<Mutex<ModelBootstrapStatusSnapshot>>) -> CommandResult<()> {
    let snapshot = get_model_bootstrap_status_from_state(status)?;

    match snapshot.state {
        ModelBootstrapState::Ready => Ok(()),
        ModelBootstrapState::Pending => Err(model_bootstrap_error(format!(
            "model bootstrap has not started for {}",
            snapshot.model_path
        ))),
        ModelBootstrapState::Downloading => Err(model_bootstrap_error(format!(
            "model bootstrap is still downloading to {}",
            snapshot.model_path
        ))),
        ModelBootstrapState::Failed => Err(snapshot.error.unwrap_or_else(|| {
            model_bootstrap_error(format!(
                "model bootstrap failed for {}",
                snapshot.model_path
            ))
        })),
    }
}

pub fn pending_status(model_path: impl Into<String>) -> ModelBootstrapStatusSnapshot {
    ModelBootstrapStatusSnapshot {
        state: ModelBootstrapState::Pending,
        model_path: model_path.into(),
        downloaded_bytes: None,
        total_bytes: None,
        error: None,
    }
}

pub fn downloading_status(
    model_path: impl Into<String>,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) -> ModelBootstrapStatusSnapshot {
    ModelBootstrapStatusSnapshot {
        state: ModelBootstrapState::Downloading,
        model_path: model_path.into(),
        downloaded_bytes: Some(downloaded_bytes),
        total_bytes,
        error: None,
    }
}

pub fn ready_status(model_path: impl Into<String>) -> ModelBootstrapStatusSnapshot {
    ModelBootstrapStatusSnapshot {
        state: ModelBootstrapState::Ready,
        model_path: model_path.into(),
        downloaded_bytes: None,
        total_bytes: None,
        error: None,
    }
}

pub fn failed_status(
    model_path: impl Into<String>,
    error: CommandError,
) -> ModelBootstrapStatusSnapshot {
    ModelBootstrapStatusSnapshot {
        state: ModelBootstrapState::Failed,
        model_path: model_path.into(),
        downloaded_bytes: None,
        total_bytes: None,
        error: Some(error),
    }
}
