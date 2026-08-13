//! Catalog-driven ONNX Runtime installation with staged activation.
//!
//! Runtimes install into immutable per-artifact directories under
//! `<app_data>/runtimes/<artifact_id>/`, each carrying the shared installed
//! artifact record (`record.json`). A small slot file (`slots.json`) names
//! the active, candidate, and previous artifacts:
//!
//! - **active** — the runtime the app loads at startup.
//! - **candidate** — a verified update staged for activation on the next
//!   launch, upholding the never-replaced-in-place invariant: a runtime
//!   loaded by the current process is never overwritten under its own path.
//! - **previous** — the last verified generation, kept for explicit
//!   recovery when a candidate fails to activate.
//!
//! Activation is transactional: the slot swap is persisted with an
//! `activation_pending` marker before the dynamic library is loaded, so a
//! crash mid-activation is detected on the next launch and rolled back to
//! the previous verified runtime.
//!
//! A pre-slot legacy install (`<app_data>/runtime/<library>`) is still
//! loadable so existing users keep working audio until the first catalog
//! runtime is installed; it is deleted once a slot runtime becomes active.

use crate::separator::artifacts;
use crate::separator::catalog::{read_artifact_record, InstalledArtifactRecord};
use crate::separator::verified_manifest::{
    verified_manifest_matches, verified_manifest_path, VerifiedManifest,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub use super::activation::ORT_RUNTIME_FILENAME;

pub const RUNTIME_SLOTS_SCHEMA_VERSION: &str = "openkara.app/runtime-slots-v1";
pub const RUNTIME_RECORD_FILENAME: &str = "record.json";

const RUNTIMES_DIR_NAME: &str = "runtimes";
const LEGACY_RUNTIME_DIR_NAME: &str = "runtime";
const SLOTS_FILENAME: &str = "slots.json";

// ---------------------------------------------------------------------------
// Disk layout
// ---------------------------------------------------------------------------

pub fn runtimes_root(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(RUNTIMES_DIR_NAME)
}

pub fn runtime_artifact_dir(app_data_dir: &Path, artifact_id: &str) -> PathBuf {
    runtimes_root(app_data_dir).join(artifact_id)
}

fn slots_path(app_data_dir: &Path) -> PathBuf {
    runtimes_root(app_data_dir).join(SLOTS_FILENAME)
}

pub fn legacy_runtime_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir
        .join(LEGACY_RUNTIME_DIR_NAME)
        .join(ORT_RUNTIME_FILENAME)
}

// ---------------------------------------------------------------------------
// Slots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivationFailure {
    pub artifact_id: String,
    pub error: String,
    pub at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSlots {
    pub schema: String,
    pub active: Option<String>,
    pub candidate: Option<String>,
    pub previous: Option<String>,
    /// True between persisting a candidate promotion and proving the
    /// promoted library loads. A pending marker found at startup means the
    /// previous launch did not persist a success acknowledgement.
    pub activation_pending: bool,
    /// Number of launches that attempted to prove the pending runtime.
    /// A pending marker with attempts left is retried (a successful load
    /// whose acknowledgement failed to persist must not lose the runtime);
    /// exhausted attempts mean the load itself is crashing and the
    /// activation rolls back.
    #[serde(default)]
    pub activation_attempts: u32,
    pub last_failure: Option<ActivationFailure>,
}

/// One promotion launch plus one retry launch. A load that crashes the
/// process twice in a row is treated as failed.
const MAX_ACTIVATION_ATTEMPTS: u32 = 2;

impl Default for RuntimeSlots {
    fn default() -> Self {
        Self {
            schema: RUNTIME_SLOTS_SCHEMA_VERSION.to_owned(),
            active: None,
            candidate: None,
            previous: None,
            activation_pending: false,
            activation_attempts: 0,
            last_failure: None,
        }
    }
}

pub fn read_slots(app_data_dir: &Path) -> RuntimeSlots {
    let path = slots_path(app_data_dir);
    let Ok(contents) = fs::read_to_string(&path) else {
        return RuntimeSlots::default();
    };
    match serde_json::from_str::<RuntimeSlots>(&contents) {
        Ok(slots) if slots.schema == RUNTIME_SLOTS_SCHEMA_VERSION => slots,
        // Corrupt/unknown slot file: fall back to defaults; inventory rebuilds.
        _ => RuntimeSlots::default(),
    }
}

