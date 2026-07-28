use crate::config::ModelVariant;
use crate::separator::catalog::{
    self, identity_from_catalog_model, read_installed_identity, InstalledArtifactRecord,
    VerifiedCatalog,
};
use crate::separator::verified_manifest::{
    verified_manifest_matches, verified_manifest_path, write_verified_manifest,
};
use anyhow::{bail, Context, Result};
use std::sync::OnceLock;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Everything needed to install and verify one model artifact. Descriptors
/// are resolved from the openkara-models catalog — the embedded snapshot by
/// default, or a freshly verified catalog for updates — never from
/// hand-maintained URL/SHA constants.
///
/// Newer generations deliver models as compressed archives: the download is
/// verified against `download_sha256`, the installed `.onnx` against
/// `file_sha256`. For raw deliveries the two are the same bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDescriptor {
    pub variant: ModelVariant,
    /// Installed `.onnx` filename under `models/`.
    pub filename: String,
    pub file_sha256: String,
    pub file_size: u64,
    pub download_filename: String,
    pub download_url: String,
    pub download_sha256: String,
    pub download_size: u64,
    pub archived: bool,
    pub artifact_id: String,
    pub upstream_tag: String,
    pub identity: InstalledArtifactRecord,
}

pub fn descriptor_from_catalog(
    catalog: &VerifiedCatalog,
    variant: ModelVariant,
) -> Result<ModelDescriptor> {
    let model = catalog::resolve_model(&catalog.manifest, variant)?;
    let (model_file, model_digest) = model.primary_model_file()?;
    Ok(ModelDescriptor {
        variant,
        filename: model_file.to_owned(),
        file_sha256: model_digest.sha256.clone(),
        file_size: model_digest.size,
        download_filename: model.filename.clone(),
        download_url: model.download_url.clone(),
        download_sha256: model.archive_digest.clone(),
        download_size: model.byte_size,
        archived: model.is_archived(),
        artifact_id: model.artifact_id.clone(),
        upstream_tag: model.upstream.tag.clone(),
        identity: identity_from_catalog_model(model, catalog),
    })
}

