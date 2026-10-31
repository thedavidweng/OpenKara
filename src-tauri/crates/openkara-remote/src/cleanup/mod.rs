//! Cleanup of unreferenced uploads: the desktop's garbage collection for a
//! Remote Repository.
//!
//! Publish Changes leaves objects behind when it is interrupted or loses the
//! manifest race: asset uploads and database snapshots that no manifest ever
//! named. Superseded generations accumulate too. Cleanup keeps the snapshots
//! of recent generations, keeps every asset a kept snapshot references, and
//! deletes the rest of the managed objects once they are older than a grace
//! period.
//!
//! The grace period protects two kinds of readers that cleanup cannot see:
//!
//! - A phone (or another desktop) that read the manifest just before it moved
//!   and is still downloading that generation's snapshot or its songs. A
//!   generation stays kept until it has been superseded for the grace period.
//! - A publication in progress on another device, whose uploads no manifest
//!   names yet. An unreferenced object is deleted only when it was last
//!   written before the grace period began, by the storage's own clock.
//!
//! When a generation stopped being current is not recorded anywhere, so it
//! is bounded from above: generation `g` was superseded when generation
//! `g + 1` was committed, which happened before any publisher could upload a
//! snapshot for generation `g + 2` (a publisher only targets the generation
//! after the one it observed in the manifest), and no later than the commit
//! of the current manifest. Missing modification times only ever keep more.
//!
//! Cleanup is a writer's action: it refuses storages that cannot publish, so
//! a phone, which opens its Remote Repository through a
//! [`ReadOnlyStorage`](crate::storage::ReadOnlyStorage), never deletes remote
//! objects. Only the managed asset directories and the database snapshot
//! directory are ever touched; the manifest, a legacy root `openkara.db`, and
//! objects cleanup does not recognize are left alone. A repository without a
//! manifest is never cleaned.
//!
//! The port offers no conditional delete. Right before each delete, cleanup
//! re-reads the manifest revision and the object's revision: once the
//! manifest has moved it deletes nothing more, and an object rewritten after
//! the listing is skipped. One race stays open. A publication that finds an
//! old unreferenced asset already present skips its upload, and cleanup can
//! delete that asset between the publisher's check and its manifest commit.
//! The asset then stays missing until the next Publish Changes from that
//! device, which uploads it again because every publication checks every
//! asset it references.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::download::{download_object, io_error, verify_file};
use crate::error::{RemoteError, RemoteErrorKind, RemoteResult};
use crate::manifest::{RepositoryManifest, MANIFEST_PATH};
use crate::reader::SNAPSHOT_DOWNLOAD_CHUNK_SIZE;
use crate::storage::{ListedObject, RepositoryStorage};
use crate::writer::{read_current_manifest, referenced_asset_paths, ASSET_TOP_LEVEL_DIRS};

/// The current generation and the one before it.
pub const DEFAULT_KEPT_GENERATIONS: i64 = 2;

/// Long enough for a phone to finish reading a generation that was current
/// when it refreshed, and for a slow publication on another device to commit.
pub const DEFAULT_CLEANUP_GRACE_PERIOD_MS: i64 = 24 * 60 * 60 * 1000;

const DATABASES_PREFIX: &str = ".openkara/databases/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupPolicy {
    /// How many of the newest generations keep their snapshots regardless of
    /// age, the current one included. Values below 1 count as 1.
    pub kept_generations: i64,
    pub grace_period_ms: i64,
}

impl Default for CleanupPolicy {
    fn default() -> Self {
        Self {
            kept_generations: DEFAULT_KEPT_GENERATIONS,
            grace_period_ms: DEFAULT_CLEANUP_GRACE_PERIOD_MS,
        }
    }
}

/// What one [`clean_up_unreferenced_uploads`] call observed and did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupReport {
    /// The generations whose snapshots and referenced assets were kept,
    /// oldest first. Empty for a repository without a manifest.
    pub kept_generations: Vec<i64>,
    pub deleted: Vec<String>,
    /// Unreferenced objects left for a later cleanup: written within the
    /// grace period, rewritten since the listing, or not yet deleted when the
    /// manifest moved.
    pub deferred: Vec<String>,
    /// Unreferenced objects the storage reported no modification time for.
    /// Cleanup cannot prove they are old, so it never deletes them.
    pub undated: Vec<String>,
}

