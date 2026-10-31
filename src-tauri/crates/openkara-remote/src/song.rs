//! Publish Song: put the assets one song references on the remote.
//!
//! Audio and stems live under content-addressed names
//! (`<top>/content/<sha256>.<ext>`) and artwork derivatives under names derived
//! from their bytes, so a path already identifies its content. An object the
//! remote holds at the right size is skipped: re-publishing a song transfers
//! nothing, and songs that share bytes share one object. Bytes are never
//! stored under a content-addressed name they do not hash to.
//!
//! The song's database rows reach the remote with the next
//! [`publish_changes`](crate::writer::publish_changes), which finds these
//! assets present and only commits the snapshot and the manifest.
//!
//! An interrupted upload leaves at most a torn object at its final path
//! (`write_new` never replaces). Publishing the song again deletes and
//! rewrites only that object; the assets that already landed are skipped.

use std::path::Path;

use crate::download::integrity_error;
use crate::error::{RemoteError, RemoteErrorKind, RemoteResult};
use crate::reader::open_readonly;
use crate::storage::RepositoryStorage;
use crate::writer::{ensure_asset, referenced_asset_paths, AssetUpload};

/// The song's referenced asset paths, split by whether this call transferred
/// them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublishedSongAssets {
    pub uploaded: Vec<String>,
    pub already_present: Vec<String>,
}

/// Upload every asset `song_hash` references in `working_database` that the
/// remote lacks. Asset paths resolve against `working_copy_root`, in sorted
/// order so a retry after an interruption meets the same sequence.
///
/// Fails with `RemoteIntegrityFailed` when the song is not in the working
/// database, when a referenced asset is missing locally and remotely, or when
/// local bytes do not match their content-addressed name; transfer failures
/// propagate unchanged so the caller can retry.
pub fn publish_song_assets(
    storage: &dyn RepositoryStorage,
    working_copy_root: &Path,
    working_database: &Path,
    song_hash: &str,
) -> RemoteResult<PublishedSongAssets> {
    if !storage.capabilities().can_publish() {
        return Err(RemoteError::new(
            RemoteErrorKind::ProviderCapabilityUnavailable,
            "storage does not support conditional replace; publication is blocked",
        ));
    }
    if !song_exists(working_database, song_hash)? {
        return Err(integrity_error(format!(
            "song {song_hash} is not in the working copy database"
        )));
    }

    let mut published = PublishedSongAssets::default();
    for path in referenced_asset_paths(working_database, Some(song_hash))? {
        match ensure_asset(storage, working_copy_root, &path)? {
            AssetUpload::Uploaded => published.uploaded.push(path),
            AssetUpload::AlreadyPresent => published.already_present.push(path),
        }
    }
    Ok(published)
}