/// The pinned descriptor for a variant, resolved once from the embedded
/// catalog snapshot. This is the offline baseline: it requires no network and
/// no configuration, and it is what startup readiness verifies against.
pub fn descriptor_for(variant: ModelVariant) -> &'static ModelDescriptor {
    static HTDEMUCS: OnceLock<ModelDescriptor> = OnceLock::new();
    static HTDEMUCS_FT: OnceLock<ModelDescriptor> = OnceLock::new();
    let (slot, variant) = match variant {
        ModelVariant::Htdemucs => (&HTDEMUCS, ModelVariant::Htdemucs),
        ModelVariant::HtdemucsFt => (&HTDEMUCS_FT, ModelVariant::HtdemucsFt),
    };
    slot.get_or_init(|| {
        descriptor_from_catalog(catalog::embedded_catalog(), variant)
            .expect("embedded catalog must resolve every model variant")
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSource {
    ManagedInstall,
    DevelopmentFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelPath {
    pub path: PathBuf,
    pub source: ModelSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelInstallationResolution {
    Ready(ResolvedModelPath),
    /// A file exists at the managed install path but its digest does not match the
    /// pinned release. The file is kept so the user can delete it from Settings.
    LegacyManaged(PathBuf),
    Absent,
}

pub fn managed_model_path(app_data_dir: &Path) -> PathBuf {
    managed_model_path_for(app_data_dir, descriptor_for(ModelVariant::Htdemucs))
}

pub fn managed_model_path_for(app_data_dir: &Path, descriptor: &ModelDescriptor) -> PathBuf {
    app_data_dir.join("models").join(&descriptor.filename)
}

pub fn model_file_size(app_data_dir: &Path, variant: ModelVariant) -> Option<u64> {
    let descriptor = descriptor_for(variant);
    let path = managed_model_path_for(app_data_dir, descriptor);
    fs::metadata(&path).ok().map(|m| m.len())
}

pub fn delete_model_file(app_data_dir: &Path, variant: ModelVariant) -> Result<()> {
    let descriptor = descriptor_for(variant);
    let path = managed_model_path_for(app_data_dir, descriptor);
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("failed to delete model file {}", path.display()))?;
    }
    let manifest_path = verified_manifest_path(&path)?;
    if manifest_path.exists() {
        fs::remove_file(&manifest_path).with_context(|| {
            format!(
                "failed to delete model verification manifest {}",
                manifest_path.display()
            )
        })?;
    }
    catalog::delete_installed_identity(&path)?;
    Ok(())
}

pub fn resolve_model_installation(
    managed_path: &Path,
    dev_path: &Path,
    expected_sha256: &str,
) -> Result<ModelInstallationResolution> {
    let managed_resolution = resolve_managed_model_installation(managed_path, expected_sha256)?;

    if matches!(&managed_resolution, ModelInstallationResolution::Ready(_)) {
        return Ok(managed_resolution);
    }

    if dev_path.exists()
        && verify_model_install(dev_path, expected_sha256)
            .with_context(|| format!("failed to verify development model {}", dev_path.display()))?
    {
        return Ok(ModelInstallationResolution::Ready(ResolvedModelPath {
            path: dev_path.to_path_buf(),
            source: ModelSource::DevelopmentFallback,
        }));
    }

    Ok(managed_resolution)
}

/// Resolve only the app-managed model installation. Release and installer
/// smoke tests use this boundary to prove a packaged app never succeeds by
/// borrowing the repository's development model cache.
pub fn resolve_managed_model_installation(
    managed_path: &Path,
    expected_sha256: &str,
) -> Result<ModelInstallationResolution> {
    let managed_invalid = if managed_path.exists() {
        let ok = verify_model_install(managed_path, expected_sha256).with_context(|| {
            format!("failed to verify managed model {}", managed_path.display())
        })?;
        if ok {
            return Ok(ModelInstallationResolution::Ready(ResolvedModelPath {
                path: managed_path.to_path_buf(),
                source: ModelSource::ManagedInstall,
            }));
        }
        // The file does not match the embedded pin, but it may be a model
        // installed from a *newer* verified catalog generation. Its identity
        // record carries the digest that install verified; a catalog refresh
        // failure or an older app binary must not invalidate it.
        if let Some(identity) = read_installed_identity(managed_path) {
            let identity_ok = verify_model_install(managed_path, &identity.archive_sha256)
                .with_context(|| {
                    format!(
                        "failed to verify managed model {} against its identity record",
                        managed_path.display()
                    )
                })?;
            if identity_ok {
                return Ok(ModelInstallationResolution::Ready(ResolvedModelPath {
                    path: managed_path.to_path_buf(),
                    source: ModelSource::ManagedInstall,
                }));
            }
        }
        true
    } else {
        false
    };

    if managed_invalid {
        return Ok(ModelInstallationResolution::LegacyManaged(
            managed_path.to_path_buf(),
        ));
    }

    Ok(ModelInstallationResolution::Absent)
}

/// True when a file exists at `path` and its digest matches `expected_sha256`.
/// Unlike `resolve_model_installation`, this never falls back to an identity
/// record — it answers "is this exact artifact installed".
pub fn model_matches_digest(path: &Path, expected_sha256: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    verify_model_install(path, expected_sha256)
}

pub fn resolve_existing_model_path(
    managed_path: &Path,
    dev_path: &Path,
    expected_sha256: &str,
) -> Result<Option<ResolvedModelPath>> {
    Ok(
        match resolve_model_installation(managed_path, dev_path, expected_sha256)? {
            ModelInstallationResolution::Ready(path) => Some(path),
            ModelInstallationResolution::LegacyManaged(_) | ModelInstallationResolution::Absent => {
                None
            }
        },
    )
}

pub fn resolve_existing_managed_model_path(
    managed_path: &Path,
    expected_sha256: &str,
) -> Result<Option<ResolvedModelPath>> {
    Ok(match resolve_managed_model_installation(managed_path, expected_sha256)? {
        ModelInstallationResolution::Ready(path) => Some(path),
        ModelInstallationResolution::LegacyManaged(_) | ModelInstallationResolution::Absent => None,
    })
}

/// Download and install a model through the shared artifact plumbing:
/// stream to a temp file with fixed memory, verify the download digest,
/// extract archived deliveries safely, verify the installed `.onnx` digest,
/// and promote atomically. No full-payload buffer exists at any point.
pub fn download_and_install_model(
    destination: &Path,
    descriptor: &ModelDescriptor,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let parent = destination.parent().with_context(|| {
        format!(
            "model destination {} is missing a parent directory",
            destination.display()
        )
    })?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create model destination directory {}",
            parent.display()
        )
    })?;

    let downloaded = crate::separator::artifacts::download_verified_to_temp(
        &descriptor.download_url,
        descriptor.download_size,
        &descriptor.download_sha256,
        parent,
        progress,
    )?;

    let install_result = (|| -> Result<()> {
        if descriptor.archived {
            let kind = crate::separator::artifacts::archive_kind_for_filename(
                &descriptor.download_filename,
            )?;
            let extract_dir =
                crate::separator::artifacts::unique_temp_path(parent, "model-extract");
            let extraction = (|| -> Result<()> {
                crate::separator::artifacts::extract_archive_safely(
                    &downloaded,
                    kind,
                    &extract_dir,
                )?;
                let extracted_model = extract_dir.join(&descriptor.filename);
                let metadata = fs::metadata(&extracted_model).with_context(|| {
                    format!(
                        "archive did not contain the declared model file {}",
                        descriptor.filename
                    )
                })?;
                if metadata.len() != descriptor.file_size {
                    bail!(
                        "extracted model has size {}, expected {}",
                        metadata.len(),
                        descriptor.file_size
                    );
                }
                let actual = crate::separator::artifacts::sha256_file(&extracted_model)?;
                if actual != descriptor.file_sha256 {
                    bail!("extracted model digest mismatch");
                }
                fs::rename(&extracted_model, destination).with_context(|| {
                    format!(
                        "failed to promote extracted model to {}",
                        destination.display()
                    )
                })?;
                Ok(())
            })();
            let _ = fs::remove_dir_all(&extract_dir);
            extraction?;
        } else {
            // Raw delivery: the verified download IS the model file.
            fs::rename(&downloaded, destination).with_context(|| {
                format!(
                    "failed to promote downloaded model to {}",
                    destination.display()
                )
            })?;
        }
        Ok(())
    })();

    let _ = fs::remove_file(&downloaded);
    install_result?;

    write_verified_manifest(destination, &descriptor.file_sha256)?;
    // The identity record is what update comparisons run against and what
    // keeps this install verifiable if the app's embedded pin later moves.
    catalog::write_installed_identity(destination, &descriptor.identity)?;
    Ok(())
}

fn verify_model_install(path: &Path, expected_sha256: &str) -> Result<bool> {
    // RATIONALE: Managed ONNX files are hundreds of MB. Once the app has
    // verified a model and recorded its exact metadata, startup should not read
    // the whole file again unless that metadata changes.
    if verified_manifest_matches(path, expected_sha256)? {
        return Ok(true);
    }

    let ok = crate::separator::verified_manifest::verify_file_checksum(path, expected_sha256)?;
    if ok {
        write_verified_manifest(path, expected_sha256)?;
    }
    Ok(ok)
}
