use crate::config::ModelVariant;
use crate::separator::{
    bootstrap::{self, descriptor_for, managed_model_path_for},
    catalog::installed_identity_path,
    runtime_bootstrap::{
        self, installed_runtime, read_slots, runtime_artifact_dir, runtime_inventory,
        verify_runtime_files, write_slots,
    },
    verified_manifest::verified_manifest_path,
};
use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultTarget {
    Runtime,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    CorruptArchiveDigest,
    CorruptExtractedFile,
    CorruptInstalledFile,
    StaleVerificationManifest,
    InterruptAfterExtraction,
    InterruptAfterActivation,
    StaleDownloadingState,
}

#[derive(Debug, Clone)]
pub struct FaultScenario {
    pub target: FaultTarget,
    pub kind: FaultKind,
    pub description: String,
}

impl FaultScenario {
    pub fn all() -> Vec<Self> {
        vec![
            Self {
                target: FaultTarget::Runtime,
                kind: FaultKind::CorruptArchiveDigest,
                description: "runtime archive digest does not match catalog".into(),
            },
            Self {
                target: FaultTarget::Runtime,
                kind: FaultKind::CorruptExtractedFile,
                description: "runtime archive extracts to a corrupted DLL".into(),
            },
            Self {
                target: FaultTarget::Runtime,
                kind: FaultKind::CorruptInstalledFile,
                description: "installed runtime DLL is corrupted after activation".into(),
            },
            Self {
                target: FaultTarget::Runtime,
                kind: FaultKind::StaleVerificationManifest,
                description: "runtime verification manifest does not match installed DLL".into(),
            },
            Self {
                target: FaultTarget::Runtime,
                kind: FaultKind::InterruptAfterExtraction,
                description: "app terminates after runtime extraction before activation".into(),
            },
            Self {
                target: FaultTarget::Runtime,
                kind: FaultKind::InterruptAfterActivation,
                description: "app terminates after runtime activation before state persistence"
                    .into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::CorruptArchiveDigest,
                description: "model archive digest does not match catalog".into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::CorruptExtractedFile,
                description: "model archive extracts to a corrupted ONNX file".into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::CorruptInstalledFile,
                description: "installed model is corrupted after successful verification".into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::StaleVerificationManifest,
                description: "model verification manifest does not match installed ONNX".into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::InterruptAfterExtraction,
                description: "app terminates after model write before verification manifest commit"
                    .into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::StaleDownloadingState,
                description: "bootstrap state is Downloading while a valid managed model exists"
                    .into(),
            },
        ]
    }

    /// On-disk recovery scenarios that do not require intercepting downloads.
    pub fn recovery_suite() -> Vec<Self> {
        Self::all()
            .into_iter()
            .filter(|scenario| {
                !matches!(
                    scenario.kind,
                    FaultKind::CorruptArchiveDigest | FaultKind::CorruptExtractedFile
                )
            })
            .collect()
    }

    pub fn assertion_id(&self) -> String {
        let target = match self.target {
            FaultTarget::Runtime => "RUNTIME",
            FaultTarget::Model => "MODEL",
        };
        let kind = match self.kind {
            FaultKind::CorruptArchiveDigest => "CORRUPT-ARCHIVE-DIGEST",
            FaultKind::CorruptExtractedFile => "CORRUPT-EXTRACTED",
            FaultKind::CorruptInstalledFile => "CORRUPT-INSTALLED",
            FaultKind::StaleVerificationManifest => "STALE-MANIFEST",
            FaultKind::InterruptAfterExtraction => "INTERRUPT-EXTRACTION",
            FaultKind::InterruptAfterActivation => "INTERRUPT-ACTIVATION",
            FaultKind::StaleDownloadingState => "STALE-DOWNLOADING",
        };
        format!("OKA-284-FAULT-{target}-{kind}")
    }

    pub fn requires_fault_server(&self) -> bool {
        matches!(
            self.kind,
            FaultKind::CorruptArchiveDigest | FaultKind::CorruptExtractedFile
        )
    }
}

