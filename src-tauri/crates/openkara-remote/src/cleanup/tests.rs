use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::*;
use crate::asset::{asset_operation_id, fetch_asset, plan_asset_fetch};
use crate::download::hex_lower;
use crate::manifest::database_path_for_operation;
use crate::reader::{refresh_repository, RefreshOutcome, RefreshPolicy, WorkingCopyDatabase};
use crate::rebase::{ChangeSet, ConflictPolicy};
use crate::storage::{ObjectMetadata, ReadOnlyStorage, StorageCapabilities};
use crate::testing::{Fault, MemoryStorage, TestWebDavServer};
use crate::writer::{ensure_referenced_assets, publish_changes, PublishRequest};

const SECOND_MS: i64 = 1000;
const HOUR_MS: i64 = 60 * 60 * SECOND_MS;
const GRACE_MS: i64 = DEFAULT_CLEANUP_GRACE_PERIOD_MS;

/// Test clocks run in the past, in whole seconds (WebDAV dates have no
/// milliseconds), well before the real clock the writer stamps manifests with.
fn base_clock_ms() -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_millis() as i64;
    (now - 60 * 24 * HOUR_MS) / SECOND_MS * SECOND_MS
}

struct AllowAll;
impl RefreshPolicy for AllowAll {
    fn may_replace_working_copy(&self) -> bool {
        true
    }
}

/// A desktop Local Working Copy with the schema subset the writer reads.
struct Desktop {
    dir: TempDir,
    operation: u32,
    generation: i64,
}

impl Desktop {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        Connection::open(dir.path().join("openkara.db"))
            .expect("create working database")
            .execute_batch(
                "CREATE TABLE songs (
                    hash TEXT PRIMARY KEY,
                    title TEXT,
                    file_path TEXT
                );",
            )
            .expect("create schema");
        Self {
            dir,
            operation: 0,
            generation: 0,
        }
    }

    fn database(&self) -> PathBuf {
        self.dir.path().join("openkara.db")
    }

    fn connection(&self) -> Connection {
        Connection::open(self.database()).expect("open working database")
    }

    /// Imports a song and returns its content-addressed media path.
    fn add_song(&self, hash: &str) -> String {
        let bytes = format!("audio of {hash}").into_bytes();
        let path = format!("media/content/{}.flac", hex_lower(&Sha256::digest(&bytes)));
        let local = self.dir.path().join(&path);
        fs::create_dir_all(local.parent().expect("media dir")).expect("media dirs");
        fs::write(local, bytes).expect("write media");
        self.connection()
            .execute(
                "INSERT INTO songs (hash, title, file_path) VALUES (?1, ?1, ?2)",
                params![hash, path],
            )
            .expect("insert song");
        path
    }

    fn remove_song(&self, hash: &str) {
        self.connection()
            .execute("DELETE FROM songs WHERE hash = ?1", params![hash])
            .expect("delete song");
    }

    fn publish(&mut self, storage: &dyn RepositoryStorage) -> i64 {
        self.operation += 1;
        let request = PublishRequest {
            repository_id: "repo-1".to_owned(),
            writer_id: "desktop".to_owned(),
            operation_id: format!("op-{}", self.operation),
            expected_generation: self.generation,
            changes: ChangeSet::default(),
            conflict_policy: ConflictPolicy::Stop,
        };
        let outcome = publish_changes(
            storage,
            self.dir.path(),
            &self.database(),
            &request,
            &mut |_| {},
        )
        .expect("publish");
        self.generation = outcome.snapshot().expect("committed snapshot").generation;
        self.generation
    }
}

fn clean_up(storage: &dyn RepositoryStorage, now_ms: i64) -> RemoteResult<CleanupReport> {
    let scratch = tempfile::tempdir().expect("scratch dir");
    clean_up_unreferenced_uploads(storage, scratch.path(), &CleanupPolicy::default(), now_ms)
}

