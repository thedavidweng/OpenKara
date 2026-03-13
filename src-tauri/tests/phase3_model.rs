use std::path::PathBuf;

use openkara_lib::separator::model;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn resolves_default_demucs_model_path() {
    let model_path = model::default_model_path();

    assert!(model_path.ends_with("src-tauri/models/htdemucs_embedded.onnx"));
    assert!(model_path.exists());
}

#[test]
fn loads_embedded_demucs_model_session() {
    let loaded = model::load_from_path(&repo_root().join("models").join("htdemucs_embedded.onnx"))
        .expect("demucs model should load");

    assert!(!loaded.inputs.is_empty());
    assert!(!loaded.outputs.is_empty());
}

#[test]
fn fails_with_clear_error_for_missing_model_file() {
    let missing_path = repo_root().join("models").join("missing-model.onnx");
    let error = model::load_from_path(&missing_path).expect_err("missing model should fail");

    assert!(error.to_string().contains("missing-model.onnx"));
}