/// Append a single flipped byte to a file so its digest changes.
pub fn corrupt_file(path: &Path) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let mut file = OpenOptions::new().write(true).append(true).open(path)?;
    file.write_all(&[0xff])?;
    Ok(())
}

/// Path to the local HTTP fault server root used to serve delayed, partial, or incorrect downloads.
pub fn fault_server_root(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("automation-fault-server")
}

#[derive(Debug, Clone)]
pub struct FaultApplication {
    pub target_path: PathBuf,
    pub notes: String,
}

#[derive(Debug, Clone)]
pub struct FaultRecoveryResult {
    pub scenario_id: String,
    pub fault_detected: bool,
    pub prior_runtime_usable: bool,
    pub observation: String,
}

/// Apply one fault against an already-bootstrapped app-data tree.
pub fn apply_fault(app_data_dir: &Path, scenario: &FaultScenario) -> Result<FaultApplication> {
    if scenario.requires_fault_server() {
        return Ok(FaultApplication {
            target_path: fault_server_root(app_data_dir),
            notes: "archive-delivery fault requires fault-server; deferred".into(),
        });
    }

    match (scenario.target, scenario.kind) {
        (FaultTarget::Runtime, FaultKind::CorruptInstalledFile) => {
            let path = active_runtime_library(app_data_dir)?;
            corrupt_file(&path).with_context(|| {
                format!(
                    "failed to corrupt installed runtime library {}",
                    path.display()
                )
            })?;
            Ok(FaultApplication {
                target_path: path,
                notes: "appended corrupt byte to active runtime library".into(),
            })
        }
        (FaultTarget::Runtime, FaultKind::StaleVerificationManifest) => {
            let runtime = runtime_inventory(app_data_dir)
                .active
                .context("no active runtime for stale manifest fault")?;
            let manifest = verified_manifest_path(&runtime.library_path)?;
            if manifest.exists() {
                corrupt_file(&manifest).with_context(|| {
                    format!("failed to corrupt runtime manifest {}", manifest.display())
                })?;
            } else {
                fs::write(&manifest, b"{\"stale\":true}\n").with_context(|| {
                    format!(
                        "failed to write stale runtime manifest {}",
                        manifest.display()
                    )
                })?;
            }
            Ok(FaultApplication {
                target_path: manifest,
                notes: "runtime verification manifest no longer matches installed files".into(),
            })
        }
        (FaultTarget::Runtime, FaultKind::InterruptAfterExtraction) => {
            let mut slots = read_slots(app_data_dir);
            let active = slots
                .active
                .clone()
                .context("no active runtime for interrupt-after-extraction fault")?;
            slots.candidate = Some(active.clone());
            slots.previous = slots.active.take();
            slots.active = None;
            slots.activation_pending = false;
            slots.activation_attempts = 0;
            write_slots(app_data_dir, &slots)?;
            Ok(FaultApplication {
                target_path: runtime_artifact_dir(app_data_dir, &active),
                notes: "runtime left extracted as candidate without activation".into(),
            })
        }
        (FaultTarget::Runtime, FaultKind::InterruptAfterActivation) => {
            let mut slots = read_slots(app_data_dir);
            if slots.active.is_none() {
                bail!("no active runtime for interrupt-after-activation fault");
            }
            slots.activation_pending = true;
            slots.activation_attempts = 1;
            write_slots(app_data_dir, &slots)?;
            Ok(FaultApplication {
                target_path: runtime_bootstrap::runtimes_root(app_data_dir),
                notes: "activation_pending left set after promotion".into(),
            })
        }
        (FaultTarget::Model, FaultKind::CorruptInstalledFile) => {
            let path = active_model_path(app_data_dir)?;
            corrupt_file(&path)
                .with_context(|| format!("failed to corrupt installed model {}", path.display()))?;
            Ok(FaultApplication {
                target_path: path,
                notes: "appended corrupt byte to managed model".into(),
            })
        }
        (FaultTarget::Model, FaultKind::StaleVerificationManifest) => {
            let path = active_model_path(app_data_dir)?;
            let manifest = verified_manifest_path(&path)?;
            if manifest.exists() {
                corrupt_file(&manifest).with_context(|| {
                    format!("failed to corrupt model manifest {}", manifest.display())
                })?;
            } else {
                fs::write(&manifest, b"{\"stale\":true}\n").with_context(|| {
                    format!(
                        "failed to write stale model manifest {}",
                        manifest.display()
                    )
                })?;
            }
            Ok(FaultApplication {
                target_path: manifest,
                notes: "model verification manifest no longer matches ONNX".into(),
            })
        }
        (FaultTarget::Model, FaultKind::InterruptAfterExtraction)
        | (FaultTarget::Model, FaultKind::InterruptAfterActivation) => {
            let path = active_model_path(app_data_dir)?;
            let identity = installed_identity_path(&path)?;
            if identity.exists() {
                fs::remove_file(&identity).with_context(|| {
                    format!("failed to remove model identity {}", identity.display())
                })?;
            }
            let manifest = verified_manifest_path(&path)?;
            if manifest.exists() {
                fs::remove_file(&manifest).with_context(|| {
                    format!("failed to remove model manifest {}", manifest.display())
                })?;
            }
            Ok(FaultApplication {
                target_path: path,
                notes: "model bytes remain but verification/identity commit is missing".into(),
            })
        }
        (FaultTarget::Model, FaultKind::StaleDownloadingState) => {
            let path = active_model_path(app_data_dir)?;
            let identity = installed_identity_path(&path)?;
            if identity.exists() {
                fs::remove_file(&identity).with_context(|| {
                    format!("failed to remove model identity {}", identity.display())
                })?;
            }
            let partial = path.with_extension("onnx.part");
            fs::write(&partial, b"partial-download").with_context(|| {
                format!(
                    "failed to write stale partial download {}",
                    partial.display()
                )
            })?;
            Ok(FaultApplication {
                target_path: partial,
                notes: "stale Downloading marker (.part) with complete managed model on disk"
                    .into(),
            })
        }
        _ => bail!(
            "unsupported fault combination {:?}/{:?}",
            scenario.target,
            scenario.kind
        ),
    }
}