fn listed_paths(storage: &dyn RepositoryStorage) -> Vec<String> {
    storage
        .list("")
        .expect("list")
        .into_iter()
        .map(|object| object.path)
        .collect()
}

fn current_manifest(storage: &dyn RepositoryStorage) -> RepositoryManifest {
    read_current_manifest(storage)
        .expect("read manifest")
        .expect("manifest exists")
        .0
}

fn sorted(paths: &[&str]) -> Vec<String> {
    let mut paths: Vec<String> = paths.iter().map(|path| (*path).to_owned()).collect();
    paths.sort();
    paths
}

/// Refresh a new working copy and fetch every song it lists, the way a phone
/// opening the repository would.
fn read_like_a_phone(storage: &dyn RepositoryStorage) -> Vec<String> {
    let dir = tempfile::tempdir().expect("phone dir");
    let working_copy = WorkingCopyDatabase::new(dir.path().join("openkara.db"));
    let outcome = refresh_repository(storage, &working_copy, None, &AllowAll).expect("refresh");
    assert!(matches!(outcome, RefreshOutcome::Refreshed { .. }));
    fetch_every_song(storage, dir.path(), working_copy.path())
}

fn fetch_every_song(storage: &dyn RepositoryStorage, root: &Path, database: &Path) -> Vec<String> {
    let paths = referenced_asset_paths(database, None).expect("referenced assets");
    for path in &paths {
        let source = plan_asset_fetch(storage, path).expect("plan fetch");
        fetch_asset(
            storage,
            &source,
            &root.join(path),
            &asset_operation_id(path, source.revision.as_deref()),
            &mut |_| {},
        )
        .unwrap_or_else(|error| panic!("fetching {path} failed: {error}"));
    }
    paths
}

/// A repository with three generations published an hour apart: song `a`
/// throughout, `b` removed in generation 2, `c` added in generation 3.
struct ThreeGenerations {
    storage: MemoryStorage,
    song_b: String,
    first_snapshot: String,
    last_publish_ms: i64,
}

fn three_generations(storage: MemoryStorage, start_ms: i64, step_ms: i64) -> ThreeGenerations {
    let mut desktop = Desktop::new();
    storage.set_clock_ms(start_ms);
    desktop.add_song("a");
    let song_b = desktop.add_song("b");
    desktop.publish(&storage);
    let first_snapshot = current_manifest(&storage).database_path;

    storage.set_clock_ms(start_ms + step_ms);
    desktop.remove_song("b");
    desktop.publish(&storage);

    storage.set_clock_ms(start_ms + 2 * step_ms);
    desktop.add_song("c");
    desktop.publish(&storage);
    ThreeGenerations {
        storage,
        song_b,
        first_snapshot,
        last_publish_ms: start_ms + 2 * step_ms,
    }
}

#[test]
fn superseded_generations_and_interrupted_uploads_are_deleted_after_the_grace_period() {
    let start = base_clock_ms();
    let repository = three_generations(MemoryStorage::new(), start, HOUR_MS);
    let storage = &repository.storage;

    // An interrupted publication left an asset and a snapshot behind.
    storage.set_clock_ms(repository.last_publish_ms + HOUR_MS);
    let orphan_asset = format!("media/content/{}.flac", "e".repeat(64));
    let orphan_snapshot = database_path_for_operation(4, "op-interrupted");
    storage
        .write_new(&orphan_asset, b"orphan audio")
        .expect("orphan asset");
    storage
        .write_new(&orphan_snapshot, b"torn snapshot")
        .expect("orphan snapshot");

    let manifest_before = current_manifest(storage);
    let report = clean_up(
        storage,
        repository.last_publish_ms + HOUR_MS + GRACE_MS + SECOND_MS,
    )
    .expect("cleanup");

    assert_eq!(report.kept_generations, [2, 3]);
    assert_eq!(
        report.deleted,
        sorted(&[
            &repository.first_snapshot,
            &orphan_snapshot,
            &orphan_asset,
            &repository.song_b,
        ])
    );
    assert!(report.deferred.is_empty() && report.undated.is_empty());
    assert_eq!(current_manifest(storage), manifest_before);
    assert_eq!(
        read_like_a_phone(storage).len(),
        2,
        "songs a and c stay playable"
    );

    let again = clean_up(storage, repository.last_publish_ms + 2 * GRACE_MS).expect("cleanup");
    assert!(again.deleted.is_empty(), "a second cleanup finds nothing");
}

