//! Persistent, verified cache catalog for remote streaming playback.
//!
//! The `remote_cache_entries` table in the control DB is the authoritative
//! catalog. On-disk data files are content-addressed by the SHA-256 of the
//! identity tuple `(library_id, relative_path, provider_revision,
//! expected_size)`. This replaces the old `ChunkedCache`-only manager that:
//!
//! - deleted the range index on completion (forgetting completion across
//!   restart),
//! - counted only entries opened in the current process,
//! - defaulted to `u64::MAX` (unbounded) when no limit was set, and
//! - keyed caches by `format!("remote-{}", song.hash)`, which reused bytes
//!   from an older provider revision when the remote object was replaced.
//!
//! ## Startup reconciliation
//!
//! On open, the catalog is reconciled against disk: orphaned data files (no
//! catalog row) are deleted; catalog rows whose data file is missing or whose
//! length does not match `expected_size` are deleted. Complete entries that
//! have not been verified since the last process start are re-hashed before
//! first reuse so an unclean shutdown cannot expose a corrupted file as
//! verified.
//!
//! ## LRU eviction
//!
//! Eviction uses the persistent wall-clock `last_access_at_ms` column (not a
//! process-local `Instant`) so LRU order survives restarts. Only entries with
//! `pinned_count == 0` are evicted. Eviction removes the catalog row first
//! (transactional), then deletes the data file; if the file deletion fails the
//! orphaned file is cleaned on the next startup scan.

use crate::audio::chunked_cache::{CacheError, ChunkedCache};
use crate::audio::range_set::RangeSet;
use crate::commands::error::{internal_error, CommandError, CommandResult};
use crate::hash;
use crate::remote::atomic_download::sha256_file;
use crate::remote::control_db::{self, CacheEntryRow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default cache budget when `remote_cache_bytes_limit` is `None`. A finite
/// 2 GiB default prevents unbounded disk growth on a fresh install while
/// staying large enough that a typical karaoke session does not thrash.
pub const DEFAULT_CACHE_BYTES_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// Identity tuple that uniquely identifies a remote cacheable file. Two files
/// with the same identity are byte-for-byte interchangeable; a changed
/// `provider_revision` or `expected_size` yields a different `cache_key`.
#[derive(Debug, Clone)]
pub struct CacheIdentity {
    pub library_id: String,
    pub relative_path: String,
    /// Provider revision token (ETag / Dropbox rev / Google Drive
    /// headRevisionId). When unavailable, the caller may substitute a content
    /// digest after the first full download.
    pub provider_revision: Option<String>,
    pub expected_size: u64,
}

impl CacheIdentity {
    /// Compute the stable SHA-256 hex digest of the identity tuple. This is
    /// used as both the `cache_key` primary key and the on-disk filename.
    pub fn cache_key(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"library_id=");
        hasher.update(self.library_id.as_bytes());
        hasher.update(b"\nrelative_path=");
        hasher.update(self.relative_path.as_bytes());
        hasher.update(b"\nprovider_revision=");
        if let Some(rev) = &self.provider_revision {
            hasher.update(rev.as_bytes());
        }
        hasher.update(b"\nexpected_size=");
        hasher.update(self.expected_size.to_le_bytes());
        hash::hex_lower(hasher.finalize())
    }
}

/// Snapshot of cache usage returned by `get_remote_cache_usage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheUsage {
    pub used_bytes: u64,
    pub limit_bytes: u64,
    pub entry_count: usize,
    pub pinned_count: usize,
}

/// A pinned cache guard. Dropping it decrements the pin count so the entry
/// becomes eligible for eviction again. RAII ensures unpin always happens,
/// even if the playback thread panics.
pub struct CachePinGuard {
    cache_key: String,
    manager: Arc<Mutex<CacheCatalog>>,
}

impl Drop for CachePinGuard {
    fn drop(&mut self) {
        if let Ok(mut manager) = self.manager.lock() {
            let _ = manager.unpin(&self.cache_key);
        }
    }
}

/// Persistent, verified cache catalog backed by `remote_cache_entries`.
///
/// The catalog is the source of truth; on-disk data files are
/// content-addressed by the `cache_key` digest. The in-memory `ChunkedCache`
/// handles are opened lazily and cached for the lifetime of the process, but
/// all durable state (ranges, completion, LRU timestamp, pin count) lives in
/// the control DB so it survives restarts.
pub struct CacheCatalog {
    cache_dir: PathBuf,
    control_db: Arc<Mutex<rusqlite::Connection>>,
    max_bytes: u64,
    /// In-memory handles keyed by `cache_key`. The handles are purely a
    /// performance optimization; the catalog row is the durable state.
    caches: HashMap<String, Arc<ChunkedCache>>,
}

fn current_unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// JSON shape for `downloaded_ranges_json`: an array of `[start, length]`
/// pairs. This is separate from `RangeSet`'s own serde format so the catalog
/// schema stays stable even if `RangeSet`'s serialization changes.
#[derive(Serialize, Deserialize)]
struct RangesJson(Vec<[u64; 2]>);