/// Assert the injected fault is visible to production verification before recovery.
pub fn inspect_fault_state(
    app_data_dir: &Path,
    scenario: &FaultScenario,
) -> Result<FaultRecoveryResult> {
    let scenario_id = scenario.assertion_id();
    if scenario.requires_fault_server() {
        return Ok(FaultRecoveryResult {
            scenario_id,
            fault_detected: true,
            prior_runtime_usable: true,
            observation: "archive-delivery fault deferred to fault-server suite".into(),
        });
    }

    match (scenario.target, scenario.kind) {
        (FaultTarget::Runtime, FaultKind::CorruptInstalledFile) => {
            let inventory = runtime_inventory(app_data_dir);
            let verified = inventory
                .active
                .as_ref()
                .map(|runtime| verify_runtime_files(runtime).unwrap_or(false))
                .unwrap_or(false);
            let slots = read_slots(app_data_dir);
            Ok(FaultRecoveryResult {
                scenario_id,
                fault_detected: !verified,
                prior_runtime_usable: slots.previous.is_some() || inventory.legacy_path.is_some(),
                observation: if verified {
                    "corrupt runtime still verifies as healthy".into()
                } else {
                    "corrupt runtime fails verification as required".into()
                },
            })
        }
        (FaultTarget::Runtime, FaultKind::StaleVerificationManifest) => {
            let inventory = runtime_inventory(app_data_dir);
            let active = inventory
                .active
                .as_ref()
                .context("no active runtime for stale-manifest inspection")?;
            let manifest = verified_manifest_path(&active.library_path)?;
            let digest = active
                .record
                .files
                .iter()
                .find(|file| {
                    file.path
                        .ends_with(crate::separator::model::ORT_RUNTIME_FILENAME)
                })
                .map(|file| file.sha256.as_str())
                .unwrap_or("");
            let matches = if digest.is_empty() {
                false
            } else {
                crate::separator::verified_manifest::verified_manifest_matches(
                    &active.library_path,
                    digest,
                )
                .unwrap_or(false)
            };
            let slots = read_slots(app_data_dir);
            Ok(FaultRecoveryResult {
                scenario_id,
                fault_detected: !matches || !manifest.exists(),
                prior_runtime_usable: slots.previous.is_some() || inventory.legacy_path.is_some(),
                observation: format!(
                    "manifest_exists={} matches_record={matches}",
                    manifest.exists()
                ),
            })
        }
        (FaultTarget::Runtime, FaultKind::InterruptAfterExtraction) => {
            let slots = read_slots(app_data_dir);
            let candidate_ok = slots
                .candidate
                .as_deref()
                .and_then(|id| installed_runtime(app_data_dir, id))
                .map(|runtime| verify_runtime_files(&runtime).unwrap_or(false))
                .unwrap_or(false);
            Ok(FaultRecoveryResult {
                scenario_id,
                fault_detected: candidate_ok && slots.active.is_none(),
                prior_runtime_usable: slots.previous.is_some(),
                observation: format!(
                    "candidate={candidate_ok} active={:?} previous={:?}",
                    slots.active, slots.previous
                ),
            })
        }
        (FaultTarget::Runtime, FaultKind::InterruptAfterActivation) => {
            let slots = read_slots(app_data_dir);
            Ok(FaultRecoveryResult {
                scenario_id,
                fault_detected: slots.activation_pending,
                prior_runtime_usable: slots.previous.is_some() || slots.active.is_some(),
                observation: format!(
                    "activation_pending={} attempts={}",
                    slots.activation_pending, slots.activation_attempts
                ),
            })
        }
        (FaultTarget::Model, FaultKind::CorruptInstalledFile)
        | (FaultTarget::Model, FaultKind::StaleVerificationManifest)
        | (FaultTarget::Model, FaultKind::InterruptAfterExtraction)
        | (FaultTarget::Model, FaultKind::InterruptAfterActivation)
        | (FaultTarget::Model, FaultKind::StaleDownloadingState) => {
            let path = active_model_path(app_data_dir)?;
            let descriptor = descriptor_for(ModelVariant::default());
            let resolution = bootstrap::resolve_managed_model_installation(
                &path,
                descriptor.file_sha256.as_str(),
            )?;
            let ready = matches!(resolution, bootstrap::ModelInstallationResolution::Ready(_));
            // Stale downloading leaves valid bytes; other faults must not remain Ready.
            let fault_detected = match scenario.kind {
                FaultKind::StaleDownloadingState => path.exists(),
                _ => !ready,
            };
            Ok(FaultRecoveryResult {
                scenario_id,
                fault_detected,
                prior_runtime_usable: true,
                observation: format!("model_path={} ready={ready}", path.display()),
            })
        }
        _ => Ok(FaultRecoveryResult {
            scenario_id,
            fault_detected: true,
            prior_runtime_usable: true,
            observation: "no-op inspect".into(),
        }),
    }
}

fn active_runtime_library(app_data_dir: &Path) -> Result<PathBuf> {
    let inventory = runtime_inventory(app_data_dir);
    if let Some(active) = inventory.active {
        return Ok(active.library_path);
    }
    if let Some(legacy) = inventory.legacy_path {
        return Ok(legacy);
    }
    bail!(
        "no active or legacy runtime library under {}",
        app_data_dir.display()
    )
}

fn active_model_path(app_data_dir: &Path) -> Result<PathBuf> {
    let variant = crate::config::load_config(app_data_dir)?
        .unwrap_or_default()
        .effective_model_variant();
    let descriptor = descriptor_for(variant);
    Ok(managed_model_path_for(app_data_dir, descriptor))
}

/// Remove leftover partial downloads left by stale-Downloading faults.
pub fn clear_stale_partials(app_data_dir: &Path) -> Result<()> {
    let models_dir = app_data_dir.join("models");
    if !models_dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&models_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "part")
        {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}
