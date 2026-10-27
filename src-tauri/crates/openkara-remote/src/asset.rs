//! Verified asset fetch with range resume, and the range stream seam.
//!
//! Assets live in the managed content-addressed directories: the writer
//! publishes `media/content/<sha256>.<ext>` (and `media-g/…`, `stems/…`), so
//! the asset path itself names the digest the download is verified against.
//! Paths outside that layout (legacy repositories, byte-addressed artwork)
//! fall back to size-only verification, exactly as before.
//!
//! [`fetch_asset`] downloads with range resume into a sibling
//! `.part.<operation-id>` file, verifies size and digest, and only then
//! moves the candidate over the destination. An existing destination that
//! already verifies is reused without a transfer; a failed fetch leaves any
//! previous copy in place.
//!
//! [`RangeReader`] reads byte ranges of one repository object. Playback
//! streaming starts from it before a whole-asset download finishes, and the
//! media cache wraps the same seam.

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::download;
use crate::error::{RemoteError, RemoteErrorKind, RemoteResult};
use crate::storage::{validate_object_path, RepositoryStorage};

/// Chunk size for resumable asset downloads, matching the snapshot reader.
pub const ASSET_DOWNLOAD_CHUNK_SIZE: u64 = 8 * 1024 * 1024;

/// The remote object an asset fetch downloads, with the expectations the
/// download is verified against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetSource {
    pub remote_path: String,
    /// Size from `stat`; `None` when the object is absent or the storage
    /// reports no size.
    pub expected_size: Option<u64>,
    /// Digest named by a content-addressed path; `None` outside that layout.
    pub expected_digest: Option<String>,
    /// Revision of the remote object at planning time.
    pub revision: Option<String>,
}

/// A verified asset in the Local Working Copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedAsset {
    pub remote_path: String,
    pub size_bytes: u64,
    pub sha256: String,
    /// `false` when the destination already held a verified copy and nothing
    /// was transferred.
    pub downloaded: bool,
}

/// The SHA-256 a content-addressed asset path names:
/// `<top>/content/<64 hex digits>.<ext>`, e.g. `media/content/<sha256>.flac`.
/// Returns `None` for paths outside that layout, where verification falls
/// back to size only.
pub fn content_digest_from_asset_path(remote_path: &str) -> Option<String> {
    let mut segments = remote_path.split('/');
    let top = segments.next()?;
    let kind = segments.next()?;
    let file_name = segments.next()?;
    if segments.next().is_some() || top.is_empty() || kind != "content" {
        return None;
    }
    let (stem, extension) = file_name.rsplit_once('.')?;
    let is_digest = stem.len() == 64
        && !extension.is_empty()
        && stem.bytes().all(|byte| byte.is_ascii_hexdigit());
    is_digest.then(|| stem.to_ascii_lowercase())
}

/// Inspect one remote asset: size and revision from `stat`, digest from the
/// content-addressed path layout.
pub fn plan_asset_fetch(
    storage: &dyn RepositoryStorage,
    remote_path: &str,
) -> RemoteResult<AssetSource> {
    validate_object_path(remote_path)?;
    let metadata = storage.stat(remote_path)?;
    Ok(AssetSource {
        remote_path: remote_path.to_owned(),
        // A stat without a size (HEAD without Content-Length) reports 0; the
        // download then verifies by digest or runs unverified like before.
        expected_size: metadata
            .as_ref()
            .map(|metadata| metadata.size_bytes)
            .filter(|&size| size > 0),
        expected_digest: content_digest_from_asset_path(remote_path),
        revision: metadata.and_then(|metadata| metadata.revision),
    })
}

/// The operation id of an asset fetch's part file. It is stable across
/// attempts so an interrupted download resumes after a restart, and it
/// changes with the remote revision so a new version never resumes from the
/// old one's bytes.
pub fn asset_operation_id(remote_path: &str, remote_revision: Option<&str>) -> String {
    let token = match remote_revision {
        Some(revision) => revision
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect(),
        None => download::hex_lower(&Sha256::digest(remote_path.as_bytes()))[..16].to_owned(),
    };
    format!("asset-{token}")
}

