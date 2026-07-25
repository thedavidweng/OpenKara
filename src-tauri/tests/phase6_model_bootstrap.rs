use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

mod support;

use openkara_lib::{
    commands::{self, error::ErrorCode},
    config::ModelVariant,
    derive_startup_model_bootstrap, hash,
    separator::bootstrap::{self, ModelSource},
};
use sha2::{Digest, Sha256};

fn unique_temp_dir() -> PathBuf {
    support::unique_temp_path("phase6-model-bootstrap")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hash::hex_lower(hasher.finalize())
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
    let managed_path = temp_dir.join("managed").join("htdemucs.onnx");
    let dev_path = temp_dir.join("dev").join("htdemucs.onnx");
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
    let managed_path = temp_dir.join("managed").join("htdemucs.onnx");
    let dev_path = temp_dir.join("dev").join("htdemucs.onnx");
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
        .join("htdemucs.onnx");
    let payload = b"fake-model";

    bootstrap::install_verified_model_bytes(&destination, payload, &sha256_hex(payload))
        .expect("verified payload should install");

    assert_eq!(
        fs::read(&destination).expect("installed model should be readable"),
        payload
    );
    assert!(
        destination
            .with_file_name("htdemucs.onnx.verified.json")
            .exists(),
        "verified installs should persist a startup manifest"
    );

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn install_verified_model_bytes_rejects_checksum_mismatch_without_creating_destination() {
    let temp_dir = unique_temp_dir();
    let destination = temp_dir
        .join("runtime")
        .join("models")
        .join("htdemucs.onnx");

    let error = bootstrap::install_verified_model_bytes(&destination, b"fake-model", "not-a-sha")
        .expect_err("checksum mismatch should fail");

    assert!(error.to_string().contains("checksum mismatch"));
    assert!(!destination.exists());

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn descriptors_resolve_from_the_embedded_catalog_snapshot() {
    // Descriptors must be derived from the pinned catalog snapshot — the
    // same contract fixture scripts/resolve-model.mjs, setup.sh, and CI
    // consume — never from hand-maintained constants.
    let catalog = openkara_lib::separator::catalog::embedded_catalog();

    for variant in [ModelVariant::Htdemucs, ModelVariant::HtdemucsFt] {
        let descriptor = bootstrap::descriptor_for(variant);
        let catalog_model =
            openkara_lib::separator::catalog::resolve_model(&catalog.manifest, variant)
                .expect("embedded catalog must resolve every variant");

        assert_eq!(descriptor.download_url, catalog_model.download_url);
        assert_eq!(descriptor.sha256, catalog_model.archive_digest);
        assert_eq!(descriptor.filename, catalog_model.filename);
        assert_eq!(descriptor.byte_size, catalog_model.byte_size);
        assert_eq!(descriptor.artifact_id, catalog_model.artifact_id);
        assert_eq!(descriptor.upstream_tag, catalog_model.upstream.tag);
        assert!(
            descriptor
                .download_url
                .starts_with("https://github.com/thedavidweng/openkara-models/releases/download/"),
            "model artifacts must come from openkara-models releases"
        );
        assert_eq!(descriptor.identity.generation, catalog.generation);
        assert_eq!(descriptor.identity.release_id, catalog.release_id);
    }
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

#[test]
fn startup_bootstrap_keeps_verified_managed_model_ready_without_spawning_worker() {
    let temp_dir = unique_temp_dir();
    let managed_path = bootstrap::managed_model_path(&temp_dir);
    let development_path = temp_dir.join("dev").join("htdemucs.onnx");
    let managed_bytes = b"managed-model";

    write_file(&managed_path, managed_bytes);

    let startup = derive_startup_model_bootstrap(
        &temp_dir,
        &development_path,
        ModelVariant::Htdemucs,
        &sha256_hex(managed_bytes),
    )
    .expect("startup bootstrap should resolve verified managed model");

    assert_eq!(startup.model_path, managed_path);
    assert_eq!(
        startup.status.state,
        commands::bootstrap::ModelBootstrapState::Ready
    );
    assert_eq!(
        startup.status.model_path,
        managed_path.display().to_string()
    );
    assert!(
        !startup.should_spawn_bootstrap_worker,
        "verified managed installs should not re-trigger bootstrap on startup"
    );

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn startup_bootstrap_uses_existing_verified_manifest_for_managed_model() {
    let temp_dir = unique_temp_dir();
    let managed_path = bootstrap::managed_model_path(&temp_dir);
    let development_path = temp_dir.join("dev").join("htdemucs.onnx");
    let managed_bytes = b"managed-model";

    bootstrap::install_verified_model_bytes(
        &managed_path,
        managed_bytes,
        &sha256_hex(managed_bytes),
    )
    .expect("verified model should install with manifest");

    let startup = derive_startup_model_bootstrap(
        &temp_dir,
        &development_path,
        ModelVariant::Htdemucs,
        &sha256_hex(managed_bytes),
    )
    .expect("startup bootstrap should trust the matching manifest");

    assert_eq!(startup.model_path, managed_path);
    assert_eq!(
        startup.status.state,
        commands::bootstrap::ModelBootstrapState::Ready
    );

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn startup_bootstrap_detects_legacy_managed_model_without_spawning_worker() {
    let temp_dir = unique_temp_dir();
    let managed_path = bootstrap::managed_model_path(&temp_dir);
    let development_path = temp_dir.join("dev").join("htdemucs.onnx");
    let wrong_bytes = b"legacy-model-bytes";

    write_file(&managed_path, wrong_bytes);

    let startup = derive_startup_model_bootstrap(
        &temp_dir,
        &development_path,
        ModelVariant::Htdemucs,
        &sha256_hex(b"pinned-expected-bytes"),
    )
    .expect("startup bootstrap should classify legacy managed install");

    assert_eq!(
        startup.status.state,
        commands::bootstrap::ModelBootstrapState::Outdated
    );
    assert!(!startup.should_spawn_bootstrap_worker);
    assert!(
        managed_path.exists(),
        "legacy file should remain for user deletion"
    );

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn ensure_model_ready_rejects_outdated_install() {
    let statuses = Arc::new(Mutex::new(commands::bootstrap::outdated_status(
        "/tmp/openkara-model.onnx",
    )));

    let error = commands::bootstrap::ensure_model_ready(&statuses)
        .expect_err("outdated install should block separation");

    assert_eq!(error.code, ErrorCode::ModelUnavailable);
}

#[test]
fn delete_model_file_removes_verification_manifest() {
    let temp_dir = unique_temp_dir();
    let managed_path = bootstrap::managed_model_path(&temp_dir);
    let payload = b"fake-model";

    bootstrap::install_verified_model_bytes(&managed_path, payload, &sha256_hex(payload))
        .expect("verified model should install with manifest");
    let manifest_path = managed_path.with_file_name("htdemucs.onnx.verified.json");
    assert!(manifest_path.exists());

    bootstrap::delete_model_file(&temp_dir, ModelVariant::Htdemucs)
        .expect("model deletion should remove model and manifest");

    assert!(!managed_path.exists());
    assert!(!manifest_path.exists());

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn startup_bootstrap_uses_active_variant_descriptor_for_managed_model_resolution() {
    let temp_dir = unique_temp_dir();
    let descriptor = bootstrap::descriptor_for(ModelVariant::HtdemucsFt);
    let managed_path = bootstrap::managed_model_path_for(&temp_dir, descriptor);
    let development_path = temp_dir.join("dev").join("htdemucs_ft.onnx");
    let managed_bytes = b"managed-model-ft";

    write_file(&managed_path, managed_bytes);

    let startup = derive_startup_model_bootstrap(
        &temp_dir,
        &development_path,
        ModelVariant::HtdemucsFt,
        &sha256_hex(managed_bytes),
    )
    .expect("startup bootstrap should resolve managed model for active variant");

    assert_eq!(startup.managed_model_path, managed_path);
    assert_eq!(startup.model_path, managed_path);
    assert_eq!(
        startup.status.state,
        commands::bootstrap::ModelBootstrapState::Ready
    );
    assert_eq!(
        startup.status.model_path,
        managed_path.display().to_string()
    );

    remove_dir_if_exists(&temp_dir);
}