#[test]
fn a_generation_superseded_within_the_grace_period_stays_readable_for_a_phone() {
    let start = base_clock_ms() - 30 * 24 * HOUR_MS;
    let storage = MemoryStorage::new();
    let mut desktop = Desktop::new();
    storage.set_clock_ms(start);
    desktop.add_song("a");
    let song_b = desktop.add_song("b");
    desktop.publish(&storage);

    let phone_dir = tempfile::tempdir().expect("phone dir");
    let phone = ReadOnlyStorage::new(storage.clone());
    let phone_copy = WorkingCopyDatabase::new(phone_dir.path().join("openkara.db"));
    refresh_repository(&phone, &phone_copy, None, &AllowAll).expect("phone refreshes");

    // Weeks later the desktop publishes twice in quick succession while the
    // phone still browses generation 1.
    let publish_at = start + 10 * GRACE_MS;
    storage.set_clock_ms(publish_at);
    desktop.remove_song("b");
    desktop.publish(&storage);
    storage.set_clock_ms(publish_at + SECOND_MS);
    desktop.add_song("c");
    desktop.publish(&storage);

    let report = clean_up(&storage, publish_at + 2 * SECOND_MS).expect("cleanup");
    assert_eq!(report.kept_generations, [1, 2, 3]);
    assert!(report.deleted.is_empty(), "deleted {:?}", report.deleted);
    assert_eq!(
        fetch_every_song(&phone, phone_dir.path(), phone_copy.path()),
        sorted(&[&song_b, &format!("media/content/{}.flac", digest_of("a"))])
    );

    let report = clean_up(&storage, publish_at + SECOND_MS + GRACE_MS + SECOND_MS)
        .expect("cleanup after the grace period");
    assert_eq!(report.kept_generations, [2, 3]);
    assert!(report.deleted.contains(&song_b));
}

fn digest_of(hash: &str) -> String {
    hex_lower(&Sha256::digest(format!("audio of {hash}").as_bytes()))
}

#[test]
fn uploads_of_a_publication_still_in_progress_are_left_alone() {
    let start = base_clock_ms();
    let storage = MemoryStorage::new();
    let mut desktop = Desktop::new();
    storage.set_clock_ms(start);
    desktop.add_song("a");
    desktop.publish(&storage);

    // Another device has uploaded a new song's media but not committed yet.
    let now = start + 5 * GRACE_MS;
    storage.set_clock_ms(now - HOUR_MS);
    let mut other_device = Desktop::new();
    other_device.generation = desktop.generation;
    other_device.add_song("a");
    let song_d = other_device.add_song("d");
    ensure_referenced_assets(&storage, other_device.dir.path(), &other_device.database())
        .expect("other device uploads");

    let report = clean_up(&storage, now).expect("cleanup");
    assert_eq!(report.deferred, std::slice::from_ref(&song_d));
    assert!(report.deleted.is_empty());

    other_device.publish(&storage);
    assert_eq!(read_like_a_phone(&storage).len(), 2);
}

#[test]
fn an_object_rewritten_after_the_listing_is_not_deleted() {
    let start = base_clock_ms();
    let storage = MemoryStorage::new();
    let mut desktop = Desktop::new();
    storage.set_clock_ms(start);
    desktop.add_song("a");
    desktop.publish(&storage);
    let orphan = format!("media/content/{}.flac", "f".repeat(64));
    storage.write_new(&orphan, b"orphan").expect("orphan");

    let now = start + 3 * GRACE_MS;
    storage.script(Fault::StaleListing);
    storage.set_clock_ms(now - SECOND_MS);
    storage.delete(&orphan).expect("delete");
    storage
        .write_new(&orphan, b"re-uploaded")
        .expect("re-upload");

    let report = clean_up(&storage, now).expect("cleanup");
    assert_eq!(report.deferred, std::slice::from_ref(&orphan));
    assert!(report.deleted.is_empty());
    assert!(storage.stat(&orphan).expect("stat").is_some());
}