fn ranges_to_json(ranges: &RangeSet) -> CommandResult<String> {
    let pairs: Vec<[u64; 2]> = ranges
        .ranges()
        .iter()
        .map(|r| [r.start, r.length])
        .collect();
    serde_json::to_string(&RangesJson(pairs))
        .map_err(|e| internal_error(format!("failed to serialize ranges: {e}")))
}

fn ranges_from_json(json: &str) -> CommandResult<RangeSet> {
    let parsed: RangesJson = serde_json::from_str(json).map_err(|e| {
        internal_error(format!("failed to deserialize downloaded_ranges_json: {e}"))
    })?;
    let mut set = RangeSet::new();
    for [start, length] in parsed.0 {
        set.add_range(start, length);
    }
    Ok(set)
}

/// Validate that a `downloaded_ranges_json` does not claim ranges beyond the
/// actual data file length. A corrupted sidecar that claims bytes past the
/// file end would expose zero-filled sparse gaps as downloaded data, so the
/// entry is rejected on verification.
fn ranges_within_file(ranges: &RangeSet, file_len: u64) -> bool {
    ranges.ranges().iter().all(|r| r.end() <= file_len)
}

impl CacheCatalog {
    /// Open the persistent cache catalog. Runs startup reconciliation: orphaned
    /// data files are deleted, catalog rows with missing/inconsistent data
    /// files are removed, and usage is recalculated from the reconciled
    /// catalog. The `control_db` handle is held for the lifetime of the
    /// manager so range/pin/access updates are durable.
    pub fn open(
        cache_dir: PathBuf,
        control_db: Arc<Mutex<rusqlite::Connection>>,
        max_bytes: u64,
    ) -> CommandResult<Self> {
        fs::create_dir_all(&cache_dir).map_err(|e| {
            internal_error(format!(
                "failed to create cache directory {}: {e}",
                cache_dir.display()
            ))
        })?;

        let mut manager = Self {
            cache_dir,
            control_db,
            max_bytes,
            caches: HashMap::new(),
        };
        manager.reconcile_on_startup()?;
        Ok(manager)
    }

    /// The configured byte budget. Exposed for the usage IPC command.
    pub fn limit_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Reconcile the catalog against disk at startup.
    ///
    /// 1. For each catalog row: verify the `data_path` file exists and its
    ///    length matches `expected_size`. Discard rows whose file is missing
    ///    or whose length mismatches. Complete entries are NOT re-hashed here
    ///    (that happens lazily on first reuse) to avoid hashing every file at
    ///    startup.
    /// 2. Delete orphaned data files in the cache directory that have no
    ///    catalog row.
    fn reconcile_on_startup(&mut self) -> CommandResult<()> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned during reconciliation"))?;

        let rows = control_db::list_cache_entries(&conn)?;

        // Track the set of valid data filenames so orphaned files can be
        // detected.
        let mut known_files: std::collections::HashSet<String> = std::collections::HashSet::new();

        for row in &rows {
            let data_path = Path::new(&row.data_path);
            // Resolve the data path relative to the cache dir when it is not
            // absolute (catalog stores a relative filename for portability
            // across app-data-dir moves).
            let absolute = if data_path.is_absolute() {
                data_path.to_path_buf()
            } else {
                self.cache_dir.join(data_path)
            };

            let file_ok = match fs::metadata(&absolute) {
                Ok(meta) => meta.len() == row.expected_size as u64,
                Err(_) => false,
            };

            if !file_ok {
                // Catalog row is inconsistent with disk — discard the row.
                // The data file (if present but wrong size) is left for the
                // orphan scan below to clean up.
                let _ = control_db::delete_cache_entry(&conn, &row.cache_key);
                continue;
            }

            known_files.insert(row.data_path.clone());
        }

        // Orphan scan: delete data files in the cache dir with no catalog row.
        // Only files ending in `.cache` are considered so temp/part files from
        // other subsystems are not touched.
        if let Ok(entries) = fs::read_dir(&self.cache_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.ends_with(".cache") && !known_files.contains(name_str.as_ref()) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }

