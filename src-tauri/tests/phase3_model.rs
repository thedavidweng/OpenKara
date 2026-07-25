mod support;

use std::path::PathBuf;

use openkara_lib::{
    config::{ExecutionProviderPreference, ModelVariant},
    separator::{model, runtime_bootstrap},
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn resolves_default_demucs_model_path() {
    let model_path = model::default_model_path();

    let expected_name =
        &openkara_lib::separator::bootstrap::descriptor_for(ModelVariant::Htdemucs).filename;
    assert!(model_path.ends_with(format!("src-tauri/models/{expected_name}")));
    assert!(model_path.exists());
}

#[test]
fn managed_runtime_installs_live_under_the_runtimes_root() {
    let app_data_dir = support::unique_temp_path("phase3-managed-runtime");
    let artifact_dir =
        runtime_bootstrap::runtime_artifact_dir(&app_data_dir, "onnxruntime-test-artifact");

    assert_eq!(
        artifact_dir,
        app_data_dir
            .join("runtimes")
            .join("onnxruntime-test-artifact")
    );
    assert_eq!(
        runtime_bootstrap::legacy_runtime_path(&app_data_dir),
        app_data_dir
            .join("runtime")
            .join(model::ORT_RUNTIME_FILENAME)
    );
}

fn initialize_test_runtime() {
    let runtime_path = repo_root()
        .join("generated")
        .join("onnxruntime")
        .join(model::ORT_RUNTIME_FILENAME);
    model::ensure_runtime_loaded_from_path(&runtime_path)
        .expect("CI-prepared runtime should initialize explicitly for model tests");
}

#[test]
fn loads_embedded_demucs_model_session() {
    initialize_test_runtime();
    let loaded = model::load_from_path(
        &model::default_model_path(),
        ExecutionProviderPreference::Cpu,
    )
    .expect("demucs model should load");

    assert!(!loaded.inputs.is_empty());
    assert!(!loaded.outputs.is_empty());
}

#[test]
fn fails_with_clear_error_for_missing_model_file() {
    initialize_test_runtime();
    let missing_path = repo_root().join("models").join("missing-model.onnx");
    let error = model::load_from_path(&missing_path, ExecutionProviderPreference::Cpu)
        .expect_err("missing model should fail");

    assert!(error.to_string().contains("missing-model.onnx"));
}

#[test]
fn describes_cpu_only_provider_path() {
    assert_eq!(
        model::provider_diagnostic_summary(ExecutionProviderPreference::Cpu),
        "cpu"
    );
}

#[test]
fn describes_xnnpack_fallback_provider_path() {
    assert_eq!(
        model::provider_diagnostic_summary(ExecutionProviderPreference::Xnnpack),
        "xnnpack -> cpu"
    );
}

#[test]
fn describes_directml_full_fallback_provider_path() {
    assert_eq!(
        model::provider_diagnostic_summary(ExecutionProviderPreference::DirectMl),
        "directml -> xnnpack -> cpu"
    );
}

/// XNNPACK session creation should be near-instant because there is no AOT compile step.
/// Exercises the full XNNPACK -> CPU session-level fallback on the embedded model.
#[test]
fn loads_embedded_demucs_model_with_xnnpack_preference() {
    initialize_test_runtime();
    let loaded = model::load_from_path(
        &model::default_model_path(),
        ExecutionProviderPreference::Xnnpack,
    )
    .expect("demucs model should load with XNNPACK preference");

    assert!(!loaded.inputs.is_empty());
    assert!(!loaded.outputs.is_empty());
}