#[test]
fn nothing_is_deleted_when_the_manifest_moves_during_cleanup() {
    let start = base_clock_ms();
    let repository = three_generations(MemoryStorage::new(), start, HOUR_MS);
    let storage = &repository.storage;
    let mut manifest_bytes = Vec::new();
    storage
        .read(MANIFEST_PATH, &mut manifest_bytes)
        .expect("read manifest");
    storage.script(Fault::MoveRevision {
        path: MANIFEST_PATH.to_owned(),
        bytes: manifest_bytes,
    });

    let before = listed_paths(storage);
    let report = clean_up(storage, repository.last_publish_ms + 2 * GRACE_MS).expect("cleanup");
    assert!(report.deleted.is_empty());
    assert_eq!(
        report.deferred,
        sorted(&[&repository.first_snapshot, &repository.song_b])
    );
    assert_eq!(listed_paths(storage), before);
}

#[test]
fn a_kept_snapshot_that_cannot_be_read_deletes_nothing() {
    let start = base_clock_ms();
    let repository = three_generations(MemoryStorage::new(), start, HOUR_MS);
    let storage = &repository.storage;
    let now = repository.last_publish_ms + 2 * GRACE_MS;
    let before = listed_paths(storage);

    let current = current_manifest(storage).database_path;
    storage.script(Fault::InterruptTransfer {
        path: current,
        after_bytes: 10,
    });
    let interrupted = clean_up(storage, now).expect_err("interrupted download");
    assert_eq!(interrupted.kind, RemoteErrorKind::NetworkUnavailable);
    assert_eq!(listed_paths(storage), before);

    let previous = storage
        .list(".openkara/databases/2")
        .expect("list generation 2")
        .remove(0)
        .path;
    storage.delete(&previous).expect("delete");
    storage
        .write_new(&previous, b"not a database")
        .expect("corrupt");
    let corrupt = clean_up(storage, now).expect_err("corrupt kept snapshot");
    assert_eq!(corrupt.kind, RemoteErrorKind::RemoteIntegrityFailed);
    assert_eq!(listed_paths(storage), before);
}

#[test]
fn a_repository_without_a_manifest_is_never_cleaned() {
    let storage = MemoryStorage::new();
    storage
        .write_new("openkara.db", b"legacy database")
        .expect("legacy db");
    storage
        .write_new("media/old.flac", b"legacy media")
        .expect("legacy media");

    let report = clean_up(&storage, base_clock_ms() + 10 * GRACE_MS).expect("cleanup");
    assert_eq!(report, CleanupReport::default());
    assert_eq!(listed_paths(&storage), ["media/old.flac", "openkara.db"]);
}

#[test]
fn objects_outside_the_managed_directories_are_left_alone() {
    let start = base_clock_ms();
    let storage = MemoryStorage::new();
    let mut desktop = Desktop::new();
    storage.set_clock_ms(start);
    desktop.add_song("a");
    desktop.publish(&storage);
    let foreign = [
        "notes.txt",
        "openkara.db",
        ".openkara/staging/op-1/media.flac",
        ".openkara/databases/readme.txt",
        ".openkara/databases/01.sqlite",
        ".openkara/databases/2/nested/x.sqlite",
        "media",
    ];
    for path in foreign {
        storage.write_new(path, b"foreign").expect("seed");
    }

    let before = listed_paths(&storage);
    let report = clean_up(&storage, start + 10 * GRACE_MS).expect("cleanup");
    assert!(report.deleted.is_empty(), "deleted {:?}", report.deleted);
    assert_eq!(listed_paths(&storage), before);
}