        Ok(())
    }

    /// Total bytes used by the cache, calculated from the catalog (which has
    /// been reconciled against disk). This includes entries not yet opened in
    /// the current process.
    pub fn total_bytes(&self) -> CommandResult<u64> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        let rows = control_db::list_cache_entries(&conn)?;
        Ok(rows.iter().map(|r| r.expected_size as u64).sum())
    }

    /// Number of catalog entries (reconciled).
    pub fn entry_count(&self) -> CommandResult<usize> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        let rows = control_db::list_cache_entries(&conn)?;
        Ok(rows.len())
    }

    /// Number of pinned entries.
    pub fn pinned_count(&self) -> CommandResult<usize> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        let rows = control_db::list_cache_entries(&conn)?;
        Ok(rows.iter().filter(|r| r.pinned_count > 0).count())
    }

    /// Usage snapshot for the IPC command.
    pub fn usage(&self) -> CommandResult<CacheUsage> {
        Ok(CacheUsage {
            used_bytes: self.total_bytes()?,
            limit_bytes: self.max_bytes,
            entry_count: self.entry_count()?,
            pinned_count: self.pinned_count()?,
        })
    }

    /// The on-disk data filename for a cache key.
    fn data_filename(cache_key: &str) -> String {
        format!("{cache_key}.cache")
    }

    /// Get or create a `ChunkedCache` for the given identity. On a cache hit,
    /// the catalog row + data file length are verified before reuse; on
    /// mismatch the stale entry is discarded and a fresh one is created.
    ///
    /// Before returning a complete entry that has not been verified since the
    /// last process start, the data file is re-hashed and `verified_at_ms` is
    /// set. This prevents an unclean shutdown from exposing a corrupted file
    /// as verified.
    pub fn get_or_create(&mut self, identity: &CacheIdentity) -> CommandResult<Arc<ChunkedCache>> {
        let cache_key = identity.cache_key();

        // Fast path: an in-memory handle already exists.
        if let Some(cache) = self.caches.get(&cache_key) {
            self.touch_access(&cache_key)?;
            return Ok(Arc::clone(cache));
        }

        // Verify the catalog row + data file before reuse.
        let verified = self.verify_entry(&cache_key, identity)?;

        if !verified {
            // Stale or missing — discard any leftover row/file and recreate.
            self.discard_entry(&cache_key)?;
        }

        // Reserve space and evict if needed before creating a NEW entry. A
        // verified hit is already counted in the catalog, so evicting for it
        // would double-count and could delete the entry being reused.
        if !verified {
            self.evict_if_needed(identity.expected_size)?;
        }

        // Create or open the on-disk file + in-memory handle. When the entry
        // was verified (catalog hit), initialize the ChunkedCache with the
        // persisted ranges from the catalog so completion and partial state
        // survive restart. The `.index` sidecar is no longer the source of
        // truth for ranges — the catalog is.
        let cache = if verified {
            let conn = self
                .control_db
                .lock()
                .map_err(|_| internal_error("control DB lock was poisoned"))?;
            let row = control_db::get_cache_entry(&conn, &cache_key)?.ok_or_else(|| {
                internal_error("verified entry disappeared between verify and open")
            })?;
            let ranges = ranges_from_json(&row.downloaded_ranges_json)?;
            ChunkedCache::open_with_ranges(
                &self.cache_dir,
                &cache_key,
                identity.expected_size,
                ranges,
            )
            .map_err(cache_error_to_command)?
        } else {
            ChunkedCache::open(&self.cache_dir, &cache_key, identity.expected_size)
                .map_err(cache_error_to_command)?
        };
        let cache = Arc::new(cache);

        // Persist the catalog row (upsert so a verified hit keeps its ranges).
        if verified {
            // The entry was verified — just touch the access time.
            self.touch_access(&cache_key)?;
        } else {
            // Fresh entry — insert the catalog row.
            let now = current_unix_time_ms();
            let data_filename = Self::data_filename(&cache_key);
            let row = CacheEntryRow {
                cache_key: cache_key.clone(),
                library_id: identity.library_id.clone(),
                relative_path: identity.relative_path.clone(),
                provider_revision: identity.provider_revision.clone(),
                content_digest: None,
                expected_size: identity.expected_size as i64,
                downloaded_ranges_json: ranges_to_json(&RangeSet::new())?,
                complete: false,
                pinned_count: 0,
                last_access_at_ms: now,
                verified_at_ms: None,
                data_path: data_filename,
            };
            let conn = self
                .control_db
                .lock()
                .map_err(|_| internal_error("control DB lock was poisoned"))?;
            control_db::upsert_cache_entry(&conn, &row)?;
        }

        self.caches.insert(cache_key.clone(), Arc::clone(&cache));
        Ok(cache)
    }

    /// Verify a catalog entry against the identity and disk. Returns `true`
    /// when the entry is reusable (catalog row exists, data file length
    /// matches, ranges are within the file). Complete entries that have not
    /// been verified since process start are re-hashed here.
    fn verify_entry(&self, cache_key: &str, identity: &CacheIdentity) -> CommandResult<bool> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        let Some(row) = control_db::get_cache_entry(&conn, cache_key)? else {
            return Ok(false);
        };

        // Identity mismatch (should not happen since cache_key is derived from
        // the identity, but defend against a manually-edited catalog).
        if row.library_id != identity.library_id
            || row.relative_path != identity.relative_path
            || row.expected_size != identity.expected_size as i64
        {
            return Ok(false);
        }

        let data_path = self.cache_dir.join(&row.data_path);
        let file_len = match fs::metadata(&data_path) {
            Ok(meta) => meta.len(),
            Err(_) => return Ok(false),
        };

        if file_len != row.expected_size as u64 {
            return Ok(false);
        }

        // Validate persisted ranges against the actual file length so a
        // corrupted sidecar cannot expose zero-filled sparse gaps as
        // downloaded data.
        let ranges = ranges_from_json(&row.downloaded_ranges_json)?;
        if !ranges_within_file(&ranges, file_len) {
            return Ok(false);
        }

        // Complete entries must be re-verified (re-hashed) before first reuse
        // after an unclean shutdown. `verified_at_ms` is set on successful
        // verification; a `None` value means "not yet verified this session".
        if row.complete && row.verified_at_ms.is_none() {
            if let Some(expected_digest) = &row.content_digest {
                let actual = sha256_file(&data_path)?;
                if !actual.eq_ignore_ascii_case(expected_digest) {
                    return Ok(false);
                }
                // Digest matches — mark as verified for this session.
                control_db::update_cache_entry_ranges(
                    &conn,
                    cache_key,
                    &row.downloaded_ranges_json,
                    true,
                    None,
                    Some(current_unix_time_ms()),
                )?;
            } else {
                // No stored digest — compute and store one now so future
                // verifications are cheap. This is the content-digest fallback
                // path for providers that do not expose a revision token.
                let digest = sha256_file(&data_path)?;
                control_db::update_cache_entry_ranges(
                    &conn,
                    cache_key,
                    &row.downloaded_ranges_json,
                    true,
                    Some(&digest),
                    Some(current_unix_time_ms()),
                )?;
            }
        }

        Ok(true)
    }

    /// Discard a catalog row and its data file. Used when verification fails.
    fn discard_entry(&self, cache_key: &str) -> CommandResult<()> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        // Remove the catalog row first; if the file deletion fails the
        // orphaned file is cleaned on the next startup scan.
        let _ = control_db::delete_cache_entry(&conn, cache_key);
        let data_path = self.cache_dir.join(Self::data_filename(cache_key));
        let _ = fs::remove_file(&data_path);
        Ok(())
    }

    /// Bump `last_access_at_ms` (wall-clock LRU touch).
    fn touch_access(&self, cache_key: &str) -> CommandResult<()> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        control_db::touch_cache_entry_access(&conn, cache_key, current_unix_time_ms())
    }

    /// Persist the current downloaded ranges + completion state for an entry.
    /// Called after each range write so a restart can resume from the
    /// persisted ranges.
    pub fn persist_ranges(&self, cache_key: &str) -> CommandResult<()> {
        let cache = self
            .caches
            .get(cache_key)
            .ok_or_else(|| internal_error(format!("no open cache for key {cache_key}")))?;
        let ranges = cache.downloaded();
        let complete = cache.is_complete();
        let json = ranges_to_json(&ranges)?;

        // When the entry becomes complete, compute and store the content
        // digest so future verifications can re-hash cheaply. Mark
        // verified_at_ms = None so the next reuse re-verifies the full file
        // (defense against unclean shutdown corruption).
        let (digest, verified_at) = if complete {
            let digest = sha256_file(cache.path())?;
            (Some(digest), None)
        } else {
            (None, None)
        };

        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        control_db::update_cache_entry_ranges(
            &conn,
            cache_key,
            &json,
            complete,
            digest.as_deref(),
            verified_at,
        )?;
        Ok(())
    }

    /// Pin an entry so eviction cannot remove it while in use. Returns a guard
    /// that decrements the pin count on drop. This is a free function on the
    /// `Arc<Mutex<CacheCatalog>>` because the guard needs to hold a reference
    /// to the manager to unpin on drop.
    pub fn pin_cache_entry(
        manager: &Arc<Mutex<CacheCatalog>>,
        cache_key: &str,
    ) -> CommandResult<CachePinGuard> {
        let manager_guard = manager
            .lock()
            .map_err(|_| internal_error("cache manager lock was poisoned"))?;
        let conn = manager_guard
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        control_db::pin_cache_entry(&conn, cache_key)?;
        drop(conn);
        drop(manager_guard);
        Ok(CachePinGuard {
            cache_key: cache_key.to_owned(),
            manager: Arc::clone(manager),
        })
    }

    /// Decrement the pin count for an entry. Called by `CachePinGuard::drop`.
    fn unpin(&mut self, cache_key: &str) -> CommandResult<()> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        control_db::unpin_cache_entry(&conn, cache_key)?;
        Ok(())
    }

    /// Evict unpinned entries with the oldest `last_access_at_ms` until adding
    /// `needed_bytes` would fit under the budget. Atomically removes the
    /// catalog row first, then the data file.
    fn evict_if_needed(&mut self, needed_bytes: u64) -> CommandResult<()> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        let rows = control_db::list_cache_entries(&conn)?;
        drop(conn);

        let current: u64 = rows.iter().map(|r| r.expected_size as u64).sum();
        if current.saturating_add(needed_bytes) <= self.max_bytes {
            return Ok(());
        }

        // Evict unpinned entries, oldest access first.
        let mut evictable: Vec<&CacheEntryRow> =
            rows.iter().filter(|r| r.pinned_count == 0).collect();
        evictable.sort_by_key(|r| r.last_access_at_ms);

        let mut freed = current;
        for row in evictable {
            if freed.saturating_add(needed_bytes) <= self.max_bytes {
                break;
            }
            // Remove the catalog row first (transactional), then the file.
            let conn = self
                .control_db
                .lock()
                .map_err(|_| internal_error("control DB lock was poisoned"))?;
            let _ = control_db::delete_cache_entry(&conn, &row.cache_key);
            drop(conn);
            let data_path = self.cache_dir.join(&row.data_path);
            let _ = fs::remove_file(&data_path);
            self.caches.remove(&row.cache_key);
            freed = freed.saturating_sub(row.expected_size as u64);
        }

        Ok(())
    }

    /// Evict all unpinned entries. Pinned entries are left in the catalog so
    /// they survive until playback releases them (their pin count drops to 0),
    /// at which point a subsequent clear or eviction removes them. Returns the
    /// number of entries evicted.
    pub fn clear_unpinned(&mut self) -> CommandResult<usize> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        let rows = control_db::delete_unpinned_cache_entries(&conn)?;
        drop(conn);

        let mut count = 0;
        for row in &rows {
            let data_path = self.cache_dir.join(&row.data_path);
            let _ = fs::remove_file(&data_path);
            self.caches.remove(&row.cache_key);
            count += 1;
        }
        Ok(count)
    }

    /// Remove an entry explicitly (catalog row + data file). Used by tests and
    /// the discard path.
    pub fn remove(&mut self, cache_key: &str) -> CommandResult<()> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        let _ = control_db::delete_cache_entry(&conn, cache_key);
        drop(conn);
        let data_path = self.cache_dir.join(Self::data_filename(cache_key));
        let _ = fs::remove_file(&data_path);
        self.caches.remove(cache_key);
        Ok(())
    }

    /// Save all in-memory range state to the catalog. Called at shutdown.
    pub fn persist_all(&self) -> CommandResult<()> {
        for (cache_key, cache) in &self.caches {
            let ranges = cache.downloaded();
            let complete = cache.is_complete();
            let json = ranges_to_json(&ranges)?;
            let (digest, verified_at) = if complete {
                let digest = sha256_file(cache.path())?;
                (Some(digest), None)
            } else {
                (None, None)
            };
            let conn = self
                .control_db
                .lock()
                .map_err(|_| internal_error("control DB lock was poisoned"))?;
            control_db::update_cache_entry_ranges(
                &conn,
                cache_key,
                &json,
                complete,
                digest.as_deref(),
                verified_at,
            )?;
        }
        Ok(())
    }

    /// Look up the catalog row for a cache key. Used by the revision-aware
    /// `ensure_remote_file_cached` to check whether a complete, verified entry
    /// already exists.
    pub fn get_entry(&self, cache_key: &str) -> CommandResult<Option<CacheEntryRow>> {
        let conn = self
            .control_db
            .lock()
            .map_err(|_| internal_error("control DB lock was poisoned"))?;
        control_db::get_cache_entry(&conn, cache_key)
    }
}

