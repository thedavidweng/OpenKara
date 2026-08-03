use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::commands::bootstrap::ModelBootstrapState;
use crate::commands::error::{internal_error, CommandResult};
use crate::commands::runtime_bootstrap::RuntimeBootstrapState;
use crate::config::{self, ExecutionProviderPreference, ModelVariant};
use crate::{separator, AppState};

const BUILD_SHA: &str = match option_env!("GIT_BUILD_HASH") {
    Some(sha) => sha,
    None => "unknown",
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DebugInfo {
    pub app_version: String,
    pub build_sha: String,
    pub os: String,
    pub arch: String,
    pub catalog_generation: u64,
    pub catalog_release_id: String,
    pub model_variant: String,
    pub model_state: String,
    pub model_installed: bool,
    pub model_installed_version: Option<String>,
    pub model_pinned_version: String,
    /// Where the active variant's model file is expected on disk. Surfaced so a
    /// user on a slow connection can place a verified download there by hand
    /// instead of guessing the app data layout (#270).
    pub model_path: String,
    pub runtime_state: String,
    pub runtime_version: String,
    pub runtime_artifact_id: Option<String>,
    pub runtime_target_triple: String,
    pub execution_provider: String,
    pub log_file: String,
}

fn model_state_label(state: &ModelBootstrapState) -> &'static str {
    match state {
        ModelBootstrapState::Pending => "pending",
        ModelBootstrapState::Downloading => "downloading",
        ModelBootstrapState::Outdated => "outdated",
        ModelBootstrapState::Ready => "ready",
        ModelBootstrapState::Failed => "failed",
    }
}

fn runtime_state_label(state: &RuntimeBootstrapState) -> &'static str {
    match state {
        RuntimeBootstrapState::Missing => "missing",
        RuntimeBootstrapState::Downloading => "downloading",
        RuntimeBootstrapState::Installing => "installing",
        RuntimeBootstrapState::Probing => "probing",
        RuntimeBootstrapState::Activating => "activating",
        RuntimeBootstrapState::Ready => "ready",
        RuntimeBootstrapState::UpdateAvailable => "update_available",
        RuntimeBootstrapState::DownloadingCandidate => "downloading_candidate",
        RuntimeBootstrapState::CandidateReadyRestartRequired => "candidate_ready_restart_required",
        RuntimeBootstrapState::ActivationFailedPreviousRestored => {
            "activation_failed_previous_restored"
        }
        RuntimeBootstrapState::Corrupt => "corrupt",
        RuntimeBootstrapState::Failed => "failed",
    }
}

pub fn assemble_debug_info(
    app_version: String,
    build_sha: &str,
    os: &str,
    arch: &str,
    catalog_generation: u64,
    catalog_release_id: String,
    model_variant: &str,
    model_state: &str,
    model_installed: bool,
    model_installed_version: Option<String>,
    model_pinned_version: String,
    model_path: String,
    runtime_state: &str,
    runtime_version: String,
    runtime_artifact_id: Option<String>,
    runtime_target_triple: String,
    execution_provider: &str,
    log_file: String,
) -> DebugInfo {
    DebugInfo {
        app_version,
        build_sha: build_sha.to_owned(),
        os: os.to_owned(),
        arch: arch.to_owned(),
        catalog_generation,
        catalog_release_id,
        model_variant: model_variant.to_owned(),
        model_state: model_state.to_owned(),
        model_installed,
        model_installed_version,
        model_pinned_version,
        model_path,
        runtime_state: runtime_state.to_owned(),
        runtime_version,
        runtime_artifact_id,
        runtime_target_triple,
        execution_provider: execution_provider.to_owned(),
        log_file,
    }
}