/// A [`MemoryStorage`] whose listings can omit modification times, like a
/// server that does not report `getlastmodified`, and on which another device
/// can commit a generation right after the first delete.
struct ScriptedStorage {
    inner: MemoryStorage,
    undated: bool,
    commit_after_first_delete: Cell<bool>,
}

impl ScriptedStorage {
    fn undated(inner: MemoryStorage) -> Self {
        Self {
            inner,
            undated: true,
            commit_after_first_delete: Cell::new(false),
        }
    }

    fn committing_after_first_delete(inner: MemoryStorage) -> Self {
        Self {
            inner,
            undated: false,
            commit_after_first_delete: Cell::new(true),
        }
    }
}

impl RepositoryStorage for ScriptedStorage {
    fn capabilities(&self) -> StorageCapabilities {
        self.inner.capabilities()
    }
    fn stat(&self, path: &str) -> RemoteResult<Option<ObjectMetadata>> {
        self.inner.stat(path)
    }
    fn read(&self, path: &str, sink: &mut dyn std::io::Write) -> RemoteResult<u64> {
        self.inner.read(path, sink)
    }
    fn read_range(
        &self,
        path: &str,
        offset: u64,
        length: u64,
        sink: &mut dyn std::io::Write,
    ) -> RemoteResult<u64> {
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
        Ok(self
            .inner
            .list(prefix)?
            .into_iter()
            .map(|object| ListedObject {
                modified_at_ms: object.modified_at_ms.filter(|_| !self.undated),
                ..object
            })
            .collect())
    }
    fn delete(&self, path: &str) -> RemoteResult<()> {
        self.inner.delete(path)?;
        if self.commit_after_first_delete.replace(false) {
            let mut manifest = Vec::new();
            self.inner.read(MANIFEST_PATH, &mut manifest)?;
            let revision = self
                .inner
                .stat(MANIFEST_PATH)?
                .and_then(|metadata| metadata.revision)
                .expect("manifest revision");
            self.inner
                .conditional_replace(MANIFEST_PATH, &manifest, &revision)?;
        }
        Ok(())
    }
}

#[test]
fn cleanup_stops_deleting_once_another_device_commits() {
    let start = base_clock_ms();
    let repository = three_generations(MemoryStorage::new(), start, HOUR_MS);
    let orphan = format!("media/content/{}.flac", "e".repeat(64));
    repository
        .storage
        .write_new(&orphan, b"orphan audio")
        .expect("orphan asset");
    let storage = ScriptedStorage::committing_after_first_delete(repository.storage.clone());

    let report = clean_up(&storage, repository.last_publish_ms + 2 * GRACE_MS).expect("cleanup");
    assert_eq!(
        report.deleted,
        std::slice::from_ref(&repository.first_snapshot)
    );
    assert_eq!(report.deferred, sorted(&[&orphan, &repository.song_b]));
    assert!(storage.stat(&orphan).expect("stat").is_some());
    assert!(storage.stat(&repository.song_b).expect("stat").is_some());
}

#[test]
fn objects_without_a_modification_time_are_never_deleted() {
    let start = base_clock_ms();
    let repository = three_generations(MemoryStorage::new(), start, HOUR_MS);
    let orphan = format!("media/content/{}.flac", "e".repeat(64));
    repository
        .storage
        .write_new(&orphan, b"orphan audio")
        .expect("orphan asset");
    let undated = ScriptedStorage::undated(repository.storage.clone());
    let before = listed_paths(&undated);

    let report = clean_up(&undated, repository.last_publish_ms + 2 * GRACE_MS).expect("cleanup");
    assert_eq!(
        report.kept_generations,
        [1, 2, 3],
        "undated snapshots cannot bound when a generation was superseded"
    );
    assert_eq!(report.undated, [orphan]);
    assert!(report.deleted.is_empty());
    assert_eq!(listed_paths(&undated), before);
}

