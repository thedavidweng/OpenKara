//! Media cache: local copies of Remote Repository media under a byte budget.
//!
//! The catalog (`remote_cache_entries`) is authoritative. Data files are named
//! by the SHA-256 of the identity tuple `(library_id, relative_path,
//! provider_revision, expected_size)`, so a new remote revision never reuses
//! bytes of an older one.
//!
//! ## Startup reconciliation
//!
//! On open, catalog rows whose data file is missing or has the wrong length
//! are deleted, then data files without a catalog row are deleted. Complete
//! entries not verified since the last process start are re-hashed before
//! first reuse, so an unclean shutdown cannot expose a corrupted file.
//!
//! ## Eviction
//!
//! Opening an entry for playback records the wall-clock access time, so the
//! budget evicts the least recently played entries first, across restarts.
//! Two things exempt an entry: an active playback lease, and an Offline Pin
//! on a song that owns the entry's asset path. The pin protects only the most
//! recently played entry of each pinned path, so a superseded revision of a
//! pinned asset still ages out. Eviction deletes the catalog row first, then
//! the data file; a file that fails to delete is an orphan removed on the
//! next startup.

mod chunked;
mod range_set;
mod store;

pub use chunked::{CacheError, ChunkedCache};
pub use range_set::{ByteRange, RangeSet};
pub use store::{read_entry, CacheEntry};

use crate::asset::content_digest_from_asset_path;
use crate::download::hex_lower;
use crate::error::{RemoteError, RemoteErrorKind, RemoteResult};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use store::storage_error;

/// Budget used when the user has not configured one: finite so a fresh
/// install cannot fill the disk, large enough that a session does not thrash.
pub const DEFAULT_CACHE_BYTES_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CacheIdentity {
    pub library_id: String,
    pub relative_path: String,
    pub provider_revision: Option<String>,
    pub expected_size: u64,
}

