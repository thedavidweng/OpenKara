use anyhow::{bail, Context, Result};
use openkara_lib::separator::verified_manifest::{sha256_hex, write_verified_manifest};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn unique_temp_path(prefix: &str) -> PathBuf {
    // Parallel integration tests were colliding on timestamp-only names and deleting each
    // other's fixtures. A per-process counter keeps temp paths unique even when the clock
    // resolution is coarser than the test scheduler.
    let sequence = TEMP_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "openkara-{prefix}-{pid}-{timestamp}-{sequence}",
        pid = std::process::id()
    ))
}

/// Materialize a verified managed install from an in-memory payload: verify the
/// payload against `expected_sha256`, write the model bytes, then persist the
/// startup verification manifest — the exact on-disk shape a real streaming
/// install leaves behind. Used by the phase6 integration tests to stage a
/// trusted install without exercising the network download path.
// Not every integration-test binary that pulls in this shared support module
// stages a managed install, so this helper is dead code in some of them.
#[allow(dead_code)]
pub fn install_verified_model_bytes(
    destination: &Path,
    payload: &[u8],
    expected_sha256: &str,
) -> Result<()> {
    let actual_sha256 = sha256_hex(payload);
    if actual_sha256 != expected_sha256 {
        // Reject before touching the filesystem so a mismatch never leaves a
        // partial install behind.
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
    fs::write(destination, payload)
        .with_context(|| format!("failed to write model fixture {}", destination.display()))?;
    write_verified_manifest(destination, expected_sha256)?;

    Ok(())
}