/// Delete the managed objects that no kept generation references.
///
/// `scratch_dir` receives temporary copies of the kept snapshots while their
/// references are read; they are removed before returning. `now_ms` is the
/// caller's Unix time in milliseconds; the desktop and storage clocks are
/// assumed to agree within the grace period.
///
/// Every kept snapshot must download and parse before anything is deleted:
/// a transfer error propagates unchanged and an unreadable or corrupt kept
/// snapshot fails with `RemoteIntegrityFailed`, deleting nothing.
pub fn clean_up_unreferenced_uploads(
    storage: &dyn RepositoryStorage,
    scratch_dir: &Path,
    policy: &CleanupPolicy,
    now_ms: i64,
) -> RemoteResult<CleanupReport> {
    if !storage.capabilities().can_publish() {
        return Err(RemoteError::new(
            RemoteErrorKind::ProviderCapabilityUnavailable,
            "cleanup is part of publication; this storage cannot publish, so it is never cleaned",
        ));
    }
    let Some((manifest, manifest_revision)) = read_current_manifest(storage)? else {
        return Ok(CleanupReport::default());
    };

    let listing = storage.list("")?;
    let manifest_written_at = listing
        .iter()
        .find(|object| object.path == MANIFEST_PATH)
        .and_then(|object| object.modified_at_ms);
    let mut snapshots: BTreeMap<i64, Vec<&ListedObject>> = BTreeMap::new();
    let mut assets = Vec::new();
    for object in &listing {
        if let Some(generation) = snapshot_generation(&object.path) {
            snapshots.entry(generation).or_default().push(object);
        } else if is_managed_asset(&object.path) {
            assets.push(object);
        }
    }

    let horizon = now_ms.saturating_sub(policy.grace_period_ms);
    let oldest_kept = oldest_kept_generation(
        &manifest,
        manifest_written_at,
        &snapshots,
        policy.kept_generations,
        horizon,
    );

    let mut kept_snapshots = vec![KeptSnapshot {
        path: manifest.database_path.clone(),
        expected_size: Some(manifest.database_size_bytes),
        expected_digest: Some(manifest.database_sha256.clone()),
    }];
    let mut candidates = Vec::new();
    for (generation, objects) in &snapshots {
        for object in objects {
            if object.path == manifest.database_path {
                continue;
            }
            // Which snapshot of an older generation its manifest named is not
            // recorded, so every snapshot of a kept older generation stays.
            if (oldest_kept..manifest.generation).contains(generation) {
                kept_snapshots.push(KeptSnapshot {
                    path: object.path.clone(),
                    expected_size: Some(object.metadata.size_bytes),
                    expected_digest: None,
                });
            } else {
                candidates.push(*object);
            }
        }
    }

    let referenced = referenced_assets(storage, scratch_dir, &kept_snapshots)?;
    candidates.extend(
        assets
            .into_iter()
            .filter(|object| !referenced.contains(&object.path)),
    );
    candidates.sort_by(|a, b| a.path.cmp(&b.path));

    let mut report = CleanupReport {
        kept_generations: (oldest_kept..=manifest.generation).collect(),
        ..CleanupReport::default()
    };
    let mut deletable = Vec::new();
    for object in candidates {
        match object.modified_at_ms {
            None => report.undated.push(object.path.clone()),
            Some(written_at) if written_at > horizon => report.deferred.push(object.path.clone()),
            Some(_) => deletable.push(object),
        }
    }

    let mut deletable = deletable.into_iter();
    while let Some(object) = deletable.next() {
        let current_revision = storage
            .stat(MANIFEST_PATH)?
            .and_then(|metadata| metadata.revision);
        if current_revision != manifest_revision {
            report.deferred.push(object.path.clone());
            report
                .deferred
                .extend(deletable.map(|object| object.path.clone()));
            break;
        }
        match storage.stat(&object.path)? {
            None => {}
            Some(metadata) if metadata.revision != object.metadata.revision => {
                report.deferred.push(object.path.clone());
            }
            Some(_) => {
                storage.delete(&object.path)?;
                report.deleted.push(object.path.clone());
            }
        }
    }
    report.deferred.sort();
    Ok(report)
}