impl CacheIdentity {
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
        hex_lower(&hasher.finalize())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheUsage {
    pub used_bytes: u64,
    pub limit_bytes: u64,
    pub entry_count: usize,
    /// Entries exempt from eviction: leased by playback or kept by an
    /// Offline Pin.
    pub pinned_count: usize,
}

/// Keeps one entry out of eviction while playback reads it. Dropping the
/// lease releases it, even when the playback thread panics.
pub struct CacheLease {
    cache_key: String,
    cache: Arc<Mutex<MediaCache>>,
}

impl Drop for CacheLease {
    fn drop(&mut self) {
        if let Ok(cache) = self.cache.lock() {
            if let Ok(catalog) = cache.catalog() {
                let _ = store::release_lease(&catalog, &self.cache_key);
            }
        }
    }
}

/// The media cache for every Remote Repository on this device.
///
/// Durable state (ranges, completion, recency, leases, Offline Pins) lives in
/// the catalog database so it survives restarts; open `ChunkedCache` handles
/// are kept for the life of the process only to share them between readers.
pub struct MediaCache {
    cache_dir: PathBuf,
    catalog: Arc<Mutex<Connection>>,
    max_bytes: u64,
    caches: HashMap<String, Arc<ChunkedCache>>,
}

fn current_unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[derive(Serialize, Deserialize)]
struct RangesJson(Vec<[u64; 2]>);

fn ranges_to_json(ranges: &RangeSet) -> RemoteResult<String> {
    let pairs: Vec<[u64; 2]> = ranges
        .ranges()
        .iter()
        .map(|r| [r.start, r.length])
        .collect();
    serde_json::to_string(&RangesJson(pairs))
        .map_err(|e| storage_error("failed to serialize cached ranges", e))
}

fn ranges_from_json(json: &str) -> RemoteResult<RangeSet> {
    let parsed: RangesJson = serde_json::from_str(json)
        .map_err(|e| storage_error("failed to deserialize cached ranges", e))?;
    let mut set = RangeSet::new();
    for [start, length] in parsed.0 {
        set.add_range(start, length);
    }
    Ok(set)
}

/// A corrupted catalog row that claims bytes past the end of the data file
/// would expose zero-filled sparse gaps as downloaded data.
fn ranges_within_file(ranges: &RangeSet, file_len: u64) -> bool {
    ranges.ranges().iter().all(|r| r.end() <= file_len)
}

fn local_io_error(context: impl std::fmt::Display, error: &std::io::Error) -> RemoteError {
    if error.kind() == std::io::ErrorKind::StorageFull || error.raw_os_error() == Some(28) {
        return RemoteError::new(
            RemoteErrorKind::DiskFull,
            format!("media cache is full: insufficient disk space ({context}: {error})"),
        );
    }
    storage_error(&context.to_string(), error)
}

fn cache_error(error: CacheError) -> RemoteError {
    match error {
        CacheError::Io(ref io) => local_io_error("media cache file", io),
        CacheError::Timeout => storage_error("media cache", error),
    }
}

fn sha256_of_file(path: &Path) -> RemoteResult<String> {
    let context = || format!("failed to hash {}", path.display());
    let mut file = fs::File::open(path).map_err(|e| local_io_error(context(), &e))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| local_io_error(context(), &e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn data_filename(cache_key: &str) -> String {
    format!("{cache_key}.cache")
}

/// Keys of the entries eviction must skip.
fn protected_keys(
    entries: &[CacheEntry],
    pinned_assets: &HashSet<(String, String)>,
) -> HashSet<String> {
    let mut newest_pinned: HashMap<(&str, &str), i64> = HashMap::new();
    for entry in entries {
        let asset = (entry.library_id.clone(), entry.relative_path.clone());
        if pinned_assets.contains(&asset) {
            let newest = newest_pinned
                .entry((&entry.library_id, &entry.relative_path))
                .or_insert(entry.last_access_at_ms);
            *newest = (*newest).max(entry.last_access_at_ms);
        }
    }
    entries
        .iter()
        .filter(|entry| {
            entry.lease_count > 0
                || newest_pinned
                    .get(&(entry.library_id.as_str(), entry.relative_path.as_str()))
                    .is_some_and(|newest| *newest == entry.last_access_at_ms)
        })
        .map(|entry| entry.cache_key.clone())
        .collect()
}

impl MediaCache {
    /// Open the media cache in `cache_dir` with its catalog in `catalog`, and
    /// reconcile the catalog against the files on disk.
    pub fn open(
        cache_dir: PathBuf,
        catalog: Arc<Mutex<Connection>>,
        max_bytes: u64,
    ) -> RemoteResult<Self> {
        fs::create_dir_all(&cache_dir).map_err(|e| {
            local_io_error(
                format!("failed to create cache directory {}", cache_dir.display()),
                &e,
            )
        })?;

        let cache = Self {
            cache_dir,
            catalog,
            max_bytes,
            caches: HashMap::new(),
        };
        store::ensure_schema(&*cache.catalog()?)?;
        cache.reconcile_on_startup()?;
        Ok(cache)
    }

    fn catalog(&self) -> RemoteResult<MutexGuard<'_, Connection>> {
        self.catalog.lock().map_err(|_| {
            RemoteError::new(
                RemoteErrorKind::LocalStorageFailed,
                "media cache catalog lock was poisoned",
            )
        })
    }

    pub fn limit_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Complete entries are not re-hashed here; that happens on first reuse
    /// so startup does not hash the whole cache.
    fn reconcile_on_startup(&self) -> RemoteResult<()> {
        let catalog = self.catalog()?;
        let mut known_files = HashSet::new();

        for row in store::list_entries(&catalog)? {
            let data_path = Path::new(&row.data_path);
            let absolute = if data_path.is_absolute() {
                data_path.to_path_buf()
            } else {
                self.cache_dir.join(data_path)
            };

            let file_ok = fs::metadata(&absolute)
                .map(|meta| meta.len() == row.expected_size as u64)
                .unwrap_or(false);
            if !file_ok {
                let _ = store::delete_entry(&catalog, &row.cache_key);
                continue;
            }
            if let Some(name) = absolute.file_name() {
                known_files.insert(name.to_string_lossy().into_owned());
            }
        }

        // Only `.cache` files belong to the catalog; other temporaries in the
        // directory are left alone.
        if let Ok(entries) = fs::read_dir(&self.cache_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.ends_with(".cache") && !known_files.contains(name.as_ref()) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }

        Ok(())
    }

    pub fn usage(&self) -> RemoteResult<CacheUsage> {
        let catalog = self.catalog()?;
        let entries = store::list_entries(&catalog)?;
        let pinned_assets = store::list_pinned_assets(&catalog)?;
        Ok(CacheUsage {
            used_bytes: entries.iter().map(|r| r.expected_size as u64).sum(),
            limit_bytes: self.max_bytes,
            entry_count: entries.len(),
            pinned_count: protected_keys(&entries, &pinned_assets).len(),
        })
    }

    /// Open the entry for `identity` for playback, creating it (and evicting
    /// to make room) when no reusable entry exists. Opening counts as playing
    /// the entry for least-recently-played eviction.
    pub fn get_or_create(&mut self, identity: &CacheIdentity) -> RemoteResult<Arc<ChunkedCache>> {
        let cache_key = identity.cache_key();

        if let Some(cache) = self.caches.get(&cache_key) {
            let cache = Arc::clone(cache);
            store::touch_entry_access(&*self.catalog()?, &cache_key, current_unix_time_ms())?;
            return Ok(cache);
        }

        let verified = self.verify_entry(&cache_key, identity)?;
        let cache = if verified {
            let catalog = self.catalog()?;
            let row = store::read_entry(&catalog, &cache_key)?.ok_or_else(|| {
                storage_error(
                    "media cache",
                    "verified entry disappeared between verify and open",
                )
            })?;
            let ranges = ranges_from_json(&row.downloaded_ranges_json)?;
            store::touch_entry_access(&catalog, &cache_key, current_unix_time_ms())?;
            ChunkedCache::open_with_ranges(
                &self.cache_dir,
                &cache_key,
                identity.expected_size,
                ranges,
            )
            .map_err(cache_error)?
        } else {
            self.discard_entry(&cache_key)?;
            self.evict_if_needed(identity.expected_size)?;
            let cache = ChunkedCache::open(&self.cache_dir, &cache_key, identity.expected_size)
                .map_err(cache_error)?;
            let row = CacheEntry {
                cache_key: cache_key.clone(),
                library_id: identity.library_id.clone(),
                relative_path: identity.relative_path.clone(),
                provider_revision: identity.provider_revision.clone(),
                content_digest: None,
                expected_size: identity.expected_size as i64,
                downloaded_ranges_json: ranges_to_json(&RangeSet::new())?,
                complete: false,
                lease_count: 0,
                last_access_at_ms: current_unix_time_ms(),
                verified_at_ms: None,
                data_path: data_filename(&cache_key),
            };
            store::upsert_entry(&*self.catalog()?, &row)?;
            cache
        };

        let cache = Arc::new(cache);
        self.caches.insert(cache_key, Arc::clone(&cache));
        Ok(cache)
    }

    /// An entry is reusable when its row matches the identity, its data file
    /// has the expected length and its ranges lie within the file. A complete
    /// entry is re-hashed once per process start against the digest its
    /// content-addressed path names, or else the digest recorded when it
    /// completed.
    fn verify_entry(&self, cache_key: &str, identity: &CacheIdentity) -> RemoteResult<bool> {
        let catalog = self.catalog()?;
        let Some(row) = store::read_entry(&catalog, cache_key)? else {
            return Ok(false);
        };

        if row.library_id != identity.library_id
            || row.relative_path != identity.relative_path
            || row.expected_size != identity.expected_size as i64
        {
            return Ok(false);
        }

        let data_path = self.cache_dir.join(&row.data_path);
        let Ok(metadata) = fs::metadata(&data_path) else {
            return Ok(false);
        };
        let file_len = metadata.len();
        if file_len != row.expected_size as u64 {
            return Ok(false);
        }

        let ranges = ranges_from_json(&row.downloaded_ranges_json)?;
        if !ranges_within_file(&ranges, file_len) {
            return Ok(false);
        }

        if row.complete && row.verified_at_ms.is_none() {
            let actual = sha256_of_file(&data_path)?;
            let expected =
                content_digest_from_asset_path(&row.relative_path).or(row.content_digest.clone());
            if let Some(expected) = expected {
                if !actual.eq_ignore_ascii_case(&expected) {
                    return Ok(false);
                }
            }
            store::update_entry_ranges(
                &catalog,
                cache_key,
                &row.downloaded_ranges_json,
                true,
                Some(&actual),
                Some(current_unix_time_ms()),
            )?;
        }

        Ok(true)
    }

    fn discard_entry(&mut self, cache_key: &str) -> RemoteResult<()> {
        let _ = store::delete_entry(&*self.catalog()?, cache_key);
        let _ = fs::remove_file(self.cache_dir.join(data_filename(cache_key)));
        self.caches.remove(cache_key);
        Ok(())
    }

    /// Record the downloaded ranges of an open entry. When the entry becomes
    /// complete its content is hashed; a content-addressed asset whose bytes
    /// do not match the digest in its path is discarded and reported as an
    /// integrity failure, so only verified copies are kept.
    pub fn persist_ranges(&mut self, cache_key: &str) -> RemoteResult<()> {
        let cache =
            self.caches.get(cache_key).cloned().ok_or_else(|| {
                storage_error("media cache", format!("no open entry {cache_key}"))
            })?;
        let ranges = cache.downloaded();
        let complete = cache.is_complete();
        let json = ranges_to_json(&ranges)?;

        let digest = if complete {
            let actual = sha256_of_file(cache.path())?;
            let row = store::read_entry(&*self.catalog()?, cache_key)?;
            let expected = row.and_then(|row| content_digest_from_asset_path(&row.relative_path));
            if let Some(expected) = expected {
                if !actual.eq_ignore_ascii_case(&expected) {
                    self.discard_entry(cache_key)?;
                    return Err(RemoteError::new(
                        RemoteErrorKind::RemoteIntegrityFailed,
                        format!("cached asset digest {actual} does not match {expected}"),
                    ));
                }
            }
            Some(actual)
        } else {
            None
        };

        store::update_entry_ranges(
            &*self.catalog()?,
            cache_key,
            &json,
            complete,
            digest.as_deref(),
            None,
        )
    }

    /// Lease an entry for the duration of playback. Leased entries are never
    /// evicted or cleared.
    pub fn lease(cache: &Arc<Mutex<MediaCache>>, cache_key: &str) -> RemoteResult<CacheLease> {
        {
            let guard = cache.lock().map_err(|_| {
                RemoteError::new(
                    RemoteErrorKind::LocalStorageFailed,
                    "media cache lock was poisoned",
                )
            })?;
            store::add_lease(&*guard.catalog()?, cache_key)?;
        }
        Ok(CacheLease {
            cache_key: cache_key.to_owned(),
            cache: Arc::clone(cache),
        })
    }

    /// Set an Offline Pin on a song. `relative_paths` are the song's asset
    /// paths in the repository and replace any set pinned before; the pin
    /// covers each path whatever its revision. Pinning does not download.
    pub fn pin_song<I>(
        &self,
        library_id: &str,
        song_id: &str,
        relative_paths: I,
    ) -> RemoteResult<()>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let paths: Vec<String> = relative_paths
            .into_iter()
            .map(|path| path.as_ref().to_owned())
            .collect();
        store::replace_song_pin(
            &*self.catalog()?,
            library_id,
            song_id,
            &paths,
            current_unix_time_ms(),
        )
    }

    /// Remove a song's Offline Pin. Its cached entries become evictable.
    pub fn unpin_song(&self, library_id: &str, song_id: &str) -> RemoteResult<()> {
        store::delete_song_pin(&*self.catalog()?, library_id, song_id)
    }

    /// Song ids in `library_id` that carry an Offline Pin, in id order.
    pub fn pinned_songs(&self, library_id: &str) -> RemoteResult<Vec<String>> {
        store::list_pinned_songs(&*self.catalog()?, library_id)
    }

    /// Evict least recently played, unprotected entries until `needed_bytes`
    /// more fit the budget, or nothing evictable remains.
    fn evict_if_needed(&mut self, needed_bytes: u64) -> RemoteResult<()> {
        let (entries, protected) = self.entries_with_protection()?;
        let mut used: u64 = entries.iter().map(|r| r.expected_size as u64).sum();
        if used.saturating_add(needed_bytes) <= self.max_bytes {
            return Ok(());
        }

        let mut evictable: Vec<&CacheEntry> = entries
            .iter()
            .filter(|entry| !protected.contains(&entry.cache_key))
            .collect();
        evictable.sort_by_key(|entry| entry.last_access_at_ms);

        for entry in evictable {
            if used.saturating_add(needed_bytes) <= self.max_bytes {
                break;
            }
            self.evict(entry)?;
            used = used.saturating_sub(entry.expected_size as u64);
        }
        Ok(())
    }

    fn entries_with_protection(&self) -> RemoteResult<(Vec<CacheEntry>, HashSet<String>)> {
        let catalog = self.catalog()?;
        let entries = store::list_entries(&catalog)?;
        let pinned_assets = store::list_pinned_assets(&catalog)?;
        let protected = protected_keys(&entries, &pinned_assets);
        Ok((entries, protected))
    }

    fn evict(&mut self, entry: &CacheEntry) -> RemoteResult<()> {
        let _ = store::delete_entry(&*self.catalog()?, &entry.cache_key);
        let _ = fs::remove_file(self.cache_dir.join(&entry.data_path));
        self.caches.remove(&entry.cache_key);
        Ok(())
    }

    /// Remove every entry that is neither leased nor kept by an Offline Pin.
    /// Returns the number of entries removed.
    pub fn clear_evictable(&mut self) -> RemoteResult<usize> {
        let (entries, protected) = self.entries_with_protection()?;
        let mut removed = 0;
        for entry in entries
            .iter()
            .filter(|entry| !protected.contains(&entry.cache_key))
        {
            self.evict(entry)?;
            removed += 1;
        }
        Ok(removed)
    }

    pub fn get_entry(&self, cache_key: &str) -> RemoteResult<Option<CacheEntry>> {
        store::read_entry(&*self.catalog()?, cache_key)
    }
}

#[cfg(test)]
mod tests;
