use crate::hash;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedManifest {
    pub filename: String,
    pub sha256: String,
    pub file_size: u64,
    pub modified_unix_nanos: u128,
}

pub fn verified_manifest_path(path: &Path) -> Result<PathBuf> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("path {} has no filename", path.display()))?;
    Ok(path.with_file_name(format!("{filename}.verified.json")))
}

pub fn build_manifest(path: &Path, expected_sha256: &str) -> Result<VerifiedManifest> {
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
        sha256: expected_sha256.to_owned(),
        file_size: metadata.len(),
        modified_unix_nanos,
    })
}

pub fn verified_manifest_matches(path: &Path, expected_sha256: &str) -> Result<bool> {
    let manifest_path = verified_manifest_path(path)?;
    if !manifest_path.exists() {
        return Ok(false);
    }

    let expected = build_manifest(path, expected_sha256)?;
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

pub fn write_verified_manifest(path: &Path, expected_sha256: &str) -> Result<()> {
    let manifest = build_manifest(path, expected_sha256)?;
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