/// Fetch `source` into the Local Working Copy at `destination`.
///
/// A destination that already verifies against `source` is kept as-is.
/// Otherwise the object downloads with range resume into the part file, is
/// verified against the expected size and the digest the path names, and
/// only a verified candidate replaces the destination. On a transfer error
/// the received prefix stays in the part file and the next call with the
/// same `operation_id` resumes from it; on a verification error the part
/// file is deleted because corrupt bytes can never resume into a valid
/// asset.
///
/// `on_progress` receives the durable byte count after every chunk and once
/// more before a transfer error is returned.
pub fn fetch_asset(
    storage: &dyn RepositoryStorage,
    source: &AssetSource,
    destination: &Path,
    operation_id: &str,
    on_progress: &mut dyn FnMut(u64),
) -> RemoteResult<FetchedAsset> {
    let has_expectation = source.expected_size.is_some() || source.expected_digest.is_some();
    if has_expectation && destination.is_file() {
        if let Ok(verified) = download::verify_file(
            destination,
            source.expected_size,
            source.expected_digest.as_deref(),
        ) {
            return Ok(FetchedAsset {
                remote_path: source.remote_path.clone(),
                size_bytes: verified.size,
                sha256: verified.digest,
                downloaded: false,
            });
        }
    }

    let part_path = download::part_path_for(destination, operation_id);
    download::download_object(
        storage,
        &source.remote_path,
        source.expected_size,
        &part_path,
        ASSET_DOWNLOAD_CHUNK_SIZE,
        on_progress,
    )?;
    let verified = match download::verify_file(
        &part_path,
        source.expected_size,
        source.expected_digest.as_deref(),
    ) {
        Ok(verified) => verified,
        Err(error) => {
            let _ = fs::remove_file(&part_path);
            return Err(error);
        }
    };
    download::install_verified(&part_path, destination)?;
    Ok(FetchedAsset {
        remote_path: source.remote_path.clone(),
        size_bytes: verified.size,
        sha256: verified.digest,
        downloaded: true,
    })
}

/// Reads byte ranges of one repository object.
///
/// Playback streaming starts from a `RangeReader` before a whole-asset
/// download finishes, and the media cache wraps the same `read_range` seam.
#[derive(Clone)]
pub struct RangeReader<S: RepositoryStorage> {
    storage: S,
    remote_path: String,
    size_bytes: u64,
    revision: Option<String>,
}

impl<S: RepositoryStorage> RangeReader<S> {
    /// Open the object at `remote_path` for range reads. Fails with
    /// `ProviderCapabilityUnavailable` when the storage cannot read ranges
    /// and with `ObjectNotFound` when the object is absent.
    pub fn open(storage: S, remote_path: impl Into<String>) -> RemoteResult<Self> {
        let remote_path = remote_path.into();
        validate_object_path(&remote_path)?;
        if !storage.capabilities().range_read {
            return Err(RemoteError::new(
                RemoteErrorKind::ProviderCapabilityUnavailable,
                "storage does not support range reads",
            ));
        }
        let metadata = storage.stat(&remote_path)?.ok_or_else(|| {
            RemoteError::new(
                RemoteErrorKind::ObjectNotFound,
                format!("no object at {remote_path}"),
            )
        })?;
        Ok(Self {
            storage,
            remote_path,
            size_bytes: metadata.size_bytes,
            revision: metadata.revision,
        })
    }

    pub fn remote_path(&self) -> &str {
        &self.remote_path
    }

    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// Revision of the object at open time, for cache identity.
    pub fn revision(&self) -> Option<&str> {
        self.revision.as_deref()
    }