pub fn write_slots(app_data_dir: &Path, slots: &RuntimeSlots) -> Result<()> {
    let path = slots_path(app_data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(slots).context("failed to serialize runtime slots")?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json)
        .with_context(|| format!("failed to write runtime slots {}", temp.display()))?;
    fs::rename(&temp, &path)
        .with_context(|| format!("failed to promote runtime slots {}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Installed runtimes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct InstalledRuntime {
    pub record: InstalledArtifactRecord,
    pub dir: PathBuf,
    pub library_path: PathBuf,
}

/// Read the installed runtime for an artifact id, when its record is valid
/// and the main library file exists. File digests are NOT re-verified here —
/// call `verify_runtime_files` before trusting it for activation.
pub fn installed_runtime(app_data_dir: &Path, artifact_id: &str) -> Option<InstalledRuntime> {
    let dir = runtime_artifact_dir(app_data_dir, artifact_id);
    let record = read_artifact_record(&dir.join(RUNTIME_RECORD_FILENAME))?;
    if record.kind != "runtime" || record.artifact_id != artifact_id {
        return None;
    }
    let library_path = dir.join(ORT_RUNTIME_FILENAME);
    library_path.is_file().then_some(InstalledRuntime {
        record,
        dir,
        library_path,
    })
}

/// Verify every file the record declares by size and streaming SHA-256.
pub fn verify_runtime_files(runtime: &InstalledRuntime) -> Result<bool> {
    for file in &runtime.record.files {
        let path = runtime.dir.join(&file.path);
        let Ok(metadata) = fs::metadata(&path) else {
            return Ok(false);
        };
        if metadata.len() != file.size {
            return Ok(false);
        }
        if artifacts::sha256_file(&path)? != file.sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Legacy (pre-slot) install
// ---------------------------------------------------------------------------

/// A legacy install is trusted when its verification sidecar's recorded
/// digest still matches the file. The sidecar digest is the identity the
/// old app verified at install time; the pinned constants it was checked
/// against no longer exist.
pub fn legacy_runtime_ready(app_data_dir: &Path) -> Option<PathBuf> {
    let path = legacy_runtime_path(app_data_dir);
    if !path.is_file() {
        return None;
    }
    let manifest_path = verified_manifest_path(&path).ok()?;
    let contents = fs::read_to_string(&manifest_path).ok()?;
    let manifest: VerifiedManifest = serde_json::from_str(&contents).ok()?;
    if verified_manifest_matches(&path, &manifest.sha256).ok()? {
        return Some(path);
    }
    let actual = artifacts::sha256_file(&path).ok()?;
    (actual == manifest.sha256).then_some(path)
}

pub fn delete_legacy_runtime(app_data_dir: &Path) -> Result<()> {
    let dir = app_data_dir.join(LEGACY_RUNTIME_DIR_NAME);
    if dir.exists() {
        fs::remove_dir_all(&dir)
            .with_context(|| format!("failed to delete legacy runtime {}", dir.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Startup activation transaction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StartupLoadPlan {
    pub library_path: PathBuf,
    pub record: Option<InstalledArtifactRecord>,
    /// True when this load is proving a freshly promoted candidate. The
    /// caller must report the load result through
    /// `finish_activation_success` or `rollback_failed_activation`.
    pub proving_candidate: bool,
    pub is_legacy: bool,
}

/// Prepare the runtime to load at startup. Promotes a valid candidate to
/// active (persisting the `activation_pending` marker first), rolls back an
/// interrupted activation from a previous crash, and falls back to the
/// legacy install when no slot runtime exists.
pub fn begin_startup(app_data_dir: &Path) -> Result<Option<StartupLoadPlan>> {
    let mut slots = read_slots(app_data_dir);

    if slots.activation_pending {
        // Pending activation is ambiguous: retry while attempts remain, then roll back.
        let pending_id = slots.active.clone();
        let retryable = pending_id
            .as_deref()
            .and_then(|id| installed_runtime(app_data_dir, id))
            .filter(|runtime| verify_runtime_files(runtime).unwrap_or(false))
            .filter(|_| slots.activation_attempts < MAX_ACTIVATION_ATTEMPTS);

        match retryable {
            Some(pending) => {
                slots.activation_attempts += 1;
                write_slots(app_data_dir, &slots)?;
                return Ok(Some(StartupLoadPlan {
                    library_path: pending.library_path,
                    record: Some(pending.record),
                    proving_candidate: true,
                    is_legacy: false,
                }));
            }
            None => {
                let failed_id = slots.active.take();
                slots.active = slots.previous.take();
                slots.activation_pending = false;
                slots.activation_attempts = 0;
                if let Some(failed_id) = failed_id {
                    slots.last_failure = Some(ActivationFailure {
                        artifact_id: failed_id.clone(),
                        error: "runtime activation kept failing and was rolled back".to_owned(),
                        at_unix: unix_now(),
                    });
                    let _ = fs::remove_dir_all(runtime_artifact_dir(app_data_dir, &failed_id));
                }
                write_slots(app_data_dir, &slots)?;
            }
        }
    }

    if let Some(candidate_id) = slots.candidate.clone() {
        match installed_runtime(app_data_dir, &candidate_id) {
            Some(candidate) if verify_runtime_files(&candidate)? => {
                // Persist pending marker before load so a crash rolls back next launch.
                let old_active = slots.active.take();
                slots.previous = old_active;
                slots.active = Some(candidate_id);
                slots.candidate = None;
                slots.activation_pending = true;
                slots.activation_attempts = 1;
                write_slots(app_data_dir, &slots)?;
                return Ok(Some(StartupLoadPlan {
                    library_path: candidate.library_path,
                    record: Some(candidate.record),
                    proving_candidate: true,
                    is_legacy: false,
                }));
            }
            _ => {
                slots.candidate = None;
                slots.last_failure = Some(ActivationFailure {
                    artifact_id: candidate_id.clone(),
                    error: "staged runtime candidate failed verification".to_owned(),
                    at_unix: unix_now(),
                });
                let _ = fs::remove_dir_all(runtime_artifact_dir(app_data_dir, &candidate_id));
                write_slots(app_data_dir, &slots)?;
            }
        }
    }

    if let Some(active_id) = slots.active.clone() {
        match installed_runtime(app_data_dir, &active_id) {
            Some(active) if verify_runtime_files(&active)? => {
                return Ok(Some(StartupLoadPlan {
                    library_path: active.library_path,
                    record: Some(active.record),
                    proving_candidate: false,
                    is_legacy: false,
                }));
            }
            _ => {
                // Corrupt active install: surface as missing; never load unverified bytes.
                let _ = fs::remove_dir_all(runtime_artifact_dir(app_data_dir, &active_id));
                slots.active = None;
                slots.last_failure = Some(ActivationFailure {
                    artifact_id: active_id,
                    error: "active runtime failed verification".to_owned(),
                    at_unix: unix_now(),
                });
                write_slots(app_data_dir, &slots)?;
            }
        }
    }

    if let Some(legacy_path) = legacy_runtime_ready(app_data_dir) {
        return Ok(Some(StartupLoadPlan {
            library_path: legacy_path,
            record: None,
            proving_candidate: false,
            is_legacy: true,
        }));
    }

    prune_unreferenced_runtimes(app_data_dir, &slots);

    Ok(None)
}

/// Clear the pending marker after a promoted candidate loaded successfully,
/// dropping any generation older than `previous` and the legacy install.
pub fn finish_activation_success(app_data_dir: &Path) -> Result<()> {
    let mut slots = read_slots(app_data_dir);
    slots.activation_pending = false;
    slots.activation_attempts = 0;
    slots.last_failure = None;
    write_slots(app_data_dir, &slots)?;
    prune_unreferenced_runtimes(app_data_dir, &slots);
    let _ = delete_legacy_runtime(app_data_dir);
    Ok(())
}

/// Roll back a candidate whose dynamic load failed: restore the previous
/// verified runtime as active, record the failure, and delete the failed
/// install. Returns the restored runtime, when one exists.
pub fn rollback_failed_activation(
    app_data_dir: &Path,
    failed_artifact_id: &str,
    error: &str,
) -> Result<Option<InstalledRuntime>> {
    let mut slots = read_slots(app_data_dir);
    slots.active = slots.previous.take();
    slots.activation_pending = false;
    slots.activation_attempts = 0;
    slots.last_failure = Some(ActivationFailure {
        artifact_id: failed_artifact_id.to_owned(),
        error: error.to_owned(),
        at_unix: unix_now(),
    });
    write_slots(app_data_dir, &slots)?;
    let _ = fs::remove_dir_all(runtime_artifact_dir(app_data_dir, failed_artifact_id));

    let Some(active_id) = slots.active else {
        return Ok(None);
    };
    let restored = installed_runtime(app_data_dir, &active_id)
        .filter(|runtime| verify_runtime_files(runtime).unwrap_or(false));
    Ok(restored)
}

/// Record a runtime as active without a restart. Only valid when no runtime
/// is loaded in the current process (first install).
pub fn activate_first_install(app_data_dir: &Path, artifact_id: &str) -> Result<()> {
    let mut slots = read_slots(app_data_dir);
    slots.previous = slots.active.take();
    slots.active = Some(artifact_id.to_owned());
    slots.candidate = None;
    slots.activation_pending = false;
    slots.activation_attempts = 0;
    slots.last_failure = None;
    write_slots(app_data_dir, &slots)?;
    prune_unreferenced_runtimes(app_data_dir, &slots);
    let _ = delete_legacy_runtime(app_data_dir);
    Ok(())
}

/// Stage a verified install as the next-launch candidate.
pub fn stage_candidate(app_data_dir: &Path, artifact_id: &str) -> Result<()> {
    let mut slots = read_slots(app_data_dir);
    slots.candidate = Some(artifact_id.to_owned());
    write_slots(app_data_dir, &slots)?;
    Ok(())
}

/// Remove artifact directories not referenced by any slot. Keeps disk usage
/// bounded to active + candidate + one previous generation.
fn prune_unreferenced_runtimes(app_data_dir: &Path, slots: &RuntimeSlots) {
    let root = runtimes_root(app_data_dir);
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    let referenced: Vec<&String> = [&slots.active, &slots.candidate, &slots.previous]
        .into_iter()
        .flatten()
        .collect();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !referenced.iter().any(|id| id.as_str() == name) {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

// ---------------------------------------------------------------------------
// Inventory (facts for the command-layer state machine)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RuntimeInventory {
    pub active: Option<InstalledRuntime>,
    pub candidate: Option<InstalledRuntime>,
    pub legacy_path: Option<PathBuf>,
    pub last_failure: Option<ActivationFailure>,
}

pub fn runtime_inventory(app_data_dir: &Path) -> RuntimeInventory {
    let slots = read_slots(app_data_dir);
    let active = slots
        .active
        .as_deref()
        .and_then(|id| installed_runtime(app_data_dir, id));
    let candidate = slots
        .candidate
        .as_deref()
        .and_then(|id| installed_runtime(app_data_dir, id));
    RuntimeInventory {
        active,
        candidate,
        legacy_path: legacy_runtime_ready(app_data_dir),
        last_failure: slots.last_failure,
    }
}

/// True when a runtime is present that separation can load (active slot or
/// legacy install). Used by flows that only need a yes/no readiness fact.
pub fn is_runtime_available(app_data_dir: &Path) -> bool {
    let inventory = runtime_inventory(app_data_dir);
    inventory.active.is_some() || inventory.legacy_path.is_some()
}

/// Delete every managed runtime install (slots, artifact directories, and
/// the legacy directory). The caller is responsible for confirming no
/// runtime is required by in-flight work.
pub fn delete_runtime(app_data_dir: &Path) -> Result<()> {
    let root = runtimes_root(app_data_dir);
    if root.exists() {
        // Clear slots first; Windows may keep a mapped library until restart.
        write_slots(app_data_dir, &RuntimeSlots::default())?;
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let _ = fs::remove_dir_all(&path);
                }
            }
        }
        let _ = fs::remove_file(slots_path(app_data_dir));
        let _ = fs::remove_dir(&root);
    }
    delete_legacy_runtime(app_data_dir)?;
    Ok(())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::separator::catalog::{
        self, embedded_catalog, record_from_catalog_runtime, resolve_runtime,
        write_artifact_record, CatalogRuntime, VerifiedCatalog,
    };
    use crate::separator::verified_manifest::{sha256_hex, write_verified_manifest};

    fn catalog_runtime() -> (&'static VerifiedCatalog, &'static CatalogRuntime) {
        let catalog = embedded_catalog();
        let runtime = resolve_runtime(
            &catalog.manifest,
            catalog::current_target_triple(),
            crate::config::ExecutionProviderPreference::default_for_current_platform(),
        )
        .expect("embedded catalog must resolve the current target runtime");
        (catalog, runtime)
    }

    /// Write a fake installed runtime whose record digests match the files.
    fn write_fake_install(app_data: &Path, artifact_id: &str, library_bytes: &[u8]) {
        let (catalog, runtime) = catalog_runtime();
        let dir = runtime_artifact_dir(app_data, artifact_id);
        fs::create_dir_all(&dir).expect("create artifact dir");
        fs::write(dir.join(ORT_RUNTIME_FILENAME), library_bytes).expect("write library");

        let mut record = record_from_catalog_runtime(runtime, catalog);
        record.artifact_id = artifact_id.to_owned();
        record.files = vec![crate::separator::catalog::InstalledFileRecord {
            path: ORT_RUNTIME_FILENAME.to_owned(),
            size: library_bytes.len() as u64,
            sha256: sha256_hex(library_bytes),
        }];
        write_artifact_record(&dir.join(RUNTIME_RECORD_FILENAME), &record).expect("write record");
    }

    #[test]
    fn slots_round_trip_and_default_on_corruption() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let slots = RuntimeSlots {
            active: Some("rt-a".to_owned()),
            candidate: Some("rt-b".to_owned()),
            ..RuntimeSlots::default()
        };
        write_slots(tmp.path(), &slots).expect("write slots");
        assert_eq!(read_slots(tmp.path()), slots);

        fs::write(slots_path(tmp.path()), b"{corrupt").expect("corrupt slots");
        assert_eq!(read_slots(tmp.path()), RuntimeSlots::default());
    }

    #[test]
    fn begin_startup_with_nothing_installed_returns_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(begin_startup(tmp.path()).expect("startup").is_none());
    }

    #[test]
    fn valid_candidate_is_promoted_with_pending_marker() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_fake_install(tmp.path(), "rt-old", b"old-runtime");
        write_fake_install(tmp.path(), "rt-new", b"new-runtime");
        let slots = RuntimeSlots {
            active: Some("rt-old".to_owned()),
            candidate: Some("rt-new".to_owned()),
            ..RuntimeSlots::default()
        };
        write_slots(tmp.path(), &slots).expect("write slots");

        let plan = begin_startup(tmp.path())
            .expect("startup")
            .expect("plan should load the promoted candidate");
        assert!(plan.proving_candidate);
        assert_eq!(
            plan.record.as_ref().map(|r| r.artifact_id.as_str()),
            Some("rt-new")
        );

        let slots = read_slots(tmp.path());
        assert_eq!(slots.active.as_deref(), Some("rt-new"));
        assert_eq!(slots.previous.as_deref(), Some("rt-old"));
        assert_eq!(slots.candidate, None);
        assert!(slots.activation_pending);

        finish_activation_success(tmp.path()).expect("finish");
        let slots = read_slots(tmp.path());
        assert!(!slots.activation_pending);
        assert!(slots.last_failure.is_none());
    }

    #[test]
    fn corrupt_candidate_is_dropped_and_recorded() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_fake_install(tmp.path(), "rt-old", b"old-runtime");
        write_fake_install(tmp.path(), "rt-bad", b"bad-runtime");
        // Corrupt the staged candidate after recording.
        fs::write(
            runtime_artifact_dir(tmp.path(), "rt-bad").join(ORT_RUNTIME_FILENAME),
            b"tampered",
        )
        .expect("tamper");
        let slots = RuntimeSlots {
            active: Some("rt-old".to_owned()),
            candidate: Some("rt-bad".to_owned()),
            ..RuntimeSlots::default()
        };
        write_slots(tmp.path(), &slots).expect("write slots");

        let plan = begin_startup(tmp.path())
            .expect("startup")
            .expect("plan should fall back to the active runtime");
        assert!(!plan.proving_candidate);
        assert_eq!(
            plan.record.as_ref().map(|r| r.artifact_id.as_str()),
            Some("rt-old")
        );

        let slots = read_slots(tmp.path());
        assert_eq!(slots.active.as_deref(), Some("rt-old"));
        assert_eq!(slots.candidate, None);
        assert_eq!(
            slots.last_failure.as_ref().map(|f| f.artifact_id.as_str()),
            Some("rt-bad")
        );
        assert!(!runtime_artifact_dir(tmp.path(), "rt-bad").exists());
    }

    #[test]
    fn interrupted_activation_retries_the_pending_runtime_first() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_fake_install(tmp.path(), "rt-old", b"old-runtime");
        write_fake_install(tmp.path(), "rt-new", b"new-runtime");
        // A pending marker after a crash is ambiguous: the load may have
        // succeeded and only the acknowledgement was lost. With attempts
        // remaining, the pending runtime is retried — never deleted.
        let slots = RuntimeSlots {
            active: Some("rt-new".to_owned()),
            previous: Some("rt-old".to_owned()),
            activation_pending: true,
            activation_attempts: 1,
            ..RuntimeSlots::default()
        };
        write_slots(tmp.path(), &slots).expect("write slots");

        let plan = begin_startup(tmp.path())
            .expect("startup")
            .expect("plan should retry the pending runtime");
        assert!(plan.proving_candidate);
        assert_eq!(
            plan.record.as_ref().map(|r| r.artifact_id.as_str()),
            Some("rt-new")
        );
        assert!(runtime_artifact_dir(tmp.path(), "rt-new").exists());

        let slots = read_slots(tmp.path());
        assert_eq!(slots.activation_attempts, 2);
        assert!(slots.activation_pending);

        // A successful load acknowledges and resets the budget.
        finish_activation_success(tmp.path()).expect("finish");
        let slots = read_slots(tmp.path());
        assert!(!slots.activation_pending);
        assert_eq!(slots.activation_attempts, 0);
        assert_eq!(slots.active.as_deref(), Some("rt-new"));
    }

    #[test]
    fn exhausted_activation_attempts_roll_back_to_previous() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_fake_install(tmp.path(), "rt-old", b"old-runtime");
        write_fake_install(tmp.path(), "rt-new", b"new-runtime");
        // The attempt budget is spent: the load itself keeps crashing.
        let slots = RuntimeSlots {
            active: Some("rt-new".to_owned()),
            previous: Some("rt-old".to_owned()),
            activation_pending: true,
            activation_attempts: 2,
            ..RuntimeSlots::default()
        };
        write_slots(tmp.path(), &slots).expect("write slots");

        let plan = begin_startup(tmp.path())
            .expect("startup")
            .expect("plan should restore the previous runtime");
        assert!(!plan.proving_candidate);
        assert_eq!(
            plan.record.as_ref().map(|r| r.artifact_id.as_str()),
            Some("rt-old")
        );

        let slots = read_slots(tmp.path());
        assert_eq!(slots.active.as_deref(), Some("rt-old"));
        assert!(!slots.activation_pending);
        assert_eq!(
            slots.last_failure.as_ref().map(|f| f.artifact_id.as_str()),
            Some("rt-new")
        );
        assert!(!runtime_artifact_dir(tmp.path(), "rt-new").exists());
    }

    #[test]
    fn delete_runtime_clears_slots_even_when_files_linger() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_fake_install(tmp.path(), "rt-a", b"runtime-bytes");
        let slots = RuntimeSlots {
            active: Some("rt-a".to_owned()),
            ..RuntimeSlots::default()
        };
        write_slots(tmp.path(), &slots).expect("write slots");

        delete_runtime(tmp.path()).expect("delete");

        // The slot state is the source of truth: nothing is active anymore
        // regardless of whether every file could be unlinked (Windows keeps
        // a loaded library mapped until restart).
        let slots = read_slots(tmp.path());
        assert_eq!(slots.active, None);
        assert!(!is_runtime_available(tmp.path()));
    }

    #[test]
    fn rollback_failed_activation_restores_previous() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_fake_install(tmp.path(), "rt-old", b"old-runtime");
        write_fake_install(tmp.path(), "rt-new", b"new-runtime");
        let slots = RuntimeSlots {
            active: Some("rt-new".to_owned()),
            previous: Some("rt-old".to_owned()),
            activation_pending: true,
            ..RuntimeSlots::default()
        };
        write_slots(tmp.path(), &slots).expect("write slots");

        let restored = rollback_failed_activation(tmp.path(), "rt-new", "dlopen failed")
            .expect("rollback")
            .expect("previous runtime should be restored");
        assert_eq!(restored.record.artifact_id, "rt-old");
        assert!(!runtime_artifact_dir(tmp.path(), "rt-new").exists());

        let slots = read_slots(tmp.path());
        assert_eq!(slots.active.as_deref(), Some("rt-old"));
        assert_eq!(
            slots.last_failure.as_ref().map(|f| f.error.as_str()),
            Some("dlopen failed")
        );
    }

    #[test]
    fn corrupt_active_install_is_removed_and_reported_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_fake_install(tmp.path(), "rt-a", b"runtime-bytes");
        fs::write(
            runtime_artifact_dir(tmp.path(), "rt-a").join(ORT_RUNTIME_FILENAME),
            b"tampered-bytes",
        )
        .expect("tamper");
        let slots = RuntimeSlots {
            active: Some("rt-a".to_owned()),
            ..RuntimeSlots::default()
        };
        write_slots(tmp.path(), &slots).expect("write slots");

        assert!(begin_startup(tmp.path()).expect("startup").is_none());
        let slots = read_slots(tmp.path());
        assert_eq!(slots.active, None);
        assert!(slots.last_failure.is_some());
    }

    #[test]
    fn legacy_install_is_loadable_until_replaced() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let legacy = legacy_runtime_path(tmp.path());
        fs::create_dir_all(legacy.parent().expect("parent")).expect("legacy dir");
        fs::write(&legacy, b"legacy-runtime").expect("write legacy");
        write_verified_manifest(&legacy, &sha256_hex(b"legacy-runtime")).expect("sidecar");

        let plan = begin_startup(tmp.path())
            .expect("startup")
            .expect("legacy plan");
        assert!(plan.is_legacy);
        assert_eq!(plan.library_path, legacy);

        // Installing and activating a slot runtime deletes the legacy copy.
        write_fake_install(tmp.path(), "rt-new", b"new-runtime");
        activate_first_install(tmp.path(), "rt-new").expect("activate");
        assert!(!legacy.exists());
    }

    #[test]
    fn tampered_legacy_install_is_not_loadable() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let legacy = legacy_runtime_path(tmp.path());
        fs::create_dir_all(legacy.parent().expect("parent")).expect("legacy dir");
        fs::write(&legacy, b"legacy-runtime").expect("write legacy");
        write_verified_manifest(&legacy, &sha256_hex(b"legacy-runtime")).expect("sidecar");
        // Tamper AFTER verification: the metadata fast path misses, the
        // rehash against the sidecar digest catches the modification.
        fs::write(&legacy, b"legacy-runtime-tampered").expect("tamper");

        assert!(legacy_runtime_ready(tmp.path()).is_none());
    }

    #[test]
    fn prune_keeps_only_referenced_generations() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_fake_install(tmp.path(), "rt-a", b"a");
        write_fake_install(tmp.path(), "rt-b", b"b");
        write_fake_install(tmp.path(), "rt-c", b"c");
        let slots = RuntimeSlots {
            active: Some("rt-b".to_owned()),
            previous: Some("rt-a".to_owned()),
            ..RuntimeSlots::default()
        };
        write_slots(tmp.path(), &slots).expect("write slots");

        prune_unreferenced_runtimes(tmp.path(), &slots);

        assert!(runtime_artifact_dir(tmp.path(), "rt-a").exists());
        assert!(runtime_artifact_dir(tmp.path(), "rt-b").exists());
        assert!(!runtime_artifact_dir(tmp.path(), "rt-c").exists());
    }

    #[test]
    fn delete_runtime_removes_everything() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_fake_install(tmp.path(), "rt-a", b"a");
        let legacy = legacy_runtime_path(tmp.path());
        fs::create_dir_all(legacy.parent().expect("parent")).expect("legacy dir");
        fs::write(&legacy, b"legacy").expect("legacy");

        delete_runtime(tmp.path()).expect("delete");

        assert!(!runtimes_root(tmp.path()).exists());
        assert!(!legacy.exists());
        assert!(delete_runtime(tmp.path()).is_ok(), "idempotent");
    }

    #[test]
    fn embedded_catalog_resolves_a_runtime_for_this_target() {
        let (_, runtime) = catalog_runtime();
        assert!(runtime
            .extracted_file_digests
            .contains_key(ORT_RUNTIME_FILENAME));
        assert_eq!(runtime.runtime.ort_c_api_level, "27");
    }
}