fn song_exists(working_database: &Path, song_hash: &str) -> RemoteResult<bool> {
    let connection = open_readonly(working_database)?;
    connection
        .prepare("SELECT 1 FROM songs WHERE hash = ?1 LIMIT 1")
        .and_then(|mut statement| statement.exists([song_hash]))
        .map_err(|error| integrity_error(format!("song lookup failed: {error}")))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use rusqlite::{params, Connection};
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::download::hex_lower;
    use crate::manifest::{RepositoryManifest, MANIFEST_PATH};
    use crate::reader::{refresh_repository, RefreshOutcome, RefreshPolicy, WorkingCopyDatabase};
    use crate::storage::{ListedObject, ObjectMetadata, StorageCapabilities};
    use crate::testing::{Fault, MemoryStorage, TestWebDavServer};
    use crate::writer::{publish_changes, PublishOutcome, PublishRequest};

    /// One song's local assets: the files exist in the working copy under the
    /// names the rows reference.
    struct LocalSong {
        hash: &'static str,
        audio: Option<Vec<u8>>,
        stems: Option<[Vec<u8>; 5]>,
        artwork: Option<(Vec<u8>, Vec<u8>)>,
    }

    impl LocalSong {
        fn original(hash: &'static str, audio: &[u8]) -> Self {
            Self {
                hash,
                audio: Some(audio.to_vec()),
                stems: None,
                artwork: None,
            }
        }

        fn separated(hash: &'static str, seed: u8) -> Self {
            Self {
                hash,
                audio: None,
                stems: Some(std::array::from_fn(|index| {
                    vec![seed.wrapping_add(index as u8); 48 + index]
                })),
                artwork: Some((vec![seed; 20], vec![seed; 60])),
            }
        }

        fn audio_path(&self) -> Option<String> {
            self.audio
                .as_deref()
                .map(|bytes| content_path("media", bytes, "flac"))
        }

        fn stem_paths(&self) -> Option<[String; 5]> {
            self.stems.as_ref().map(|stems| {
                std::array::from_fn(|index| content_path("stems", &stems[index], "ogg"))
            })
        }

        fn artwork_paths(&self) -> Option<(String, String)> {
            self.artwork.as_ref().map(|(thumb, preview)| {
                (
                    format!("artwork/thumb_{}_80.webp", digest_of(thumb)),
                    format!("artwork/preview_{}_256.webp", digest_of(preview)),
                )
            })
        }

        fn files(&self) -> Vec<(String, Vec<u8>)> {
            let mut files = Vec::new();
            if let (Some(path), Some(bytes)) = (self.audio_path(), &self.audio) {
                files.push((path, bytes.clone()));
            }
            if let (Some(paths), Some(stems)) = (self.stem_paths(), &self.stems) {
                files.extend(paths.into_iter().zip(stems.iter().cloned()));
            }
            if let (Some((thumb_path, preview_path)), Some((thumb, preview))) =
                (self.artwork_paths(), &self.artwork)
            {
                files.push((thumb_path, thumb.clone()));
                files.push((preview_path, preview.clone()));
            }
            files.sort();
            files
        }

        fn remote_paths(&self) -> Vec<String> {
            self.files().into_iter().map(|(path, _)| path).collect()
        }
    }

    fn content_path(top_level: &str, bytes: &[u8], extension: &str) -> String {
        format!("{top_level}/content/{}.{extension}", digest_of(bytes))
    }

    fn digest_of(bytes: &[u8]) -> String {
        hex_lower(&Sha256::digest(bytes))
    }

    fn working_copy(songs: &[LocalSong]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let connection = Connection::open(dir.path().join("openkara.db")).expect("database");
        connection
            .execute_batch(
                "CREATE TABLE songs (
                    hash TEXT PRIMARY KEY,
                    title TEXT,
                    file_path TEXT,
                    cdg_path TEXT,
                    artwork_thumb_path TEXT,
                    artwork_preview_path TEXT,
                    audio_source_kind TEXT NOT NULL DEFAULT 'original',
                    imported_at INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE stems (
                    song_hash TEXT PRIMARY KEY,
                    vocals_path TEXT,
                    accomp_path TEXT,
                    drums_path TEXT,
                    bass_path TEXT,
                    other_path TEXT
                );",
            )
            .expect("schema");
        for song in songs {
            let (thumb, preview) = song.artwork_paths().unzip();
            connection
                .execute(
                    "INSERT INTO songs (hash, title, file_path, artwork_thumb_path, \
                     artwork_preview_path) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![song.hash, song.hash, song.audio_path(), thumb, preview],
                )
                .expect("insert song");
            if let Some(stems) = song.stem_paths() {
                connection
                    .execute(
                        "INSERT INTO stems (song_hash, vocals_path, accomp_path, drums_path, \
                         bass_path, other_path) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![song.hash, stems[0], stems[1], stems[2], stems[3], stems[4]],
                    )
                    .expect("insert stems");
            }
            for (path, bytes) in song.files() {
                let local = dir.path().join(&path);
                fs::create_dir_all(local.parent().expect("parent")).expect("asset dirs");
                fs::write(local, bytes).expect("asset");
            }
        }
        dir
    }

    fn publish_song(
        storage: &dyn RepositoryStorage,
        dir: &tempfile::TempDir,
        song_hash: &str,
    ) -> RemoteResult<PublishedSongAssets> {
        publish_song_assets(
            storage,
            dir.path(),
            &dir.path().join("openkara.db"),
            song_hash,
        )
    }

    fn read_all(storage: &dyn RepositoryStorage, path: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        storage.read(path, &mut bytes).expect("read object");
        bytes
    }

    fn remote_paths(storage: &dyn RepositoryStorage) -> Vec<String> {
        storage
            .list("")
            .expect("list")
            .into_iter()
            .map(|object| object.path)
            .collect()
    }

    /// Records every `write_new` so tests can prove what a call transferred.
    struct WriteRecordingStorage {
        inner: MemoryStorage,
        writes: Arc<Mutex<Vec<String>>>,
    }

    impl WriteRecordingStorage {
        fn new(inner: MemoryStorage) -> Self {
            Self {
                inner,
                writes: Arc::default(),
            }
        }

        fn take_writes(&self) -> Vec<String> {
            std::mem::take(&mut *self.writes.lock().expect("writes lock"))
        }
    }

    impl RepositoryStorage for WriteRecordingStorage {
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
            self.inner.read_range(path, offset, length, sink)
        }
        fn write_new(&self, path: &str, bytes: &[u8]) -> RemoteResult<ObjectMetadata> {
            self.writes
                .lock()
                .expect("writes lock")
                .push(path.to_owned());
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

    struct AllowAll;
    impl RefreshPolicy for AllowAll {
        fn may_replace_working_copy(&self) -> bool {
            true
        }
    }

    #[test]
    fn publishing_a_song_uploads_its_stems_and_artwork_derivatives() {
        let storage = MemoryStorage::new();
        let song = LocalSong::separated("song-1", 1);
        let dir = working_copy(&[song]);
        let song = LocalSong::separated("song-1", 1);

        let published = publish_song(&storage, &dir, "song-1").expect("publish song");

        assert_eq!(published.uploaded, song.remote_paths());
        assert!(published.already_present.is_empty());
        for (path, bytes) in song.files() {
            assert_eq!(read_all(&storage, &path), bytes, "{path}");
        }
        assert_eq!(remote_paths(&storage), song.remote_paths());
    }

    #[test]
    fn publishing_a_song_uploads_its_original_audio() {
        let storage = MemoryStorage::new();
        let dir = working_copy(&[LocalSong::original("song-1", b"original audio")]);

        let published = publish_song(&storage, &dir, "song-1").expect("publish song");

        let audio_path = content_path("media", b"original audio", "flac");
        assert_eq!(published.uploaded, std::slice::from_ref(&audio_path));
        assert_eq!(read_all(&storage, &audio_path), b"original audio");
    }

    #[test]
    fn republishing_a_song_transfers_nothing() {
        let storage = WriteRecordingStorage::new(MemoryStorage::new());
        let dir = working_copy(&[LocalSong::separated("song-1", 1)]);
        publish_song(&storage, &dir, "song-1").expect("first publish");
        storage.take_writes();

        let published = publish_song(&storage, &dir, "song-1").expect("republish");

        assert!(storage.take_writes().is_empty(), "nothing is re-uploaded");
        assert!(published.uploaded.is_empty());
        assert_eq!(
            published.already_present,
            LocalSong::separated("song-1", 1).remote_paths()
        );
    }

    #[test]
    fn songs_that_share_bytes_store_one_object() {
        let storage = WriteRecordingStorage::new(MemoryStorage::new());
        let dir = working_copy(&[
            LocalSong::original("song-1", b"the same recording"),
            LocalSong::original("song-2", b"the same recording"),
        ]);
        publish_song(&storage, &dir, "song-1").expect("first song");
        storage.take_writes();

        let published = publish_song(&storage, &dir, "song-2").expect("second song");

        let shared = content_path("media", b"the same recording", "flac");
        assert!(storage.take_writes().is_empty());
        assert_eq!(published.already_present, std::slice::from_ref(&shared));
        assert_eq!(remote_paths(&storage.inner), [shared]);
    }

    #[test]
    fn only_the_named_song_is_published() {
        let storage = MemoryStorage::new();
        let dir = working_copy(&[
            LocalSong::original("song-1", b"first song"),
            LocalSong::original("song-2", b"second song"),
        ]);

        publish_song(&storage, &dir, "song-1").expect("publish song 1");

        assert_eq!(
            remote_paths(&storage),
            [content_path("media", b"first song", "flac")]
        );
    }

    #[test]
    fn an_upload_interrupted_mid_object_resumes_by_rewriting_only_the_torn_asset() {
        let memory = MemoryStorage::new();
        let storage = WriteRecordingStorage::new(memory.clone());
        let song = LocalSong::separated("song-1", 7);
        let dir = working_copy(&[LocalSong::separated("song-1", 7)]);
        let paths = song.remote_paths();
        let torn = paths[3].clone();
        memory.script(Fault::PartialWrite {
            path: torn.clone(),
            stored_bytes: 5,
        });

        let error = publish_song(&storage, &dir, "song-1").expect_err("interrupted");
        assert!(
            error.retryable,
            "an interrupted upload is retryable: {error}"
        );
        assert_eq!(read_all(&memory, &torn).len(), 5, "a torn object is left");
        let attempted = storage.take_writes();
        assert_eq!(attempted, paths[..=3]);

        let published = publish_song(&storage, &dir, "song-1").expect("resume");

        assert_eq!(
            storage.take_writes()[0],
            torn,
            "the torn asset is rewritten"
        );
        assert_eq!(published.already_present, paths[..3]);
        assert_eq!(published.uploaded, paths[3..]);
        for (path, bytes) in song.files() {
            assert_eq!(read_all(&memory, &path), bytes, "{path}");
        }
    }

    #[test]
    fn an_interrupted_artwork_derivative_upload_resumes() {
        let storage = MemoryStorage::new();
        let song = LocalSong::separated("song-1", 5);
        let dir = working_copy(&[LocalSong::separated("song-1", 5)]);
        let (thumb, _) = song.artwork_paths().expect("artwork");
        storage.script(Fault::PartialWrite {
            path: thumb.clone(),
            stored_bytes: 3,
        });
        publish_song(&storage, &dir, "song-1").expect_err("interrupted");

        let published = publish_song(&storage, &dir, "song-1").expect("resume");

        assert!(published.uploaded.contains(&thumb));
        for (path, bytes) in song.files() {
            assert_eq!(read_all(&storage, &path), bytes, "{path}");
        }
    }

    #[test]
    fn a_song_published_after_an_interruption_commits_and_installs() {
        let storage = MemoryStorage::new();
        let song = LocalSong::separated("song-1", 3);
        let dir = working_copy(&[LocalSong::separated("song-1", 3)]);
        let paths = song.remote_paths();
        storage.script(Fault::PartialWrite {
            path: paths[1].clone(),
            stored_bytes: 1,
        });
        publish_song(&storage, &dir, "song-1").expect_err("interrupted");

        publish_song(&storage, &dir, "song-1").expect("resume");
        let request = PublishRequest {
            repository_id: "repo-1".to_owned(),
            writer_id: "writer-1".to_owned(),
            operation_id: "op-1".to_owned(),
            expected_generation: 0,
        };
        let outcome = publish_changes(
            &storage,
            dir.path(),
            &dir.path().join("openkara.db"),
            &request,
            &mut |_| {},
        )
        .expect("publish changes");

        assert!(matches!(outcome, PublishOutcome::Committed(_)));
        let manifest_bytes = read_all(&storage, MANIFEST_PATH);
        let manifest =
            RepositoryManifest::from_json(std::str::from_utf8(&manifest_bytes).expect("utf8"))
                .expect("manifest");
        assert_eq!(manifest.generation, 1);
        let pulled = tempfile::tempdir().expect("temp dir");
        let working_copy = WorkingCopyDatabase::new(pulled.path().join("openkara.db"));
        let refreshed =
            refresh_repository(&storage, &working_copy, None, &AllowAll).expect("refresh");
        assert!(matches!(refreshed, RefreshOutcome::Refreshed { .. }));
        for (path, bytes) in song.files() {
            assert_eq!(read_all(&storage, &path), bytes, "{path}");
        }
    }

    #[test]
    fn local_bytes_that_do_not_match_their_content_address_are_never_uploaded() {
        let storage = MemoryStorage::new();
        let dir = working_copy(&[LocalSong::original("song-1", b"expected bytes")]);
        let audio_path = content_path("media", b"expected bytes", "flac");
        fs::write(dir.path().join(&audio_path), b"swapped bytes!").expect("overwrite");

        let error = publish_song(&storage, &dir, "song-1").expect_err("must fail");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert!(remote_paths(&storage).is_empty());
    }

    #[test]
    fn a_song_missing_from_the_working_copy_fails() {
        let storage = MemoryStorage::new();
        let dir = working_copy(&[]);

        let error = publish_song(&storage, &dir, "song-1").expect_err("unknown song");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
    }

    #[test]
    fn a_storage_without_conditional_replace_cannot_publish_a_song() {
        let storage = MemoryStorage::with_capabilities(StorageCapabilities {
            conditional_replace: false,
            range_read: true,
        });
        let dir = working_copy(&[LocalSong::original("song-1", b"audio")]);

        let error = publish_song(&storage, &dir, "song-1").expect_err("read-only storage");

        assert_eq!(error.kind, RemoteErrorKind::ProviderCapabilityUnavailable);
        assert!(remote_paths(&storage).is_empty());
    }

    #[test]
    fn an_interrupted_song_upload_over_webdav_resumes_without_reuploading() {
        let server = TestWebDavServer::start();
        let storage = server.storage().clone();
        let song = LocalSong::separated("song-1", 9);
        let dir = working_copy(&[LocalSong::separated("song-1", 9)]);
        let paths = song.remote_paths();
        let torn = paths[2].clone();
        server.script(Fault::PartialWrite {
            path: torn.clone(),
            stored_bytes: 4,
        });

        let error = publish_song(&storage, &dir, "song-1").expect_err("interrupted");
        assert!(
            error.retryable,
            "an interrupted upload is retryable: {error}"
        );

        let published = publish_song(&storage, &dir, "song-1").expect("resume");
        assert_eq!(published.already_present, paths[..2]);
        assert_eq!(published.uploaded, paths[2..]);
        publish_song(&storage, &dir, "song-1").expect("republish");

        for (path, bytes) in song.files() {
            assert_eq!(server.file(&path), Some(bytes), "{path}");
            let expected_puts = if path == torn { 2 } else { 1 };
            assert_eq!(server.put_request_count(&path), expected_puts, "{path}");
        }
    }
}
