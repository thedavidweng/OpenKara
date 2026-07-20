use crate::hash;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// Verification manifest written next to a downloaded model or runtime file.
///
/// The `sha256` field records the checksum that was verified at install time.
/// For the ONNX Runtime (which has a pinned release), callers compare this
/// field against the pinned value. For models (which track a floating latest
/// release), callers trust the manifest's metadata alone — the checksum was
/// verified against the upstream manifest at download time and is kept here
/// for update detection, not for startup re-verification.
///
/// `release_tag` is set for model installs (e.g. `"model-v2.1.0"`) and `None`
/// for runtime installs. Old manifests without this field parse as `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedManifest {
    pub filename: String,
    pub sha256: String,
    pub file_size: u64,
    pub modified_unix_nanos: u128,
    #[serde(default)]
    pub release_tag: Option<String>,
}

pub fn verified_manifest_path(path: &Path) -> Result<PathBuf> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("path {} has no filename", path.display()))?;
    Ok(path.with_file_name(format!("{filename}.verified.json")))
}

pub fn build_manifest(
    path: &Path,
    sha256: &str,
    release_tag: Option<&str>,
) -> Result<VerifiedManifest> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to inspect file {}", path.display()))?;
    let modified_unix_nanos = metadata
        .modified()
        .with_context(|| format!("failed to read modified time for {}", path.display()))?
        .duration_since(UNIX_EPOCH)
        .with_context(|| format!("file {} has invalid modified time", path.display()))?
        .as_nanos();
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("path {} has no filename", path.display()))?
        .to_owned();

    Ok(VerifiedManifest {
        filename,
        sha256: sha256.to_owned(),
        file_size: metadata.len(),
        modified_unix_nanos,
        release_tag: release_tag.map(ToOwned::to_owned),
    })
}

/// Full manifest check used by the ONNX Runtime path, which has a pinned
/// `expected_sha256`. Compares filename, sha256, file size, and modified time.
pub fn verified_manifest_matches(path: &Path, expected_sha256: &str) -> Result<bool> {
    let manifest_path = verified_manifest_path(path)?;
    if !manifest_path.exists() {
        return Ok(false);
    }

    let expected = build_manifest(path, expected_sha256, None)?;
    let contents = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "failed to read verification manifest {}",
            manifest_path.display()
        )
    })?;
    let actual: VerifiedManifest = serde_json::from_str(&contents).with_context(|| {
        format!(
            "failed to parse verification manifest {}",
            manifest_path.display()
        )
    })?;

    Ok(actual == expected)
}

/// Metadata-only manifest check used by the model path, which has no pinned
/// checksum. If the file's size and modified time match what the manifest
/// recorded, the file has not changed since install and the stored checksum
/// is still valid.
pub fn verified_manifest_metadata_matches(path: &Path) -> Result<bool> {
    let manifest_path = verified_manifest_path(path)?;
    if !manifest_path.exists() {
        return Ok(false);
    }

    let metadata =
        fs::metadata(path).with_context(|| format!("failed to inspect file {}", path.display()))?;
    let modified_unix_nanos = metadata
        .modified()
        .with_context(|| format!("failed to read modified time for {}", path.display()))?
        .duration_since(UNIX_EPOCH)
        .with_context(|| format!("file {} has invalid modified time", path.display()))?
        .as_nanos();
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("path {} has no filename", path.display()))?;

    let contents = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "failed to read verification manifest {}",
            manifest_path.display()
        )
    })?;
    let actual: VerifiedManifest = serde_json::from_str(&contents).with_context(|| {
        format!(
            "failed to parse verification manifest {}",
            manifest_path.display()
        )
    })?;

    Ok(actual.filename == filename
        && actual.file_size == metadata.len()
        && actual.modified_unix_nanos == modified_unix_nanos)
}

/// Reads the manifest for a file without comparing it to anything.
/// Used by the model update check to discover the installed release tag
/// and checksum.
pub fn read_verified_manifest(path: &Path) -> Result<Option<VerifiedManifest>> {
    let manifest_path = verified_manifest_path(path)?;
    if !manifest_path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "failed to read verification manifest {}",
            manifest_path.display()
        )
    })?;
    let manifest = serde_json::from_str(&contents).with_context(|| {
        format!(
            "failed to parse verification manifest {}",
            manifest_path.display()
        )
    })?;
    Ok(Some(manifest))
}

pub fn write_verified_manifest(path: &Path, sha256: &str, release_tag: Option<&str>) -> Result<()> {
    let manifest = build_manifest(path, sha256, release_tag)?;
    let manifest_path = verified_manifest_path(path)?;
    let json = serde_json::to_string_pretty(&manifest).context("failed to serialize manifest")?;
    fs::write(&manifest_path, json).with_context(|| {
        format!(
            "failed to write verification manifest {}",
            manifest_path.display()
        )
    })?;
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hash::hex_lower(hasher.finalize())
}

pub fn verify_file_checksum(path: &Path, expected_sha256: &str) -> Result<bool> {
    let bytes =
        fs::read(path).with_context(|| format!("failed to read file {}", path.display()))?;
    Ok(sha256_hex(&bytes) == expected_sha256)
}