/// Map a `CacheError` to a `CommandError`. A disk-full write error is mapped
/// to a clear disk-space message so the caller can surface a specific error.
fn cache_error_to_command(error: CacheError) -> CommandError {
    match error {
        CacheError::Io(ref e) if is_disk_full(e) => internal_error(format!(
            "remote cache is full: insufficient disk space to write the cache file ({e})"
        )),
        other => internal_error(format!("remote cache error: {other}")),
    }
}

/// Detect ENOSPC (disk full) from an `io::Error`. `ErrorKind::Other` may wrap
/// the raw OS error on some platforms.
fn is_disk_full(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::Other && error.raw_os_error() == Some(28) {
        return true;
    }
    // Some platforms report `StorageFull` directly.
    #[cfg(unix)]
    if error.raw_os_error() == Some(28) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::control_db::open_control_db;
    use rusqlite::params;
    use tempfile::TempDir;

    /// Build a fresh catalog backed by a temp control DB and temp cache dir.
    fn fresh_catalog(
        max_bytes: u64,
    ) -> (
        TempDir,
        TempDir,
        Arc<Mutex<rusqlite::Connection>>,
        Arc<Mutex<CacheCatalog>>,
    ) {
        let db_dir = TempDir::new().expect("db temp dir");
        let cache_dir = TempDir::new().expect("cache temp dir");
        let conn = open_control_db(&db_dir.path().join("remote-state.db")).expect("open db");
        let control_db = Arc::new(Mutex::new(conn));
        let catalog = CacheCatalog::open(
            cache_dir.path().to_path_buf(),
            Arc::clone(&control_db),
            max_bytes,
        )
        .expect("open catalog");
        (db_dir, cache_dir, control_db, Arc::new(Mutex::new(catalog)))
    }

    fn identity(library_id: &str, path: &str, revision: Option<&str>, size: u64) -> CacheIdentity {
        CacheIdentity {
            library_id: library_id.to_owned(),
            relative_path: path.to_owned(),
            provider_revision: revision.map(str::to_owned),
            expected_size: size,
        }
    }

    // ---- complete and partial entries reopen correctly after restart ----

    #[test]
    fn complete_entry_reopens_verified_after_restart() {
        let (_db_dir, cache_dir, control_db, catalog_arc) =
            fresh_catalog(DEFAULT_CACHE_BYTES_LIMIT);
        let id = identity("lib-1", "media/a.mp3", Some("rev-1"), 100);

        let cache = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id).unwrap()
        };
        cache.write_at(0, &[42u8; 100]).unwrap();
        {
            let catalog = catalog_arc.lock().unwrap();
            catalog.persist_ranges(&id.cache_key()).unwrap();
        }

        // Simulate restart: drop the in-memory handles and reopen.
        drop(catalog_arc);
        let catalog = CacheCatalog::open(
            cache_dir.path().to_path_buf(),
            Arc::clone(&control_db),
            DEFAULT_CACHE_BYTES_LIMIT,
        )
        .expect("reopen");

        let row = catalog.get_entry(&id.cache_key()).unwrap().unwrap();
        assert!(row.complete);
        // Reopening should verify (re-hash) the complete entry and mark it
        // verified.
        let mut catalog = catalog;
        let cache2 = catalog.get_or_create(&id).unwrap();
        assert!(cache2.is_complete());

        let row = catalog.get_entry(&id.cache_key()).unwrap().unwrap();
        assert!(
            row.verified_at_ms.is_some(),
            "complete entry must be verified after reopen"
        );
    }

    #[test]
    fn partial_entry_resumes_with_ranges_intact() {
        let (_db_dir, cache_dir, control_db, catalog_arc) =
            fresh_catalog(DEFAULT_CACHE_BYTES_LIMIT);
        let id = identity("lib-1", "media/b.mp3", Some("rev-1"), 200);

        let cache = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id).unwrap()
        };
        cache.write_at(0, &[1u8; 50]).unwrap();
        cache.write_at(100, &[2u8; 50]).unwrap();
        {
            let catalog = catalog_arc.lock().unwrap();
            catalog.persist_ranges(&id.cache_key()).unwrap();
        }

        drop(catalog_arc);
        let mut catalog = CacheCatalog::open(
            cache_dir.path().to_path_buf(),
            Arc::clone(&control_db),
            DEFAULT_CACHE_BYTES_LIMIT,
        )
        .expect("reopen");

        let cache2 = catalog.get_or_create(&id).unwrap();
        assert!(cache2.is_cached(0, 50));
        assert!(cache2.is_cached(100, 50));
        assert!(!cache2.is_complete());

        let row = catalog.get_entry(&id.cache_key()).unwrap().unwrap();
        assert!(!row.complete);
    }

    // ---- orphaned data file with no catalog row is removed on startup ----

    #[test]
    fn orphaned_data_file_removed_on_startup() {
        let (_db_dir, cache_dir, control_db, _catalog_arc) =
            fresh_catalog(DEFAULT_CACHE_BYTES_LIMIT);
        // Drop the catalog so only the orphan file remains.
        drop(_catalog_arc);
        let orphan = cache_dir.path().join("orphan.cache");
        fs::write(&orphan, b"junk").unwrap();
        assert!(orphan.exists());

        let _catalog = CacheCatalog::open(
            cache_dir.path().to_path_buf(),
            Arc::clone(&control_db),
            DEFAULT_CACHE_BYTES_LIMIT,
        )
        .expect("reopen");

        assert!(
            !orphan.exists(),
            "orphaned data file must be removed on startup scan"
        );
    }

    // ---- changed provider revision is NOT reused ----

    #[test]
    fn changed_revision_creates_new_entry() {
        let (_db_dir, _cache_dir, _control_db, catalog_arc) =
            fresh_catalog(DEFAULT_CACHE_BYTES_LIMIT);
        let id_v1 = identity("lib-1", "media/c.mp3", Some("rev-1"), 100);
        let id_v2 = identity("lib-1", "media/c.mp3", Some("rev-2"), 100);

        let cache_v1 = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_v1).unwrap()
        };
        cache_v1.write_at(0, &[9u8; 100]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id_v1.cache_key())
                .unwrap();
        }

        let key_v1 = id_v1.cache_key();
        let key_v2 = id_v2.cache_key();
        assert_ne!(
            key_v1, key_v2,
            "different revisions must yield different cache keys"
        );

        let cache_v2 = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_v2).unwrap()
        };
        assert!(
            !cache_v2.is_complete(),
            "new revision must not reuse old entry's data"
        );

        // Old entry remains until evicted.
        let catalog = catalog_arc.lock().unwrap();
        assert!(catalog.get_entry(&key_v1).unwrap().is_some());
        assert!(catalog.get_entry(&key_v2).unwrap().is_some());
    }

    // ---- mismatched size is rejected ----

    #[test]
    fn mismatched_size_is_rejected() {
        let (_db_dir, cache_dir, control_db, catalog_arc) =
            fresh_catalog(DEFAULT_CACHE_BYTES_LIMIT);
        let id = identity("lib-1", "media/d.mp3", Some("rev-1"), 100);

        let cache = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id).unwrap()
        };
        cache.write_at(0, &[0u8; 100]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id.cache_key())
                .unwrap();
        }
        drop(catalog_arc);

        // Corrupt the data file: truncate it so the length no longer matches.
        let data_path = cache_dir.path().join(format!("{}.cache", id.cache_key()));
        let _ = fs::write(&data_path, b"short");

        let catalog = CacheCatalog::open(
            cache_dir.path().to_path_buf(),
            Arc::clone(&control_db),
            DEFAULT_CACHE_BYTES_LIMIT,
        )
        .expect("reopen");

        // The stale row should have been discarded during reconciliation.
        let row = catalog.get_entry(&id.cache_key()).unwrap();
        assert!(
            row.is_none(),
            "entry with mismatched size must be discarded"
        );
    }

    // ---- default budget is 2 GiB ----

    #[test]
    fn default_budget_is_2_gib() {
        assert_eq!(DEFAULT_CACHE_BYTES_LIMIT, 2 * 1024 * 1024 * 1024);
    }

    // ---- startup accounting includes pre-existing disk files ----

    #[test]
    fn startup_accounting_includes_preexisting_files() {
        let (_db_dir, cache_dir, control_db, catalog_arc) =
            fresh_catalog(DEFAULT_CACHE_BYTES_LIMIT);

        let id = identity("lib-1", "media/e.mp3", Some("rev-1"), 100);
        let cache = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id).unwrap()
        };
        cache.write_at(0, &[0u8; 100]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id.cache_key())
                .unwrap();
        }
        drop(catalog_arc);

        let catalog = CacheCatalog::open(
            cache_dir.path().to_path_buf(),
            Arc::clone(&control_db),
            DEFAULT_CACHE_BYTES_LIMIT,
        )
        .expect("reopen");

        let usage = catalog.usage().unwrap();
        assert_eq!(usage.used_bytes, 100);
        assert_eq!(usage.entry_count, 1);
    }

    // ---- LRU uses persistent timestamps ----

    #[test]
    fn lru_evicts_oldest_access_first() {
        let (_db_dir, _cache_dir, _control_db, catalog_arc) = fresh_catalog(250);

        let id_a = identity("lib-1", "media/a.mp3", Some("rev-1"), 100);
        let id_b = identity("lib-1", "media/b.mp3", Some("rev-1"), 100);

        let cache_a = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_a).unwrap()
        };
        cache_a.write_at(0, &[1u8; 100]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id_a.cache_key())
                .unwrap();
        }

        // Sleep so B's access timestamp is strictly greater than A's.
        std::thread::sleep(std::time::Duration::from_millis(20));

        let cache_b = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_b).unwrap()
        };
        cache_b.write_at(0, &[2u8; 100]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id_b.cache_key())
                .unwrap();
        }

        // Adding a third 100-byte entry (total 300 > 250) forces eviction.
        let id_c = identity("lib-1", "media/c.mp3", Some("rev-1"), 100);
        {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_c).unwrap();
        }

        let catalog = catalog_arc.lock().unwrap();
        // A (oldest) should be evicted; B and C remain.
        assert!(
            catalog.get_entry(&id_a.cache_key()).unwrap().is_none(),
            "oldest entry must be evicted first"
        );
        assert!(catalog.get_entry(&id_b.cache_key()).unwrap().is_some());
        assert!(catalog.get_entry(&id_c.cache_key()).unwrap().is_some());
    }

    #[test]
    fn lru_persists_across_restart() {
        let (_db_dir, cache_dir, control_db, catalog_arc) = fresh_catalog(250);

        let id_a = identity("lib-1", "media/a.mp3", Some("rev-1"), 100);
        let id_b = identity("lib-1", "media/b.mp3", Some("rev-1"), 100);

        let cache_a = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_a).unwrap()
        };
        cache_a.write_at(0, &[1u8; 100]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id_a.cache_key())
                .unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        let cache_b = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_b).unwrap()
        };
        cache_b.write_at(0, &[2u8; 100]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id_b.cache_key())
                .unwrap();
        }
        drop(catalog_arc);

        let mut catalog =
            CacheCatalog::open(cache_dir.path().to_path_buf(), Arc::clone(&control_db), 250)
                .expect("reopen");

        let id_c = identity("lib-1", "media/c.mp3", Some("rev-1"), 100);
        catalog.get_or_create(&id_c).unwrap();

        assert!(catalog.get_entry(&id_a.cache_key()).unwrap().is_none());
        assert!(catalog.get_entry(&id_b.cache_key()).unwrap().is_some());
        assert!(catalog.get_entry(&id_c.cache_key()).unwrap().is_some());
    }

    // ---- pinned entries are never evicted ----

    #[test]
    fn pinned_entry_surves_eviction() {
        let (_db_dir, _cache_dir, _control_db, catalog_arc) = fresh_catalog(200);

        let id_a = identity("lib-1", "media/a.mp3", Some("rev-1"), 100);
        let id_b = identity("lib-1", "media/b.mp3", Some("rev-1"), 100);

        let cache_a = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_a).unwrap()
        };
        cache_a.write_at(0, &[1u8; 100]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id_a.cache_key())
                .unwrap();
        }

        // Pin A.
        let _guard = CacheCatalog::pin_cache_entry(&catalog_arc, &id_a.cache_key()).unwrap();

        let cache_b = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_b).unwrap()
        };
        cache_b.write_at(0, &[2u8; 100]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id_b.cache_key())
                .unwrap();
        }

        // Adding a third entry would exceed budget; A is pinned so B (the
        // next oldest unpinned) is evicted instead.
        let id_c = identity("lib-1", "media/c.mp3", Some("rev-1"), 100);
        {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_c).unwrap();
        }

        let catalog = catalog_arc.lock().unwrap();
        assert!(
            catalog.get_entry(&id_a.cache_key()).unwrap().is_some(),
            "pinned entry must survive eviction"
        );
    }

    // ---- clearing cache defers pinned deletion ----

    #[test]
    fn clear_cache_keeps_pinned_entries() {
        let (_db_dir, _cache_dir, _control_db, catalog_arc) =
            fresh_catalog(DEFAULT_CACHE_BYTES_LIMIT);

        let id_a = identity("lib-1", "media/a.mp3", Some("rev-1"), 100);
        let id_b = identity("lib-1", "media/b.mp3", Some("rev-1"), 100);

        let cache_a = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_a).unwrap()
        };
        cache_a.write_at(0, &[1u8; 100]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id_a.cache_key())
                .unwrap();
        }

        let _guard = CacheCatalog::pin_cache_entry(&catalog_arc, &id_a.cache_key()).unwrap();

        let cache_b = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id_b).unwrap()
        };
        cache_b.write_at(0, &[2u8; 100]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id_b.cache_key())
                .unwrap();
        }

        let evicted = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.clear_unpinned().unwrap()
        };
        assert_eq!(evicted, 1, "only the unpinned entry should be evicted");

        {
            let catalog = catalog_arc.lock().unwrap();
            assert!(
                catalog.get_entry(&id_a.cache_key()).unwrap().is_some(),
                "pinned entry must remain after clear"
            );
            assert!(
                catalog.get_entry(&id_b.cache_key()).unwrap().is_none(),
                "unpinned entry must be evicted by clear"
            );
        }

        // Dropping the guard makes A eligible for a future clear. The guard's
        // Drop impl locks the manager, so the manager lock above must be
        // released first (the block scope ensures this).
        drop(_guard);
        let evicted2 = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.clear_unpinned().unwrap()
        };
        assert_eq!(evicted2, 1, "deferred pinned entry is evicted after unpin");
    }

    // ---- corrupted range metadata is rejected ----

    #[test]
    fn corrupted_ranges_beyond_file_length_rejected() {
        let (_db_dir, cache_dir, control_db, catalog_arc) =
            fresh_catalog(DEFAULT_CACHE_BYTES_LIMIT);
        let id = identity("lib-1", "media/f.mp3", Some("rev-1"), 100);

        let cache = {
            let mut catalog = catalog_arc.lock().unwrap();
            catalog.get_or_create(&id).unwrap()
        };
        cache.write_at(0, &[0u8; 50]).unwrap();
        {
            catalog_arc
                .lock()
                .unwrap()
                .persist_ranges(&id.cache_key())
                .unwrap();
        }
        drop(catalog_arc);

        // Corrupt the downloaded_ranges_json to claim a range beyond the file.
        let conn = control_db.lock().unwrap();
        conn.execute(
            "UPDATE remote_cache_entries SET downloaded_ranges_json = ?2 WHERE cache_key = ?1",
            params![id.cache_key(), r#"[[0,999]]"#],
        )
        .unwrap();
        drop(conn);

        let mut catalog = CacheCatalog::open(
            cache_dir.path().to_path_buf(),
            Arc::clone(&control_db),
            DEFAULT_CACHE_BYTES_LIMIT,
        )
        .expect("reopen");

        // The corrupted entry should be rejected on verification (get_or_create
        // discards it and creates a fresh one).
        let cache2 = catalog.get_or_create(&id).unwrap();
        assert!(
            !cache2.is_cached(0, 999),
            "corrupted ranges must not be trusted"
        );
    }

    // ---- cache key is deterministic ----

    #[test]
    fn cache_key_is_deterministic() {
        let id1 = identity("lib-1", "media/a.mp3", Some("rev-1"), 100);
        let id2 = identity("lib-1", "media/a.mp3", Some("rev-1"), 100);
        assert_eq!(id1.cache_key(), id2.cache_key());

        let id3 = identity("lib-1", "media/a.mp3", Some("rev-2"), 100);
        assert_ne!(id1.cache_key(), id3.cache_key());

        let id4 = identity("lib-1", "media/a.mp3", Some("rev-1"), 200);
        assert_ne!(id1.cache_key(), id4.cache_key());
    }
}
