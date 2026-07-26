//! Integration tests for the catalog-driven model architecture (issue #167):
//! identity records, update comparison against installed state, offline
//! resilience, and downgrade rejection at the resolution layer.

use std::{fs, path::Path, path::PathBuf};

mod support;

use openkara_lib::{
    config::{ModelVariant, StemMode},
    separator::bootstrap,
    separator::catalog::{
        self, compare_installed_model, embedded_catalog, read_installed_identity, resolve_model,
        ModelUpdateState,
    },
};

fn unique_temp_dir() -> PathBuf {
    support::unique_temp_path("phase6-catalog")
}

fn remove_dir_if_exists(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).expect("temporary directory should be removable");
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    openkara_lib::separator::verified_manifest::sha256_hex(bytes)
}

#[test]
fn stem_mode_defaults_to_four_stem() {
    // Product decision #182: the model natively outputs four stems and
    // two-stem output costs the same inference time, so four-stem is the
    // default for new separations. Catalog consumption must never change
    // the user's stem mode decision.
    assert_eq!(StemMode::default(), StemMode::FourStem);
}

#[test]
fn install_records_identity_and_delete_removes_it() {
    let temp_dir = unique_temp_dir();
    let descriptor = bootstrap::descriptor_for(ModelVariant::Htdemucs);
    let managed_path = bootstrap::managed_model_path_for(&temp_dir, descriptor);
    let payload = b"fake-model-payload";

    support::install_verified_model_bytes(&managed_path, payload, &sha256_hex(payload))
        .expect("verified payload should install");
    let mut identity = descriptor.identity.clone();
    identity.archive_sha256 = sha256_hex(payload);
    identity.archive_size = payload.len() as u64;
    catalog::write_installed_identity(&managed_path, &identity).expect("identity should persist");

    let read_back = read_installed_identity(&managed_path).expect("identity should read back");
    assert_eq!(read_back.artifact_id, descriptor.artifact_id);
    assert_eq!(read_back.upstream_version, descriptor.upstream_tag);

    bootstrap::delete_model_file(&temp_dir, ModelVariant::Htdemucs)
        .expect("model deletion should succeed");
    assert!(!managed_path.exists());
    assert!(
        read_installed_identity(&managed_path).is_none(),
        "delete must remove the identity record"
    );

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn identity_verified_model_stays_ready_when_pin_moves() {
    // A model installed from a newer verified catalog generation must remain
    // usable when the embedded pin (a stale app binary, or an offline catalog
    // refresh) does not know its digest.
    let temp_dir = unique_temp_dir();
    let descriptor = bootstrap::descriptor_for(ModelVariant::Htdemucs);
    let managed_path = bootstrap::managed_model_path_for(&temp_dir, descriptor);
    let newer_payload = b"newer-generation-model";
    let newer_sha = sha256_hex(newer_payload);

    support::install_verified_model_bytes(&managed_path, newer_payload, &newer_sha)
        .expect("verified payload should install");
    let mut identity = descriptor.identity.clone();
    identity.generation += 1;
    identity.release_id = "2099-01-01-001".to_owned();
    identity.artifact_id = "htdemucs.balanced.fp32.newer".to_owned();
    identity.upstream_version = "model-v9.9.9".to_owned();
    identity.archive_sha256 = newer_sha;
    identity.archive_size = newer_payload.len() as u64;
    catalog::write_installed_identity(&managed_path, &identity).expect("identity should persist");

    // The embedded pin's digest does NOT match this file, but the identity
    // record proves it is a verified install.
    let resolution = bootstrap::resolve_model_installation(
        &managed_path,
        &temp_dir.join("__no_dev_fallback__"),
        &descriptor.file_sha256,
    )
    .expect("resolution should succeed");

    match resolution {
        bootstrap::ModelInstallationResolution::Ready(resolved) => {
            assert_eq!(resolved.path, managed_path);
        }
        other => panic!("identity-verified install must stay ready, got {other:?}"),
    }

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn tampered_file_with_identity_record_is_not_ready() {
    // The identity record alone proves nothing: the file digest must match
    // the digest the record captured at install time.
    let temp_dir = unique_temp_dir();
    let descriptor = bootstrap::descriptor_for(ModelVariant::Htdemucs);
    let managed_path = bootstrap::managed_model_path_for(&temp_dir, descriptor);

    fs::create_dir_all(managed_path.parent().expect("parent")).expect("create models dir");
    fs::write(&managed_path, b"tampered-bytes").expect("write model file");
    let mut identity = descriptor.identity.clone();
    identity.archive_sha256 = sha256_hex(b"the-original-bytes");
    catalog::write_installed_identity(&managed_path, &identity).expect("identity should persist");

    let resolution = bootstrap::resolve_model_installation(
        &managed_path,
        &temp_dir.join("__no_dev_fallback__"),
        &descriptor.file_sha256,
    )
    .expect("resolution should succeed");

    assert!(
        matches!(
            resolution,
            bootstrap::ModelInstallationResolution::LegacyManaged(_)
        ),
        "a file matching neither the pin nor its identity is a legacy install"
    );

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn corrupt_identity_record_falls_back_to_pin_verification() {
    let temp_dir = unique_temp_dir();
    let descriptor = bootstrap::descriptor_for(ModelVariant::Htdemucs);
    let managed_path = bootstrap::managed_model_path_for(&temp_dir, descriptor);
    let payload = b"some-model-bytes";

    fs::create_dir_all(managed_path.parent().expect("parent")).expect("create models dir");
    fs::write(&managed_path, payload).expect("write model file");
    let identity_path = catalog::installed_identity_path(&managed_path).expect("identity path");
    fs::write(&identity_path, b"{corrupt json").expect("write corrupt identity");

    // Pin matches the file: corrupt identity must not block readiness.
    let resolution = bootstrap::resolve_model_installation(
        &managed_path,
        &temp_dir.join("__no_dev_fallback__"),
        &sha256_hex(payload),
    )
    .expect("resolution should succeed");
    assert!(matches!(
        resolution,
        bootstrap::ModelInstallationResolution::Ready(_)
    ));

    // Pin does not match and the identity is corrupt: legacy install.
    let resolution = bootstrap::resolve_model_installation(
        &managed_path,
        &temp_dir.join("__no_dev_fallback__"),
        &sha256_hex(b"other-bytes"),
    )
    .expect("resolution should succeed");
    assert!(matches!(
        resolution,
        bootstrap::ModelInstallationResolution::LegacyManaged(_)
    ));

    remove_dir_if_exists(&temp_dir);
}

#[test]
fn update_comparison_covers_install_update_and_downgrade() {
    let catalog = embedded_catalog();
    let model = resolve_model(&catalog.manifest, ModelVariant::Htdemucs).expect("model");

    // Not installed.
    let comparison =
        compare_installed_model(None, model, catalog, false).expect("comparison should succeed");
    assert_eq!(comparison.state, ModelUpdateState::NotInstalled);

    // Same artifact installed: up to date.
    let identity = catalog::identity_from_catalog_model(model, catalog);
    let comparison = compare_installed_model(Some(identity.clone()), model, catalog, true)
        .expect("comparison should succeed");
    assert_eq!(comparison.state, ModelUpdateState::UpToDate);

    // Older artifact installed: update available.
    let mut older = identity.clone();
    older.generation = catalog.generation.saturating_sub(1).max(1);
    older.artifact_id = "htdemucs.balanced.fp32.older".to_owned();
    older.archive_sha256 = "2".repeat(64);
    let comparison = compare_installed_model(Some(older), model, catalog, true)
        .expect("comparison should succeed");
    assert_eq!(comparison.state, ModelUpdateState::UpdateAvailable);

    // Newer artifact installed than the catalog offers: implicit downgrade is
    // rejected instead of being reported as an update.
    let mut newer = identity;
    newer.generation = catalog.generation + 1;
    newer.archive_sha256 = "3".repeat(64);
    let error = compare_installed_model(Some(newer), model, catalog, true)
        .expect_err("downgrade must be rejected");
    assert!(error.to_string().contains("refusing implicit downgrade"));
}

#[test]
fn embedded_catalog_resolves_offline_for_both_variants() {
    // The embedded snapshot is the offline trust anchor: full resolution must
    // work with no network and no configuration.
    for variant in [ModelVariant::Htdemucs, ModelVariant::HtdemucsFt] {
        let descriptor = bootstrap::descriptor_for(variant);
        assert!(!descriptor.download_url.is_empty());
        assert_eq!(descriptor.file_sha256.len(), 64);
        assert!(descriptor.download_size > 0);
        assert!(!descriptor.identity.compatible_ids.is_empty());
    }
}