#[tauri::command]
pub fn get_debug_info(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<DebugInfo> {
    let app_data_dir = state.shell.app_data_dir.clone();

    let log_file = app_handle
        .path()
        .app_log_dir()
        .map(|dir| crate::logging::log_file_hint(&dir).display().to_string())
        .unwrap_or_else(|_| "unknown".to_owned());

    let config = config::load_config(&app_data_dir)
        .ok()
        .flatten()
        .unwrap_or_default();
    let variant: ModelVariant = config.effective_model_variant();
    let execution_provider: ExecutionProviderPreference = config.effective_execution_provider();

    let catalog = separator::catalog::embedded_catalog();

    let descriptor = separator::bootstrap::descriptor_for(variant);
    let managed_model_path =
        separator::bootstrap::managed_model_path_for(&app_data_dir, descriptor);
    let model_snapshot = state
        .shell
        .model_bootstrap_status
        .lock()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| internal_error("model bootstrap status lock was poisoned"))?;
    let model_installed = matches!(model_snapshot.state, ModelBootstrapState::Ready);
    let model_installed_version = if model_installed {
        separator::catalog::read_installed_identity(&managed_model_path)
            .map(|identity| identity.upstream_version)
            .or_else(|| Some(descriptor.upstream_tag.clone()))
    } else {
        None
    };

    let runtime_snapshot = state
        .shell
        .runtime_bootstrap_status
        .lock()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| internal_error("runtime bootstrap status lock was poisoned"))?;

    Ok(assemble_debug_info(
        app_handle.package_info().version.to_string(),
        BUILD_SHA,
        std::env::consts::OS,
        std::env::consts::ARCH,
        catalog.generation,
        catalog.release_id.clone(),
        variant.as_str(),
        model_state_label(&model_snapshot.state),
        model_installed,
        model_installed_version,
        descriptor.upstream_tag.clone(),
        managed_model_path.display().to_string(),
        runtime_state_label(&runtime_snapshot.state),
        runtime_snapshot.version.clone(),
        runtime_snapshot.active_artifact_id.clone(),
        runtime_snapshot.target_triple.clone(),
        execution_provider.as_str(),
        log_file,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DebugInfo {
        assemble_debug_info(
            "0.9.1".to_owned(),
            "abc1234",
            "macos",
            "aarch64",
            9,
            "2026-07-25-006".to_owned(),
            "htdemucs",
            "ready",
            true,
            Some("model-v2.1.0".to_owned()),
            "model-v2.1.0".to_owned(),
            "/Users/me/Library/Application Support/com.openkara.desktop/models/htdemucs.onnx"
                .to_owned(),
            "ready",
            "v1.27.1".to_owned(),
            Some("onnxruntime-1.27.1-openkara-aarch64-apple-darwin".to_owned()),
            "aarch64-apple-darwin".to_owned(),
            "xnnpack",
            "/Users/me/Library/Logs/com.openkara.desktop/openkara.<date>.log".to_owned(),
        )
    }

    #[test]
    fn assembles_every_field() {
        let info = sample();
        assert_eq!(info.app_version, "0.9.1");
        assert_eq!(info.build_sha, "abc1234");
        assert_eq!(info.os, "macos");
        assert_eq!(info.arch, "aarch64");
        assert_eq!(info.catalog_generation, 9);
        assert_eq!(info.catalog_release_id, "2026-07-25-006");
        assert_eq!(info.model_variant, "htdemucs");
        assert_eq!(info.model_state, "ready");
        assert_eq!(
            info.model_path,
            "/Users/me/Library/Application Support/com.openkara.desktop/models/htdemucs.onnx"
        );
        assert!(info.model_installed);
        assert_eq!(
            info.model_installed_version.as_deref(),
            Some("model-v2.1.0")
        );
        assert_eq!(info.model_pinned_version, "model-v2.1.0");
        assert_eq!(info.runtime_state, "ready");
        assert_eq!(info.runtime_version, "v1.27.1");
        assert_eq!(
            info.runtime_artifact_id.as_deref(),
            Some("onnxruntime-1.27.1-openkara-aarch64-apple-darwin")
        );
        assert_eq!(info.runtime_target_triple, "aarch64-apple-darwin");
        assert_eq!(info.execution_provider, "xnnpack");
        assert!(info.log_file.ends_with("openkara.<date>.log"));
    }

    #[test]
    fn serializes_to_snake_case_json() {
        let json = serde_json::to_value(sample()).expect("debug info serializes");
        for key in [
            "app_version",
            "build_sha",
            "os",
            "arch",
            "catalog_generation",
            "catalog_release_id",
            "model_variant",
            "model_state",
            "model_installed",
            "model_installed_version",
            "model_pinned_version",
            "runtime_state",
            "runtime_version",
            "runtime_artifact_id",
            "runtime_target_triple",
            "execution_provider",
            "log_file",
        ] {
            assert!(json.get(key).is_some(), "missing key: {key}");
        }
    }

    #[test]
    fn model_state_labels_are_stable() {
        assert_eq!(model_state_label(&ModelBootstrapState::Ready), "ready");
        assert_eq!(model_state_label(&ModelBootstrapState::Pending), "pending");
        assert_eq!(model_state_label(&ModelBootstrapState::Failed), "failed");
    }

    #[test]
    fn runtime_state_labels_are_stable() {
        assert_eq!(runtime_state_label(&RuntimeBootstrapState::Ready), "ready");
        assert_eq!(
            runtime_state_label(&RuntimeBootstrapState::Missing),
            "missing"
        );
        assert_eq!(
            runtime_state_label(&RuntimeBootstrapState::CandidateReadyRestartRequired),
            "candidate_ready_restart_required"
        );
    }

    #[test]
    fn not_installed_model_reports_no_version() {
        let info = assemble_debug_info(
            "0.9.1".to_owned(),
            "abc1234",
            "linux",
            "x86_64",
            9,
            "2026-07-25-006".to_owned(),
            "htdemucs_ft",
            "pending",
            false,
            None,
            "model-v2.1.0".to_owned(),
            "/home/me/.local/share/com.openkara.desktop/models/htdemucs_ft.onnx".to_owned(),
            "missing",
            "v1.27.1".to_owned(),
            None,
            "x86_64-unknown-linux-gnu".to_owned(),
            "cpu",
            "/home/me/.local/share/com.openkara.desktop/logs/openkara.<date>.log".to_owned(),
        );
        assert!(!info.model_installed);
        assert!(info.model_installed_version.is_none());
        assert!(info.runtime_artifact_id.is_none());
    }
}
