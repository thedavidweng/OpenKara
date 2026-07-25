use crate::config::ModelVariant;
use crate::separator::catalog::{
    self, identity_from_catalog_model, read_installed_identity, InstalledModelIdentity,
    VerifiedCatalog,
};
use crate::separator::verified_manifest::{
    sha256_hex, verified_manifest_matches, verified_manifest_path, write_verified_manifest,
};
use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use std::sync::OnceLock;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Everything needed to install and verify one model artifact. Descriptors
/// are resolved from the openkara-models catalog — the embedded snapshot by
/// default, or a freshly verified catalog for updates — never from
/// hand-maintained URL/SHA constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDescriptor {
    pub variant: ModelVariant,
    pub filename: String,
    pub download_url: String,
    pub sha256: String,
    pub byte_size: u64,
    pub artifact_id: String,
    pub upstream_tag: String,
    pub identity: InstalledModelIdentity,
}

pub fn descriptor_from_catalog(
    catalog: &VerifiedCatalog,
    variant: ModelVariant,
) -> Result<ModelDescriptor> {
    let model = catalog::resolve_model(&catalog.manifest, variant)?;
    Ok(ModelDescriptor {
        variant,
        filename: model.filename.clone(),
        download_url: model.download_url.clone(),
        sha256: model.archive_digest.clone(),
        byte_size: model.byte_size,
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
            let identity_ok =
                verify_model_install(managed_path, &identity.sha256).with_context(|| {
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

    if dev_path.exists()
        && verify_model_install(dev_path, expected_sha256)
            .with_context(|| format!("failed to verify development model {}", dev_path.display()))?
    {
        return Ok(ModelInstallationResolution::Ready(ResolvedModelPath {
            path: dev_path.to_path_buf(),
            source: ModelSource::DevelopmentFallback,
        }));
    }

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

pub fn install_verified_model_bytes(
    destination: &Path,
    payload: &[u8],
    expected_sha256: &str,
) -> Result<()> {
    let actual_sha256 = sha256_hex(payload);
    if actual_sha256 != expected_sha256 {
        bail!(
            "downloaded model checksum mismatch: expected {expected_sha256}, got {actual_sha256}"
        );
    }

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

    let temp_path = temporary_download_path(destination);
    fs::write(&temp_path, payload).with_context(|| {
        format!(
            "failed to write temporary model download {}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, destination).with_context(|| {
        format!(
            "failed to move verified model from {} to {}",
            temp_path.display(),
            destination.display()
        )
    })?;
    write_verified_manifest(destination, expected_sha256)?;

    Ok(())
}

pub fn download_and_install_model(
    destination: &Path,
    descriptor: &ModelDescriptor,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let download_url = descriptor.download_url.as_str();
    let expected_sha256 = descriptor.sha256.as_str();
    let client = Client::builder()
        .build()
        .context("failed to build model download client")?;
    let mut response = client
        .get(download_url)
        .send()
        .and_then(|response| response.error_for_status())
        .with_context(|| format!("failed to download ONNX model from {download_url}"))?;

    let total_bytes = response.content_length();
    let mut last_emit_bytes = 0_u64;
    let mut last_emit_at = Instant::now();
    let emit_interval = Duration::from_millis(150);
    let emit_min_step: u64 = 256 * 1024;

    let mut emit = |downloaded: u64, total: Option<u64>, force: bool| {
        let step_ok = downloaded.saturating_sub(last_emit_bytes) >= emit_min_step;
        let time_ok = last_emit_at.elapsed() >= emit_interval;
        if force || step_ok || time_ok {
            progress(downloaded, total);
            last_emit_bytes = downloaded;
            last_emit_at = Instant::now();
        }
    };

    emit(0, total_bytes, true);

    let mut payload = Vec::new();
    let mut downloaded_bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = response
            .read(&mut buffer)
            .context("failed while streaming ONNX model download")?;
        if read == 0 {
            break;
        }

        payload.extend_from_slice(&buffer[..read]);
        downloaded_bytes += read as u64;
        emit(downloaded_bytes, total_bytes, false);
    }

    emit(downloaded_bytes, total_bytes, true);

    install_verified_model_bytes(destination, &payload, expected_sha256)?;
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

fn temporary_download_path(destination: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();

    destination.with_extension(format!("download.{timestamp}.tmp"))
}
