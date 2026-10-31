//! The base snapshots a Local Working Copy retains for rebasing (ADR 0040).
//!
//! The base of generation `G` is the database snapshot that generation
//! committed. A rebase compares the local changes against it, so a wrong base
//! would make overlapping changes look disjoint. Every base therefore has an
//! identity, its SHA-256, which the application records when the base is
//! retained and hands back with each publication; a retained file is only
//! trusted when its bytes still hash to that identity.
//!
//! Bases are written atomically and never pruned implicitly: the application
//! prunes the bases it no longer needs once the generation that replaced them
//! is durably recorded, so a crash at any point keeps the base a retry needs.

use std::fs;
use std::path::{Path, PathBuf};

use crate::download::{install_verified, io_error, sha256_file};
use crate::error::RemoteResult;

/// The single base snapshot file that working copies kept before bases were
/// retained per generation.
const LEGACY_BASE_SNAPSHOT: &str = ".openkara/base-snapshot.sqlite";

/// Where the Local Working Copy keeps the database snapshot of
/// `generation`, the base a later rebase compares against.
pub fn base_snapshot_path(working_copy_root: &Path, generation: i64) -> PathBuf {
    base_directory(working_copy_root).join(format!("{generation}.sqlite"))
}

/// Keep a copy of `source` as the base snapshot of `generation` and return
/// the SHA-256 of the retained bytes, the identity to record for it.
pub fn retain_base_snapshot(
    working_copy_root: &Path,
    generation: i64,
    source: &Path,
) -> RemoteResult<String> {
    let directory = base_directory(working_copy_root);
    fs::create_dir_all(&directory).map_err(|error| {
        io_error(
            format!("failed to create directory {}", directory.display()),
            error,
        )
    })?;
    let staging = directory.join(format!("{generation}.sqlite.tmp"));
    fs::copy(source, &staging).map_err(|error| {
        io_error(
            format!(
                "failed to copy {} to base snapshot {}",
                source.display(),
                staging.display()
            ),
            error,
        )
    })?;
    let digest = sha256_file(&staging)?;
    install_verified(&staging, &base_snapshot_path(working_copy_root, generation))?;
    Ok(digest)
}

/// The retained base snapshot of `generation`, when its bytes hash to
/// `expected_sha256`. A missing identity, a missing file and different bytes
/// all mean there is no trustworthy base.
pub fn verified_base_snapshot(
    working_copy_root: &Path,
    generation: i64,
    expected_sha256: Option<&str>,
) -> RemoteResult<Option<PathBuf>> {
    let Some(expected_sha256) = expected_sha256 else {
        return Ok(None);
    };
    let path = base_snapshot_path(working_copy_root, generation);
    if !path.is_file() {
        return Ok(None);
    }
    let digest = sha256_file(&path)?;
    Ok(digest.eq_ignore_ascii_case(expected_sha256).then_some(path))
}