    /// Read the bytes `[offset, offset + length)`, clamped at the end of the
    /// object.
    pub fn read_range(&self, offset: u64, length: u64) -> RemoteResult<Vec<u8>> {
        let mut bytes = Vec::new();
        self.storage
            .read_range(&self.remote_path, offset, length, &mut bytes)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{ListedObject, ObjectMetadata, StorageCapabilities};
    use crate::testing::{Fault, MemoryStorage, TestWebDavServer};
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    fn digest_of(bytes: &[u8]) -> String {
        download::hex_lower(&Sha256::digest(bytes))
    }

    fn content_path(digest: &str, extension: &str) -> String {
        format!("media/content/{digest}.{extension}")
    }

    fn seed_asset(storage: &MemoryStorage, bytes: &[u8]) -> String {
        let path = content_path(&digest_of(bytes), "flac");
        storage.write_new(&path, bytes).expect("seed asset");
        path
    }

    /// Records the windows every range read requests, to prove resume offsets.
    #[derive(Clone)]
    struct RecordingStorage {
        inner: MemoryStorage,
        ranges: Arc<Mutex<Vec<(u64, u64)>>>,
    }

    impl RecordingStorage {
        fn new(inner: MemoryStorage) -> Self {
            Self {
                inner,
                ranges: Arc::default(),
            }
        }

        fn recorded_ranges(&self) -> Vec<(u64, u64)> {
            self.ranges.lock().expect("ranges lock").clone()
        }
    }

    impl RepositoryStorage for RecordingStorage {
        fn capabilities(&self) -> StorageCapabilities {
            self.inner.capabilities()
        }

        fn stat(&self, path: &str) -> RemoteResult<Option<ObjectMetadata>> {
            self.inner.stat(path)
        }

        fn read(&self, path: &str, sink: &mut dyn Write) -> RemoteResult<u64> {
            self.inner.read(path, sink)
        }

        fn read_range(
            &self,
            path: &str,
            offset: u64,
            length: u64,
            sink: &mut dyn Write,
        ) -> RemoteResult<u64> {
            self.ranges
                .lock()
                .expect("ranges lock")
                .push((offset, length));
            self.inner.read_range(path, offset, length, sink)
        }

        fn write_new(&self, path: &str, bytes: &[u8]) -> RemoteResult<ObjectMetadata> {
            self.inner.write_new(path, bytes)
        }

        fn conditional_replace(
            &self,
            path: &str,
            bytes: &[u8],
            expected_revision: &str,
        ) -> RemoteResult<ObjectMetadata> {
            self.inner
                .conditional_replace(path, bytes, expected_revision)
        }

        fn list(&self, prefix: &str) -> RemoteResult<Vec<ListedObject>> {
            self.inner.list(prefix)
        }

        fn delete(&self, path: &str) -> RemoteResult<()> {
            self.inner.delete(path)
        }
    }

    fn ignore_progress(_: u64) {}

    #[test]
    fn content_digest_parsing() {
        let digest = "ab".repeat(32);
        assert_eq!(
            content_digest_from_asset_path(&format!("media/content/{digest}.flac")),
            Some(digest.clone())
        );
        assert_eq!(
            content_digest_from_asset_path(&format!("stems/content/{digest}.ogg")),
            Some(digest.clone())
        );
        assert_eq!(
            content_digest_from_asset_path(&format!("media-g/content/{digest}.zip")),
            Some(digest.clone())
        );
        assert_eq!(
            content_digest_from_asset_path(&format!(
                "media/content/{}.Flac",
                digest.to_uppercase()
            )),
            Some(digest.clone()),
            "digests compare case-insensitively"
        );

        for path in [
            "media/song.flac",
            "media/content/readme.md",
            "media/content/abcd.flac",
            "media/artwork/content/x.flac",
            "content/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.flac",
            "artwork/thumbs/song.jpg",
        ] {
            assert_eq!(
                content_digest_from_asset_path(path),
                None,
                "{path:?} is not content-addressed"
            );
        }
    }

    #[test]
    fn plan_fills_expectations_from_stat_and_path() {
        let storage = MemoryStorage::new();
        let bytes = b"audio-bytes".to_vec();
        let path = seed_asset(&storage, &bytes);

        let source = plan_asset_fetch(&storage, &path).expect("plan");

        assert_eq!(source.remote_path, path);
        assert_eq!(source.expected_size, Some(bytes.len() as u64));
        assert_eq!(source.expected_digest, Some(digest_of(&bytes)));
        assert!(source.revision.is_some());

        let legacy = plan_asset_fetch(&storage, "media/song.flac").expect("plan legacy");
        assert_eq!(legacy.expected_size, None, "an absent object has no size");
        assert_eq!(legacy.expected_digest, None);
        assert_eq!(legacy.revision, None);
    }

    #[test]
    fn fetch_installs_a_verified_asset() {
        let storage = MemoryStorage::new();
        let bytes = b"pretend-flac".to_vec();
        let path = seed_asset(&storage, &bytes);
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("media/content/x.flac");
        let source = plan_asset_fetch(&storage, &path).expect("plan");
        let operation_id = asset_operation_id(&path, source.revision.as_deref());

        let fetched = fetch_asset(
            &storage,
            &source,
            &destination,
            &operation_id,
            &mut ignore_progress,
        )
        .expect("fetch");

        assert!(fetched.downloaded);
        assert_eq!(fetched.size_bytes, bytes.len() as u64);
        assert_eq!(fetched.sha256, digest_of(&bytes));
        assert_eq!(fs::read(&destination).expect("destination"), bytes);
        assert!(
            !download::part_path_for(&destination, &operation_id).exists(),
            "the part file is consumed by the install"
        );
    }

    #[test]
    fn an_existing_verified_copy_is_reused_without_a_transfer() {
        let storage = MemoryStorage::new();
        let bytes = b"cached-audio".to_vec();
        let path = seed_asset(&storage, &bytes);
        let recording = RecordingStorage::new(storage);
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("song.flac");
        fs::write(&destination, &bytes).expect("seed destination");
        let source = plan_asset_fetch(&recording, &path).expect("plan");

        let fetched = fetch_asset(
            &recording,
            &source,
            &destination,
            "asset-reuse",
            &mut ignore_progress,
        )
        .expect("fetch");

        assert!(!fetched.downloaded);
        assert_eq!(fetched.sha256, digest_of(&bytes));
        assert!(
            recording.recorded_ranges().is_empty(),
            "a verified copy must not download"
        );
    }

    #[test]
    fn an_existing_copy_with_the_wrong_digest_is_downloaded_again() {
        let storage = MemoryStorage::new();
        let bytes = b"good-bytes".to_vec();
        let path = seed_asset(&storage, &bytes);
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("song.flac");
        fs::write(&destination, b"stale-bytes").expect("seed stale destination");
        let source = plan_asset_fetch(&storage, &path).expect("plan");

        let fetched = fetch_asset(
            &storage,
            &source,
            &destination,
            "asset-redo",
            &mut ignore_progress,
        )
        .expect("fetch");

        assert!(fetched.downloaded);
        assert_eq!(fs::read(&destination).expect("destination"), bytes);
    }

    #[test]
    fn an_interrupted_download_resumes_from_the_received_prefix() {
        let storage = MemoryStorage::new();
        let bytes: Vec<u8> = (0..100_000u32).flat_map(|n| n.to_le_bytes()).collect();
        let path = seed_asset(&storage, &bytes);
        let recording = RecordingStorage::new(storage);
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("song.flac");
        let source = plan_asset_fetch(&recording, &path).expect("plan");
        let operation_id = asset_operation_id(&path, source.revision.as_deref());
        let interrupt_at = bytes.len() as u64 / 2;
        recording.inner.script(Fault::InterruptTransfer {
            path: path.clone(),
            after_bytes: interrupt_at,
        });

        let error = fetch_asset(
            &recording,
            &source,
            &destination,
            &operation_id,
            &mut ignore_progress,
        )
        .expect_err("the scripted interruption fails the first attempt");
        assert_eq!(error.kind, RemoteErrorKind::NetworkUnavailable);
        assert!(error.retryable);
        assert_eq!(
            fs::metadata(download::part_path_for(&destination, &operation_id))
                .expect("part file")
                .len(),
            interrupt_at,
            "the received prefix stays on disk"
        );

        let fetched = fetch_asset(
            &recording,
            &source,
            &destination,
            &operation_id,
            &mut ignore_progress,
        )
        .expect("resume");
        assert!(fetched.downloaded);
        assert_eq!(fs::read(&destination).expect("destination"), bytes);

        let ranges = recording.recorded_ranges();
        assert_eq!(
            ranges[1],
            (interrupt_at, bytes.len() as u64 - interrupt_at),
            "the retry resumes at the durable prefix instead of starting over"
        );
    }

    #[test]
    fn a_digest_mismatch_deletes_the_part_file_and_never_touches_the_destination() {
        let storage = MemoryStorage::new();
        // The path names a digest the stored bytes do not have.
        let path = content_path(&"00".repeat(32), "flac");
        storage
            .write_new(&path, b"not-the-named-digest")
            .expect("seed asset");
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("song.flac");
        let source = plan_asset_fetch(&storage, &path).expect("plan");

        let error = fetch_asset(
            &storage,
            &source,
            &destination,
            "asset-bad",
            &mut ignore_progress,
        )
        .expect_err("bytes that do not hash to the named digest must fail");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert!(!error.retryable);
        assert!(!destination.exists());
        assert!(
            !download::part_path_for(&destination, "asset-bad").exists(),
            "corrupt bytes are deleted: they can never resume into a valid asset"
        );
    }

    #[test]
    fn a_failed_refetch_keeps_the_previous_copy() {
        let storage = MemoryStorage::new();
        let bytes_v1 = b"version-one".to_vec();
        let path_v1 = seed_asset(&storage, &bytes_v1);
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("song.flac");
        let source_v1 = plan_asset_fetch(&storage, &path_v1).expect("plan v1");
        fetch_asset(
            &storage,
            &source_v1,
            &destination,
            "asset-v1",
            &mut ignore_progress,
        )
        .expect("first fetch");

        // The next generation of the song arrives under its own digest path;
        // its download is interrupted.
        let bytes_v2 = b"version-two".to_vec();
        let path_v2 = seed_asset(&storage, &bytes_v2);
        let source_v2 = plan_asset_fetch(&storage, &path_v2).expect("plan v2");
        storage.script(Fault::InterruptTransfer {
            path: path_v2,
            after_bytes: 2,
        });

        let error = fetch_asset(
            &storage,
            &source_v2,
            &destination,
            "asset-v2",
            &mut ignore_progress,
        )
        .expect_err("the interrupted download fails the refetch");

        assert_eq!(error.kind, RemoteErrorKind::NetworkUnavailable);
        assert_eq!(
            fs::read(&destination).expect("destination"),
            bytes_v1,
            "the previous verified copy survives a failed refetch"
        );
    }

    #[test]
    fn a_revision_moving_mid_download_fails_integrity_and_keeps_the_destination() {
        let storage = MemoryStorage::new();
        // Two full-chunk assets: the revision moves after chunk one, so the
        // part file splices bytes from two revisions.
        let bytes_v1 = vec![0x11u8; 9 * 1024 * 1024];
        let bytes_v2 = vec![0x22u8; 9 * 1024 * 1024];
        assert!(bytes_v1.len() as u64 > ASSET_DOWNLOAD_CHUNK_SIZE);
        let path = seed_asset(&storage, &bytes_v1);
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("song.flac");
        let source = plan_asset_fetch(&storage, &path).expect("plan");
        storage.script(Fault::MoveRevision {
            path: path.clone(),
            bytes: bytes_v2,
        });

        let error = fetch_asset(
            &storage,
            &source,
            &destination,
            "asset-move",
            &mut ignore_progress,
        )
        .expect_err("bytes from two revisions can never verify");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert!(!destination.exists());
        assert!(
            !download::part_path_for(&destination, "asset-move").exists(),
            "corrupt bytes are deleted: they can never resume into a valid asset"
        );
    }

    #[test]
    fn a_non_content_addressed_path_verifies_size_only() {
        let storage = MemoryStorage::new();
        let bytes = b"legacy-song".to_vec();
        storage
            .write_new("media/song.flac", &bytes)
            .expect("seed legacy asset");
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("song.flac");
        let source = plan_asset_fetch(&storage, "media/song.flac").expect("plan");
        assert_eq!(source.expected_digest, None);

        let fetched = fetch_asset(
            &storage,
            &source,
            &destination,
            "asset-legacy",
            &mut ignore_progress,
        )
        .expect("fetch");

        assert!(fetched.downloaded);
        assert_eq!(fetched.sha256, digest_of(&bytes));
        assert_eq!(fs::read(&destination).expect("destination"), bytes);
    }

    #[test]
    fn a_size_mismatch_is_an_integrity_failure() {
        let storage = MemoryStorage::new();
        let bytes = b"some-bytes".to_vec();
        storage
            .write_new("media/song.flac", &bytes)
            .expect("seed asset");
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("song.flac");
        let mut source = plan_asset_fetch(&storage, "media/song.flac").expect("plan");
        source.expected_size = Some(bytes.len() as u64 + 64);

        let error = fetch_asset(
            &storage,
            &source,
            &destination,
            "asset-size",
            &mut ignore_progress,
        )
        .expect_err("a truncated asset must fail");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert!(!destination.exists());
    }

    #[test]
    fn a_missing_asset_surfaces_object_not_found() {
        let storage = MemoryStorage::new();
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("song.flac");
        let source = plan_asset_fetch(&storage, "media/content/aa.flac").expect("plan");

        let error = fetch_asset(
            &storage,
            &source,
            &destination,
            "asset-missing",
            &mut ignore_progress,
        )
        .expect_err("the object is absent");

        assert_eq!(error.kind, RemoteErrorKind::ObjectNotFound);
        assert_eq!(error.code, "remote_object_not_found");
        assert!(!destination.exists());
    }

    #[test]
    fn storage_without_range_reads_downloads_the_whole_file_without_resume() {
        let storage = MemoryStorage::with_capabilities(StorageCapabilities {
            conditional_replace: true,
            range_read: false,
        });
        let bytes = b"whole-file".to_vec();
        let path = seed_asset(&storage, &bytes);
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("song.flac");
        let source = plan_asset_fetch(&storage, &path).expect("plan");

        let fetched = fetch_asset(
            &storage,
            &source,
            &destination,
            "asset-whole",
            &mut ignore_progress,
        )
        .expect("fetch");
        assert_eq!(fetched.sha256, digest_of(&bytes));

        // An interrupted whole-file download leaves nothing resumable.
        let destination2 = dir.path().join("song2.flac");
        let bytes2 = b"whole-file-two".to_vec();
        let path2 = seed_asset(&storage, &bytes2);
        let source2 = plan_asset_fetch(&storage, &path2).expect("plan");
        storage.script(Fault::InterruptTransfer {
            path: path2,
            after_bytes: 3,
        });
        let error = fetch_asset(
            &storage,
            &source2,
            &destination2,
            "asset-whole-2",
            &mut ignore_progress,
        )
        .expect_err("the whole-file read is interrupted");
        assert_eq!(error.kind, RemoteErrorKind::NetworkUnavailable);
        assert!(
            !download::part_path_for(&destination2, "asset-whole-2").exists(),
            "without range reads a partial file can never resume"
        );
    }

    #[test]
    fn progress_reports_durable_bytes() {
        let storage = MemoryStorage::new();
        let bytes: Vec<u8> = (0..1000u32).flat_map(|n| n.to_le_bytes()).collect();
        let path = seed_asset(&storage, &bytes);
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("song.flac");
        let source = plan_asset_fetch(&storage, &path).expect("plan");

        let mut progress = Vec::new();
        fetch_asset(
            &storage,
            &source,
            &destination,
            "asset-progress",
            &mut |transferred| progress.push(transferred),
        )
        .expect("fetch");

        assert_eq!(progress, vec![bytes.len() as u64]);
    }

    #[test]
    fn range_reader_reads_windows_of_the_object() {
        let storage = MemoryStorage::new();
        let bytes: Vec<u8> = (0..255u8).cycle().take(4096).collect();
        let path = seed_asset(&storage, &bytes);

        let reader = RangeReader::open(storage, path.clone()).expect("open");

        assert_eq!(reader.remote_path(), path);
        assert_eq!(reader.size_bytes(), bytes.len() as u64);
        assert_eq!(reader.read_range(0, 16).expect("head"), bytes[..16]);
        assert_eq!(
            reader.read_range(1000, 500).expect("middle"),
            bytes[1000..1500]
        );
        assert_eq!(
            reader.read_range(4000, 500).expect("clamped"),
            bytes[4000..],
            "a window past the end is clamped"
        );
        assert!(reader.read_range(4096, 10).is_err(), "past the end fails");
    }

    #[test]
    fn range_reader_requires_the_range_read_capability() {
        let storage = MemoryStorage::with_capabilities(StorageCapabilities {
            conditional_replace: true,
            range_read: false,
        });
        let bytes = b"no-ranges".to_vec();
        let path = seed_asset(&storage, &bytes);

        let error = RangeReader::open(storage, path)
            .map(drop)
            .expect_err("no range capability");

        assert_eq!(error.kind, RemoteErrorKind::ProviderCapabilityUnavailable);
    }

    #[test]
    fn range_reader_reports_a_missing_object() {
        let storage = MemoryStorage::new();

        let error = RangeReader::open(storage, "media/content/aa.flac")
            .map(drop)
            .expect_err("object is absent");

        assert_eq!(error.kind, RemoteErrorKind::ObjectNotFound);
    }

    mod webdav {
        use super::*;

        #[test]
        fn fetch_over_webdav_installs_then_reuses_without_a_transfer() {
            let server = TestWebDavServer::start();
            let bytes = b"webdav-audio".to_vec();
            let digest = digest_of(&bytes);
            let path = content_path(&digest, "flac");
            server.storage().write_new(&path, &bytes).expect("seed");
            let dir = tempfile::tempdir().expect("temp dir");
            let destination = dir.path().join("song.flac");
            let storage = server.storage().clone();
            let source = plan_asset_fetch(&storage, &path).expect("plan");
            let operation_id = asset_operation_id(&path, source.revision.as_deref());

            let fetched = fetch_asset(
                &storage,
                &source,
                &destination,
                &operation_id,
                &mut ignore_progress,
            )
            .expect("fetch");
            assert!(fetched.downloaded);
            assert_eq!(fs::read(&destination).expect("destination"), bytes);
            let gets_after_fetch = server.get_request_count(&path);
            assert!(gets_after_fetch > 0);

            let reused = fetch_asset(
                &storage,
                &source,
                &destination,
                &operation_id,
                &mut ignore_progress,
            )
            .expect("reuse");
            assert!(!reused.downloaded);
            assert_eq!(
                server.get_request_count(&path),
                gets_after_fetch,
                "a verified copy must not re-download"
            );
        }

        #[test]
        fn an_interrupted_webdav_download_resumes() {
            let server = TestWebDavServer::start();
            let bytes: Vec<u8> = (0..100_000u32).flat_map(|n| n.to_le_bytes()).collect();
            let path = content_path(&digest_of(&bytes), "flac");
            server.storage().write_new(&path, &bytes).expect("seed");
            let dir = tempfile::tempdir().expect("temp dir");
            let destination = dir.path().join("song.flac");
            let storage = server.storage().clone();
            let source = plan_asset_fetch(&storage, &path).expect("plan");
            let operation_id = asset_operation_id(&path, source.revision.as_deref());
            server.script(Fault::InterruptTransfer {
                path: path.clone(),
                after_bytes: bytes.len() as u64 / 2,
            });

            let error = fetch_asset(
                &storage,
                &source,
                &destination,
                &operation_id,
                &mut ignore_progress,
            )
            .expect_err("the scripted interruption fails the first attempt");
            assert_eq!(error.kind, RemoteErrorKind::NetworkUnavailable);

            let fetched = fetch_asset(
                &storage,
                &source,
                &destination,
                &operation_id,
                &mut ignore_progress,
            )
            .expect("the retry resumes and completes");
            assert_eq!(fetched.sha256, digest_of(&bytes));
            assert_eq!(fs::read(&destination).expect("destination"), bytes);
        }

        #[test]
        fn range_reader_streams_windows_over_webdav() {
            let server = TestWebDavServer::start();
            let bytes: Vec<u8> = (0..255u8).cycle().take(8192).collect();
            let path = content_path(&digest_of(&bytes), "ogg");
            server.storage().write_new(&path, &bytes).expect("seed");

            let reader = RangeReader::open(server.storage().clone(), path).expect("open");

            assert_eq!(reader.size_bytes(), bytes.len() as u64);
            assert_eq!(
                reader.read_range(2048, 1024).expect("window"),
                bytes[2048..3072]
            );
        }
    }
}
