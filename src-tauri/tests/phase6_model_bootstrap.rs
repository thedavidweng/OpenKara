use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use openkara_lib::{
    commands::{self, error::ErrorCode},
    separator::bootstrap::{self, ModelSource},
};
use sha2::{Digest, Sha256};

fn unique_temp_dir() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();

    std::env::temp_dir().join(format!("openkara-phase6-model-bootstrap-{timestamp}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn write_file(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory should be created");
    }
    fs::write(path, contents).expect("fixture file should be written");
}

fn remove_dir_if_exists(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).expect("temporary directory should be removable");
    }
}

#[test]
fn resolve_existing_model_path_prefers_managed_install_over_dev_fallback() {
    let temp_dir = unique_temp_dir();
    let managed_path = temp_dir.join("managed").join("htdemucs_embedded.onnx");
    let dev_path = temp_dir.join("dev").join("htdemucs_embedded.onnx");
    let managed_bytes = b"managed-model";
    let dev_bytes = b"dev-model";

    write_file(&managed_path, managed_bytes);
    write_file(&dev_path, dev_bytes);

    let resolved = bootstrap::resolve_existing_model_path(
        &managed_path,
        &dev_path,
        &sha256_hex(managed_bytes),
    )
    .expect("resolution should succeed")
    .expect("managed install should be selected");

    assert_eq!(resolved.path, managed_path);
    assert_eq!(resolved.source, ModelSource::ManagedInstall);

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn resolve_existing_model_path_falls_back_to_verified_dev_model() {
    let temp_dir = unique_temp_dir();
    let managed_path = temp_dir.join("managed").join("htdemucs_embedded.onnx");
    let dev_path = temp_dir.join("dev").join("htdemucs_embedded.onnx");
    let dev_bytes = b"dev-model";

    write_file(&dev_path, dev_bytes);

    let resolved =
        bootstrap::resolve_existing_model_path(&managed_path, &dev_path, &sha256_hex(dev_bytes))
            .expect("resolution should succeed")
            .expect("development fallback should be selected");

    assert_eq!(resolved.path, dev_path);
    assert_eq!(resolved.source, ModelSource::DevelopmentFallback);

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn install_verified_model_bytes_writes_model_to_nested_runtime_directory() {
    let temp_dir = unique_temp_dir();
    let destination = temp_dir
        .join("runtime")
        .join("models")
        .join("htdemucs_embedded.onnx");
    let payload = b"fake-model";

    bootstrap::install_verified_model_bytes(&destination, payload, &sha256_hex(payload))
        .expect("verified payload should install");

    assert_eq!(
        fs::read(&destination).expect("installed model should be readable"),
        payload
    );

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn install_verified_model_bytes_rejects_checksum_mismatch_without_creating_destination() {
    let temp_dir = unique_temp_dir();
    let destination = temp_dir
        .join("runtime")
        .join("models")
        .join("htdemucs_embedded.onnx");

    let error = bootstrap::install_verified_model_bytes(&destination, b"fake-model", "not-a-sha")
        .expect_err("checksum mismatch should fail");

    assert!(error.to_string().contains("checksum mismatch"));
    assert!(!destination.exists());

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn get_model_bootstrap_status_returns_latest_snapshot() {
    let statuses = Arc::new(Mutex::new(commands::bootstrap::ready_status(
        "/tmp/openkara-model.onnx",
    )));

    let snapshot = commands::bootstrap::get_model_bootstrap_status_from_state(&statuses)
        .expect("status lookup should succeed");

    assert_eq!(
        snapshot.state,
        commands::bootstrap::ModelBootstrapState::Ready
    );
    assert_eq!(snapshot.model_path, "/tmp/openkara-model.onnx");
}

#[test]
fn ensure_model_ready_rejects_download_in_progress() {
    let statuses = Arc::new(Mutex::new(commands::bootstrap::downloading_status(
        "/tmp/openkara-model.onnx",
        128,
        Some(256),
    )));

    let error = commands::bootstrap::ensure_model_ready(&statuses)
        .expect_err("download in progress should block separation");

    assert_eq!(error.code, ErrorCode::ModelUnavailable);
}