/// Remove every retained base except the one of `keep_generation`, plus the
/// single base file of older working copies. Call it only once the state
/// that names `keep_generation` as the base is durably recorded.
pub fn prune_base_snapshots(working_copy_root: &Path, keep_generation: i64) {
    let keep = base_snapshot_path(working_copy_root, keep_generation);
    if let Ok(entries) = fs::read_dir(base_directory(working_copy_root)) {
        for entry in entries.flatten() {
            if entry.path() != keep {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let _ = fs::remove_file(working_copy_root.join(LEGACY_BASE_SNAPSHOT));
}

/// What [`adopt_legacy_base_snapshot`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyBaseAdoption {
    /// The working copy has no single base file.
    Absent,
    /// The base of the generation was already retained; nothing was copied.
    AlreadyRetained,
    /// The single base file hashed to the recorded identity and is now the
    /// retained base of the generation.
    Adopted,
    /// The single base file does not hash to the recorded identity, or no
    /// identity was recorded. It stays where it is and is not used.
    Unverified,
}

/// Move a working copy that kept one base file into the per-generation
/// layout.
///
/// The old file names no generation, and older releases could capture it
/// from a working copy that already held unpublished edits. It is adopted as
/// the base of `generation` only when its bytes hash to `trusted_sha256`, the
/// identity recorded when that generation was installed. The old file is
/// left in place until [`prune_base_snapshots`] runs after the next durable
/// completion.
pub fn adopt_legacy_base_snapshot(
    working_copy_root: &Path,
    generation: i64,
    trusted_sha256: Option<&str>,
) -> RemoteResult<LegacyBaseAdoption> {
    let legacy = working_copy_root.join(LEGACY_BASE_SNAPSHOT);
    if !legacy.is_file() {
        return Ok(LegacyBaseAdoption::Absent);
    }
    if verified_base_snapshot(working_copy_root, generation, trusted_sha256)?.is_some() {
        return Ok(LegacyBaseAdoption::AlreadyRetained);
    }
    let Some(trusted_sha256) = trusted_sha256 else {
        return Ok(LegacyBaseAdoption::Unverified);
    };
    if !sha256_file(&legacy)?.eq_ignore_ascii_case(trusted_sha256) {
        return Ok(LegacyBaseAdoption::Unverified);
    }
    let retained = retain_base_snapshot(working_copy_root, generation, &legacy)?;
    Ok(if retained.eq_ignore_ascii_case(trusted_sha256) {
        LegacyBaseAdoption::Adopted
    } else {
        let _ = fs::remove_file(base_snapshot_path(working_copy_root, generation));
        LegacyBaseAdoption::Unverified
    })
}

fn base_directory(working_copy_root: &Path) -> PathBuf {
    working_copy_root.join(".openkara/base")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn working_copy() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::create_dir_all(dir.path().join(".openkara")).expect("state dir");
        dir
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        fs::write(path, bytes).expect("write");
    }

    fn digest(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        crate::download::hex_lower(&Sha256::digest(bytes))
    }

    #[test]
    fn a_retained_base_is_verified_by_its_recorded_identity() {
        let dir = working_copy();
        let source = dir.path().join("source.sqlite");
        write(&source, b"generation one");

        let identity = retain_base_snapshot(dir.path(), 1, &source).expect("retain");

        assert_eq!(identity, digest(b"generation one"));
        assert_eq!(
            verified_base_snapshot(dir.path(), 1, Some(&identity)).expect("verify"),
            Some(base_snapshot_path(dir.path(), 1))
        );
        assert_eq!(
            verified_base_snapshot(dir.path(), 1, Some(&identity.to_ascii_uppercase()))
                .expect("verify"),
            Some(base_snapshot_path(dir.path(), 1)),
            "the identity compares case-insensitively"
        );
    }

    #[test]
    fn a_base_without_identity_missing_or_with_other_bytes_is_not_trusted() {
        let dir = working_copy();
        let source = dir.path().join("source.sqlite");
        write(&source, b"generation one");
        let identity = retain_base_snapshot(dir.path(), 1, &source).expect("retain");

        assert_eq!(verified_base_snapshot(dir.path(), 1, None).unwrap(), None);
        assert_eq!(
            verified_base_snapshot(dir.path(), 2, Some(&identity)).unwrap(),
            None,
            "no base of that generation"
        );
        write(&base_snapshot_path(dir.path(), 1), b"generation two");
        assert_eq!(
            verified_base_snapshot(dir.path(), 1, Some(&identity)).unwrap(),
            None,
            "same-size different bytes"
        );
    }

    #[test]
    fn retaining_a_base_keeps_the_other_generations_until_pruned() {
        let dir = working_copy();
        let source = dir.path().join("source.sqlite");
        write(&source, b"one");
        retain_base_snapshot(dir.path(), 1, &source).expect("retain 1");
        write(&source, b"two");
        retain_base_snapshot(dir.path(), 2, &source).expect("retain 2");
        write(&dir.path().join(LEGACY_BASE_SNAPSHOT), b"legacy");

        assert!(base_snapshot_path(dir.path(), 1).is_file());
        prune_base_snapshots(dir.path(), 2);

        assert!(!base_snapshot_path(dir.path(), 1).exists());
        assert_eq!(fs::read(base_snapshot_path(dir.path(), 2)).unwrap(), b"two");
        assert!(!dir.path().join(LEGACY_BASE_SNAPSHOT).exists());
    }

    #[test]
    fn a_legacy_base_with_the_recorded_identity_is_adopted_and_kept_until_pruned() {
        let dir = working_copy();
        let legacy = dir.path().join(LEGACY_BASE_SNAPSHOT);
        write(&legacy, b"clean generation four");
        // An earlier adoption that crashed mid-copy left its staging file.
        write(
            &dir.path().join(".openkara/base/4.sqlite.tmp"),
            b"torn copy",
        );

        let adoption = adopt_legacy_base_snapshot(
            dir.path(),
            4,
            Some(&digest(b"clean generation four").to_ascii_uppercase()),
        )
        .expect("adopt");

        assert_eq!(adoption, LegacyBaseAdoption::Adopted);
        assert_eq!(
            fs::read(base_snapshot_path(dir.path(), 4)).unwrap(),
            b"clean generation four"
        );
        assert!(
            legacy.is_file(),
            "the old file stays until a durable completion"
        );
        assert_eq!(
            adopt_legacy_base_snapshot(dir.path(), 4, Some(&digest(b"clean generation four")))
                .unwrap(),
            LegacyBaseAdoption::AlreadyRetained
        );
        prune_base_snapshots(dir.path(), 4);
        assert!(!legacy.exists());
        assert!(base_snapshot_path(dir.path(), 4).is_file());
    }

    #[test]
    fn a_legacy_base_that_is_not_the_recorded_ancestor_is_never_adopted() {
        let dir = working_copy();
        let legacy = dir.path().join(LEGACY_BASE_SNAPSHOT);
        write(&legacy, b"captured with unpublished edits");

        assert_eq!(
            adopt_legacy_base_snapshot(dir.path(), 4, Some(&digest(b"clean generation four")))
                .unwrap(),
            LegacyBaseAdoption::Unverified
        );
        assert_eq!(
            adopt_legacy_base_snapshot(dir.path(), 4, None).unwrap(),
            LegacyBaseAdoption::Unverified,
            "no recorded identity"
        );
        assert!(!base_snapshot_path(dir.path(), 4).exists());
        assert_eq!(
            fs::read(&legacy).unwrap(),
            b"captured with unpublished edits"
        );
        assert_eq!(
            adopt_legacy_base_snapshot(tempfile::tempdir().unwrap().path(), 4, None).unwrap(),
            LegacyBaseAdoption::Absent
        );
    }
}
