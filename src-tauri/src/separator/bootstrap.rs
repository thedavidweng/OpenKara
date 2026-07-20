use crate::config::ModelVariant;
use crate::separator::verified_manifest::{
    read_verified_manifest, sha256_hex, verified_manifest_metadata_matches, verified_manifest_path,
    write_verified_manifest,
};
use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Static metadata for a model variant. The download URL and SHA-256 are
/// resolved at runtime from the upstream `latest.json` manifest rather than
/// hardcoded here, so the app always uses the newest release without a
/// code change.
pub struct ModelDescriptor {
    pub filename: &'static str,
    /// Key in the upstream `latest.json` manifest (e.g. `"htdemucs"`).
    pub variant_key: &'static str,
}

pub const HTDEMUCS: ModelDescriptor = ModelDescriptor {
    filename: "htdemucs.onnx",
    variant_key: "htdemucs",
};

pub const HTDEMUCS_FT: ModelDescriptor = ModelDescriptor {
    filename: "htdemucs_ft.onnx",
    variant_key: "htdemucs_ft",
};

pub fn descriptor_for(variant: ModelVariant) -> &'static ModelDescriptor {
    match variant {
        ModelVariant::Htdemucs => &HTDEMUCS,
        ModelVariant::HtdemucsFt => &HTDEMUCS_FT,
    }
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
    Absent,
}

pub fn managed_model_path(app_data_dir: &Path) -> PathBuf {
    managed_model_path_for(app_data_dir, &HTDEMUCS)
}

pub fn managed_model_path_for(app_data_dir: &Path, descriptor: &ModelDescriptor) -> PathBuf {
    app_data_dir.join("models").join(descriptor.filename)
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
    Ok(())
}

/// Resolves whether a usable model is installed at the managed or dev path.
///
/// Without a hardcoded pin, "usable" means the file has a verification
/// manifest whose metadata (filename, size, modified time) matches the
/// file on disk. If the manifest is missing, the file is treated as absent
/// and will be re-downloaded. If the manifest exists but metadata does not
/// match, the checksum is recomputed and compared to the manifest's stored
/// value — a match means the file content is unchanged (only metadata
/// drifted) and the manifest is refreshed; a mismatch means corruption
/// and the file is treated as absent.
pub fn resolve_model_installation(
    managed_path: &Path,
    dev_path: &Path,
) -> Result<ModelInstallationResolution> {
    if managed_path.exists() && verify_model_install(managed_path)? {
        return Ok(ModelInstallationResolution::Ready(ResolvedModelPath {
            path: managed_path.to_path_buf(),
            source: ModelSource::ManagedInstall,
        }));
    }

    if dev_path.exists() && verify_model_install(dev_path)? {
        return Ok(ModelInstallationResolution::Ready(ResolvedModelPath {
            path: dev_path.to_path_buf(),
            source: ModelSource::DevelopmentFallback,
        }));
    }

    Ok(ModelInstallationResolution::Absent)
}

pub fn resolve_existing_model_path(
    managed_path: &Path,
    dev_path: &Path,
) -> Result<Option<ResolvedModelPath>> {
    Ok(match resolve_model_installation(managed_path, dev_path)? {
        ModelInstallationResolution::Ready(path) => Some(path),
        ModelInstallationResolution::Absent => None,
    })
}

pub fn install_verified_model_bytes(
    destination: &Path,
    payload: &[u8],
    expected_sha256: &str,
    release_tag: Option<&str>,
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
    write_verified_manifest(destination, expected_sha256, release_tag)?;

    Ok(())
}

pub fn download_and_install_model(
    destination: &Path,
    download_url: &str,
    expected_sha256: &str,
    release_tag: Option<&str>,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
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

    install_verified_model_bytes(destination, &payload, expected_sha256, release_tag)
}

/// Reads the installed model's release tag from its verification manifest,
/// if one exists. Used by the update check to compare against the upstream
/// latest tag.
pub fn installed_release_tag(model_path: &Path) -> Result<Option<String>> {
    Ok(read_verified_manifest(model_path)?.and_then(|m| m.release_tag))
}

fn verify_model_install(path: &Path) -> Result<bool> {
    // RATIONALE: Managed ONNX files are hundreds of MB. Once the app has
    // verified a model and recorded its metadata, startup should not read
    // the whole file again unless that metadata changes.
    if verified_manifest_metadata_matches(path)? {
        return Ok(true);
    }

    // Metadata mismatch — recompute the checksum and compare it to the
    // value the manifest recorded at install time. If the content is
    // unchanged (only metadata drifted), refresh the manifest and trust
    // the file. If the content changed, the file is corrupt or unknown.
    let manifest = read_verified_manifest(path)?;
    if let Some(manifest) = manifest {
        let ok = crate::separator::verified_manifest::verify_file_checksum(path, &manifest.sha256)?;
        if ok {
            write_verified_manifest(path, &manifest.sha256, manifest.release_tag.as_deref())?;
            return Ok(true);
        }
    }

    Ok(false)
}

fn temporary_download_path(destination: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();

    destination.with_extension(format!("download.{timestamp}.tmp"))
}
