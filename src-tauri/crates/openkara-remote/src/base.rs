//! True merge-base retention for song-scoped publish rebase (ADR 0040).
//!
//! The Local Working Copy keeps the base database snapshot — the snapshot at
//! `expected_generation` it last pulled — until Publish Changes commits. The
//! remote cannot serve as the base source: remote GC keeps only the last two
//! generations. The outbox records `expected_generation` and `source_db_digest`
//! per operation, but the digest alone cannot rebuild the base bytes once the
//! working copy is mutated, so the bytes live in one file per working copy.
//!
//! Lifecycle: refresh installs a generation and retains it; mutation prepare
//! retains the pre-mutation working copy when no base is present (legacy
//! working copies, or a base cleared by a commit); a successful publish commit
//! removes the base (the working copy now matches the committed generation).
//! Every file turnover is atomic (temp file plus rename). A rebase verifies
//! the retained bytes against the outbox `source_db_digest` before merging
//! and fails closed with a conflict when the base is missing or mismatched.

use std::fs;
use std::path::{Path, PathBuf};

use crate::download::{fsync_parent, io_error, sha256_file};
use crate::error::{RemoteError, RemoteErrorKind, RemoteResult};

/// File name of the retained base snapshot under the working copy state dir.
const BASE_SNAPSHOT_FILE_NAME: &str = "base-snapshot.sqlite";

/// The one retained base snapshot file for a working copy.
pub fn base_snapshot_path(working_copy_root: &Path) -> PathBuf {
    working_copy_root
        .join(".openkara")
        .join(BASE_SNAPSHOT_FILE_NAME)
}

/// Atomically retain `source_db` as the working copy's base snapshot.
///
/// Writes to a temp sibling and renames, so a crash never leaves a torn base.
pub fn retain_base_snapshot(working_copy_root: &Path, source_db: &Path) -> RemoteResult<()> {
    let destination = base_snapshot_path(working_copy_root);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            io_error(
                format!("failed to create base snapshot dir {}", parent.display()),
                error,
            )
        })?;
    }
    let temp = destination.with_extension(format!("tmp-{}", std::process::id()));
    let _ = fs::remove_file(&temp);
    fs::copy(source_db, &temp).map_err(|error| {
        io_error(
            format!("failed to stage base snapshot from {}", source_db.display()),
            error,
        )
    })?;
    fs::rename(&temp, &destination).map_err(|error| {
        let _ = fs::remove_file(&temp);
        io_error(
            format!(
                "failed to install base snapshot as {}",
                destination.display()
            ),
            error,
        )
    })?;
    fsync_parent(&destination)?;
    Ok(())
}

/// Retain the pre-mutation working database as the base when no base is
/// present. Base retention is best-effort here: a working copy that predates
/// base retention (or whose base a commit cleared) records its next base the
/// first time it mutates, so the following publish can prove disjointness.
pub fn retain_base_snapshot_if_missing(
    working_copy_root: &Path,
    working_database: &Path,
) -> RemoteResult<bool> {
    if base_snapshot_path(working_copy_root).exists() {
        return Ok(false);
    }
    retain_base_snapshot(working_copy_root, working_database)?;
    Ok(true)
}

/// Remove the retained base after a successful publish commit. Best-effort:
/// a leftover base only makes a later rebase re-verify bytes that still match.
pub fn clear_base_snapshot(working_copy_root: &Path) {
    let _ = fs::remove_file(base_snapshot_path(working_copy_root));
}

/// Resolve the base snapshot for a rebase and verify it against the outbox
/// `source_db_digest`. Fails closed with a conflict — never a silent
/// overwrite — when the base is missing, unreadable, or mismatched.
///
/// `explicit` is the caller-supplied path (PublishRequest); `None` falls back
/// to the working copy default. `expected_digest` must be `Some`: a rebase
/// without a recorded base digest cannot prove disjointness.
pub fn load_verified_base_snapshot(
    working_copy_root: &Path,
    explicit: Option<&Path>,
    expected_digest: Option<&str>,
    expected_generation: i64,
    current_generation: i64,
) -> RemoteResult<PathBuf> {
    let generations = format!(
        "expected generation {expected_generation} but the remote repository is at \
         {current_generation}; another device published first"
    );
    let conflict = |detail: String| {
        RemoteError::new(
            RemoteErrorKind::RemoteConflict,
            format!("automatic song-scoped rebase rejected: {detail} ({generations})"),
        )
    };
    let Some(expected_digest) = expected_digest else {
        return Err(conflict(
            "no recorded base digest for the local operation; cannot prove disjointness".to_owned(),
        ));
    };
    let path = explicit
        .map(Path::to_path_buf)
        .unwrap_or_else(|| base_snapshot_path(working_copy_root));
    if !path.is_file() {
        return Err(conflict(format!(
            "no retained base snapshot at {}; refresh the repository and publish again",
            path.display()
        )));
    }
    let actual = sha256_file(&path).map_err(|error| {
        conflict(format!(
            "retained base snapshot at {} is unreadable: {}",
            path.display(),
            error.detail.as_deref().unwrap_or("unknown error")
        ))
    })?;
    if !actual.eq_ignore_ascii_case(expected_digest) {
        return Err(conflict(
            "retained base snapshot does not match the operation's recorded base digest; \
             refresh the repository and publish again"
                .to_owned(),
        ));
    }
    Ok(path)
}