struct KeptSnapshot {
    path: String,
    expected_size: Option<u64>,
    expected_digest: Option<String>,
}

/// The oldest generation a reader may still be using: the newest
/// `kept_generations` generations, plus every older one that might have been
/// superseded after `horizon`.
fn oldest_kept_generation(
    manifest: &RepositoryManifest,
    manifest_written_at: Option<i64>,
    snapshots: &BTreeMap<i64, Vec<&ListedObject>>,
    kept_generations: i64,
    horizon: i64,
) -> i64 {
    let current = manifest.generation;
    let window_floor = current - kept_generations.max(1) + 1;
    // Upper bound on when `generation` stopped being current: the commit of
    // `generation + 1`.
    let mut superseded_by = manifest
        .committed_at_ms
        .max(manifest_written_at.unwrap_or(i64::MIN));
    let mut oldest = current;
    let mut generation = current - 1;
    while generation >= 1 {
        if generation < window_floor && superseded_by <= horizon {
            break;
        }
        oldest = generation;
        let earliest_later_upload = snapshots
            .get(&(generation + 1))
            .into_iter()
            .flatten()
            .filter_map(|object| object.modified_at_ms)
            .min();
        if let Some(written_at) = earliest_later_upload {
            superseded_by = superseded_by.min(written_at);
        }
        generation -= 1;
    }
    oldest
}

fn referenced_assets(
    storage: &dyn RepositoryStorage,
    scratch_dir: &Path,
    snapshots: &[KeptSnapshot],
) -> RemoteResult<BTreeSet<String>> {
    fs::create_dir_all(scratch_dir).map_err(|error| {
        io_error(
            format!("failed to create directory {}", scratch_dir.display()),
            error,
        )
    })?;
    let mut referenced = BTreeSet::new();
    for (index, snapshot) in snapshots.iter().enumerate() {
        let copy = ScratchFile(scratch_dir.join(format!("cleanup-snapshot-{index}.sqlite")));
        let _ = fs::remove_file(&copy.0);
        download_object(
            storage,
            &snapshot.path,
            snapshot.expected_size,
            &copy.0,
            SNAPSHOT_DOWNLOAD_CHUNK_SIZE,
            &mut |_| {},
        )?;
        verify_file(
            &copy.0,
            snapshot.expected_size,
            snapshot.expected_digest.as_deref(),
        )
        .map_err(|error| kept_snapshot_unreadable(&snapshot.path, &error))?;
        let paths = referenced_asset_paths(&copy.0, None)
            .map_err(|error| kept_snapshot_unreadable(&snapshot.path, &error))?;
        referenced.extend(paths);
    }
    Ok(referenced)
}

fn kept_snapshot_unreadable(path: &str, error: &RemoteError) -> RemoteError {
    RemoteError::new(
        RemoteErrorKind::RemoteIntegrityFailed,
        format!("cleanup cannot read the kept snapshot {path}, so it deletes nothing: {error}"),
    )
}

/// Removes the temporary snapshot copy however the cleanup ends.
struct ScratchFile(PathBuf);

impl Drop for ScratchFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// The generation of a database snapshot object: the legacy
/// `.openkara/databases/<generation>.sqlite` or the operation-scoped
/// `.openkara/databases/<generation>/<hash>.sqlite`.
fn snapshot_generation(path: &str) -> Option<i64> {
    let rest = path.strip_prefix(DATABASES_PREFIX)?;
    let generation = match rest.split_once('/') {
        Some((generation, file)) if !file.contains('/') && file.ends_with(".sqlite") => generation,
        Some(_) => return None,
        None => rest.strip_suffix(".sqlite")?,
    };
    let parsed = generation.parse::<i64>().ok()?;
    (parsed >= 1 && parsed.to_string() == generation).then_some(parsed)
}

fn is_managed_asset(path: &str) -> bool {
    path.split_once('/')
        .is_some_and(|(top_level, _)| ASSET_TOP_LEVEL_DIRS.contains(&top_level))
}

#[cfg(test)]
mod tests;