#[test]
fn a_phone_reads_and_plays_through_a_read_only_storage() {
    let repository = three_generations(MemoryStorage::new(), base_clock_ms(), HOUR_MS);
    let before = listed_paths(&repository.storage);
    let phone = ReadOnlyStorage::new(repository.storage.clone());
    assert_eq!(read_like_a_phone(&phone).len(), 2);
    assert_eq!(listed_paths(&repository.storage), before);
}

#[test]
fn a_phone_never_deletes_remote_objects() {
    let repository = three_generations(MemoryStorage::new(), base_clock_ms(), HOUR_MS);
    let before = listed_paths(&repository.storage);
    let phone = ReadOnlyStorage::new(repository.storage.clone());

    let refused = clean_up(&phone, repository.last_publish_ms + 10 * GRACE_MS)
        .expect_err("a phone cannot clean up");
    assert_eq!(refused.kind, RemoteErrorKind::ProviderCapabilityUnavailable);
    let mut desktop = Desktop::new();
    desktop.generation = 3;
    desktop.add_song("z");
    let refused = publish_changes(
        &phone,
        desktop.dir.path(),
        &desktop.database(),
        &PublishRequest {
            repository_id: "repo-1".to_owned(),
            writer_id: "phone".to_owned(),
            operation_id: "op-phone".to_owned(),
            expected_generation: 3,
            changes: ChangeSet::default(),
            conflict_policy: ConflictPolicy::Stop,
        },
        &mut |_| {},
    )
    .expect_err("a phone cannot publish");
    assert_eq!(refused.kind, RemoteErrorKind::ProviderCapabilityUnavailable);
    for path in &before {
        let refused = phone.delete(path).expect_err("a phone cannot delete");
        assert_eq!(refused.kind, RemoteErrorKind::PermissionDenied);
    }
    assert!(phone.write_new("media/new.flac", b"new").is_err());

    assert_eq!(listed_paths(&repository.storage), before);
}

#[test]
fn cleanup_over_webdav_deletes_superseded_uploads() {
    let server = TestWebDavServer::start();
    let storage = server.storage();
    let start = base_clock_ms();
    let mut desktop = Desktop::new();
    server.set_clock_ms(start);
    desktop.add_song("a");
    let song_b = desktop.add_song("b");
    desktop.publish(storage);
    let first_snapshot = current_manifest(storage).database_path;
    server.set_clock_ms(start + HOUR_MS);
    desktop.remove_song("b");
    desktop.publish(storage);
    server.set_clock_ms(start + 2 * HOUR_MS);
    desktop.add_song("c");
    desktop.publish(storage);
    let in_flight = format!("media/content/{}.flac", "d".repeat(64));
    let now = start + 2 * HOUR_MS + GRACE_MS + SECOND_MS;
    server.set_clock_ms(now - HOUR_MS);
    storage
        .write_new(&in_flight, b"in-flight upload")
        .expect("in-flight upload");

    let report = clean_up(storage, now).expect("cleanup over WebDAV");
    assert_eq!(report.kept_generations, [2, 3]);
    assert_eq!(report.deleted, sorted(&[&first_snapshot, &song_b]));
    assert_eq!(report.deferred, [in_flight]);
    assert!(server.file(&song_b).is_none());
    assert_eq!(read_like_a_phone(storage).len(), 2);
}

#[test]
fn snapshot_paths_name_their_generation() {
    assert_eq!(snapshot_generation(".openkara/databases/7.sqlite"), Some(7));
    assert_eq!(
        snapshot_generation(&database_path_for_operation(12, "op")),
        Some(12)
    );
    for path in [
        ".openkara/databases/0.sqlite",
        ".openkara/databases/-1.sqlite",
        ".openkara/databases/07.sqlite",
        ".openkara/databases/7.sqlite.part",
        ".openkara/databases/7/a/b.sqlite",
        ".openkara/databases/x/a.sqlite",
        ".openkara/staging/7.sqlite",
    ] {
        assert_eq!(snapshot_generation(path), None, "{path}");
    }
}
