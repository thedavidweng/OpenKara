//! Shared download mechanics for the reader and the asset fetch: resumable
//! chunked downloads into sibling `.part.<operation-id>` files, size and
//! SHA-256 verification, and the atomic install of a verified candidate.
//!
//! A failed or interrupted transfer leaves the received prefix in the part
//! file so the next attempt resumes where it stopped. A candidate that fails
//! verification is deleted: corrupt bytes can never resume into a valid
//! object. The destination is only ever replaced by a fully verified
//! candidate.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{kind_from_io_error, RemoteError, RemoteErrorKind, RemoteResult};
use crate::storage::RepositoryStorage;

/// A file that passed size and digest verification.
pub(crate) struct VerifiedFile {
    pub size: u64,
    pub digest: String,
}

/// The sibling temp file a download streams into before verification.
pub(crate) fn part_path_for(destination: &Path, operation_id: &str) -> PathBuf {
    let file_name = destination
        .file_name()
        .map(|name| format!("{}.part.{}", name.to_string_lossy(), operation_id))
        .unwrap_or_else(|| format!("openkara.part.{operation_id}"));
    destination.with_file_name(file_name)
}

/// Download `remote_path` into `part_path`, resuming from the part file's
/// durable prefix when the storage supports range reads and the expected
/// size is known. `on_progress` receives the durable byte count after every
/// chunk and once more before a transfer error is returned, so the caller
/// can mirror resumable-transfer bookkeeping.
pub(crate) fn download_object(
    storage: &dyn RepositoryStorage,
    remote_path: &str,
    expected_size: Option<u64>,
    part_path: &Path,
    chunk_size: u64,
    on_progress: &mut dyn FnMut(u64),
) -> RemoteResult<()> {
    if let Some(parent) = part_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            io_error(
                format!("failed to create directory {}", parent.display()),
                error,
            )
        })?;
    }

    let resumable = storage.capabilities().range_read && expected_size.is_some();
    if !resumable {
        return download_object_whole(storage, remote_path, part_path, on_progress);
    }

    let expected_size = expected_size.unwrap_or(0);
    let mut offset = fs::metadata(part_path).map(|meta| meta.len()).unwrap_or(0);
    if offset > expected_size {
        let _ = fs::remove_file(part_path);
        offset = 0;
    }
    if offset == expected_size {
        return Ok(());
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(part_path)
        .map_err(|error| {
            io_error(
                format!("failed to open part file {}", part_path.display()),
                error,
            )
        })?;
    while offset < expected_size {
        let length = chunk_size.min(expected_size - offset);
        match storage.read_range(remote_path, offset, length, &mut file) {
            Ok(0) => {
                return Err(RemoteError::new(
                    RemoteErrorKind::RemoteIntegrityFailed,
                    format!("download of {remote_path} made no progress at offset {offset}"),
                ));
            }
            Ok(written) => {
                offset += written;
                file.flush().map_err(|error| {
                    io_error(
                        format!("failed to flush part file {}", part_path.display()),
                        error,
                    )
                })?;
                on_progress(offset);
            }
            Err(error) => {
                let durable = fs::metadata(part_path)
                    .map(|meta| meta.len())
                    .unwrap_or(offset)
                    .min(expected_size);
                on_progress(durable);
                if durable == 0 {
                    let _ = fs::remove_file(part_path);
                }
                return Err(error);
            }
        }
    }
    Ok(())
}

fn download_object_whole(
    storage: &dyn RepositoryStorage,
    remote_path: &str,
    part_path: &Path,
    on_progress: &mut dyn FnMut(u64),
) -> RemoteResult<()> {
    let _ = fs::remove_file(part_path);
    let mut file = fs::File::create(part_path).map_err(|error| {
        io_error(
            format!("failed to create part file {}", part_path.display()),
            error,
        )
    })?;
    match storage.read(remote_path, &mut file) {
        Ok(written) => {
            on_progress(written);
            Ok(())
        }
        Err(error) => {
            // Without range reads a partial file can never resume.
            let _ = fs::remove_file(part_path);
            Err(error)
        }
    }
}

/// Check a downloaded or pre-existing file against the expected size and
/// digest. When both expectations are absent (a legacy object with unknown
/// size) the file is only measured, never rejected.
pub(crate) fn verify_file(
    path: &Path,
    expected_size: Option<u64>,
    expected_digest: Option<&str>,
) -> RemoteResult<VerifiedFile> {
    let size = fs::metadata(path).map(|meta| meta.len()).map_err(|error| {
        io_error(
            format!("failed to stat candidate {}", path.display()),
            error,
        )
    })?;
    if let Some(expected_size) = expected_size {
        if size != expected_size {
            return Err(integrity_error(format!(
                "downloaded file size {size} does not match expected {expected_size}"
            )));
        }
    }
    let digest = sha256_file(path)?;
    if let Some(expected_digest) = expected_digest {
        if !digest.eq_ignore_ascii_case(expected_digest) {
            return Err(integrity_error(format!(
                "downloaded file digest {digest} does not match expected {expected_digest}"
            )));
        }
    }
    Ok(VerifiedFile { size, digest })
}

/// Atomically move a verified candidate over its destination.
pub(crate) fn install_verified(candidate: &Path, destination: &Path) -> RemoteResult<()> {
    fsync_file(candidate)?;
    fs::rename(candidate, destination).map_err(|error| {
        io_error(
            format!(
                "failed to install verified file as {}",
                destination.display()
            ),
            error,
        )
    })?;
    fsync_parent(destination)?;
    Ok(())
}

pub(crate) fn sha256_file(path: &Path) -> RemoteResult<String> {
    let mut file = fs::File::open(path).map_err(|error| {
        io_error(
            format!("failed to open {} for digest", path.display()),
            error,
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        use std::io::Read;
        let read = file.read(&mut buffer).map_err(|error| {
            io_error(
                format!("failed to read {} for digest", path.display()),
                error,
            )
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub(crate) fn fsync_file(path: &Path) -> RemoteResult<()> {
    let file = fs::File::open(path).map_err(|error| {
        io_error(
            format!("failed to open {} for fsync", path.display()),
            error,
        )
    })?;
    file.sync_all()
        .map_err(|error| io_error(format!("failed to fsync {}", path.display()), error))
}

pub(crate) fn fsync_parent(path: &Path) -> RemoteResult<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    #[cfg(unix)]
    {
        let dir = fs::File::open(parent).map_err(|error| {
            io_error(
                format!("failed to open {} for fsync", parent.display()),
                error,
            )
        })?;
        dir.sync_all().map_err(|error| {
            io_error(
                format!("failed to fsync directory {}", parent.display()),
                error,
            )
        })?;
    }
    #[cfg(not(unix))]
    {
        // Directory fsync is not meaningful on Windows.
        let _ = parent;
    }
    Ok(())
}

pub(crate) fn integrity_error(detail: impl Into<String>) -> RemoteError {
    RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, detail)
}

pub(crate) fn io_error(context: String, error: std::io::Error) -> RemoteError {
    RemoteError::new(kind_from_io_error(&error), format!("{context}: {error}"))
}
