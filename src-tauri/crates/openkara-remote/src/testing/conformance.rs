//! Behavior every [`RepositoryStorage`] adapter must show. Run it for an
//! adapter with [`storage_conformance_suite!`](crate::storage_conformance_suite).

use std::fmt::Debug;

use super::Fault;
use crate::error::{RemoteErrorKind, RemoteResult};
use crate::manifest::MANIFEST_PATH;
use crate::storage::{ListedObject, ObjectMetadata, RepositoryStorage};

/// A fresh, empty repository behind the adapter under test, with a way to
/// script [`Fault`]s into it.
pub trait ConformanceFixture {
    fn storage(&self) -> &dyn RepositoryStorage;
    fn script(&self, fault: Fault);
}

/// Expands to one `#[test]` per conformance case. `$fixture` is evaluated
/// once per test and must produce a value that implements
/// [`ConformanceFixture`].
#[macro_export]
macro_rules! storage_conformance_suite {
    ($fixture:expr) => {
        $crate::storage_conformance_suite!(@cases $fixture;
            stat_reports_a_missing_object_as_absent,
            write_new_stores_bytes_that_stat_and_read_report,
            write_new_stores_empty_objects,
            write_new_never_replaces_an_existing_object,
            read_of_a_missing_object_fails_as_not_found,
            read_range_returns_the_requested_window,
            read_range_clamps_a_window_past_the_end,
            read_range_rejects_a_window_starting_at_or_past_the_end,
            read_range_with_zero_length_transfers_nothing,
            conditional_replace_succeeds_while_the_revision_matches,
            conditional_replace_fails_after_another_writer_moved_the_revision,
            conditional_replace_fails_for_a_missing_object,
            list_returns_every_object_under_a_directory_recursively,
            list_of_the_repository_root_returns_every_object,
            list_of_an_absent_directory_is_empty,
            delete_removes_the_object_from_stat_read_and_list,
            delete_of_an_absent_object_succeeds,
            paths_outside_the_repository_root_are_rejected,
            a_scripted_precondition_failure_rejects_one_conditional_write,
            an_interrupted_read_keeps_the_received_prefix_for_resumption,
            an_interrupted_range_read_resumes_from_the_received_offset,
            a_partial_write_is_detectable_and_recoverable,
            a_partial_conditional_replace_moves_the_revision,
            a_stale_listing_misses_recent_changes_once,
            a_revision_moving_between_stat_and_replace_rejects_the_replace,
            a_revision_moving_after_a_read_is_visible_to_the_next_read,
        );
    };
    (@cases $fixture:expr; $($case:ident),+ $(,)?) => {
        $(
            #[test]
            fn $case() {
                $crate::testing::conformance::$case(&$fixture);
            }
        )+
    };
}

const ASSET: &str = "media/ab/0123456789abcdef.flac";

fn sample(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index * 31 % 251) as u8).collect()
}

fn read_all(storage: &dyn RepositoryStorage, path: &str) -> RemoteResult<Vec<u8>> {
    let mut received = Vec::new();
    storage.read(path, &mut received)?;
    Ok(received)
}

fn read_window(
    storage: &dyn RepositoryStorage,
    path: &str,
    offset: u64,
    length: u64,
) -> RemoteResult<Vec<u8>> {
    let mut received = Vec::new();
    let count = storage.read_range(path, offset, length, &mut received)?;
    assert_eq!(count, received.len() as u64, "read_range miscounted {path}");
    Ok(received)
}

fn seed(storage: &dyn RepositoryStorage, path: &str, bytes: &[u8]) -> ObjectMetadata {
    storage
        .write_new(path, bytes)
        .unwrap_or_else(|error| panic!("seeding {path} failed: {error}"))
}

fn revision(metadata: &ObjectMetadata) -> &str {
    metadata
        .revision
        .as_deref()
        .expect("a storage with conditional replace reports revisions")
}

fn expect_kind<T: Debug>(result: RemoteResult<T>, kind: RemoteErrorKind) {
    match result {
        Err(error) => assert_eq!(error.kind, kind, "unexpected error {error}"),
        Ok(value) => panic!("expected {}, got Ok({value:?})", kind.code()),
    }
}

fn assert_stored(storage: &dyn RepositoryStorage, path: &str, bytes: &[u8]) {
    assert_eq!(read_all(storage, path).expect("read stored object"), bytes);
    let metadata = storage.stat(path).expect("stat").expect("object exists");
    assert_eq!(metadata.size_bytes, bytes.len() as u64);
}

fn listed_paths(listing: &[ListedObject]) -> Vec<&str> {
    listing.iter().map(|object| object.path.as_str()).collect()
}

/// Returns false after it checks that a storage without range reads refuses them.
fn supports_range_read(storage: &dyn RepositoryStorage) -> bool {
    if storage.capabilities().range_read {
        return true;
    }
    let path = "capability-probe/range.bin";
    seed(storage, path, b"range");
    expect_kind(
        read_window(storage, path, 0, 1),
        RemoteErrorKind::ProviderCapabilityUnavailable,
    );
    false
}

/// Returns false after it checks that a storage without conditional replace
/// refuses it, leaves the object unchanged, and cannot publish.
fn supports_conditional_replace(storage: &dyn RepositoryStorage) -> bool {
    let capabilities = storage.capabilities();
    assert_eq!(capabilities.can_publish(), capabilities.conditional_replace);
    if capabilities.conditional_replace {
        return true;
    }
    let path = "capability-probe/replace.json";
    let original = seed(storage, path, b"original");
    let expected = original.revision.clone().unwrap_or_default();
    expect_kind(
        storage.conditional_replace(path, b"replacement", &expected),
        RemoteErrorKind::ProviderCapabilityUnavailable,
    );
    assert_stored(storage, path, b"original");
    false
}

pub fn stat_reports_a_missing_object_as_absent(fixture: &dyn ConformanceFixture) {
    assert_eq!(fixture.storage().stat(ASSET).expect("stat"), None);
}

pub fn write_new_stores_bytes_that_stat_and_read_report(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    let bytes = sample(1000);
    let written = seed(storage, ASSET, &bytes);
    assert_eq!(written.size_bytes, 1000);
    if storage.capabilities().conditional_replace {
        assert!(written.revision.is_some());
    }
    assert_eq!(storage.stat(ASSET).expect("stat"), Some(written));
    assert_eq!(read_all(storage, ASSET).expect("read"), bytes);
}

pub fn write_new_stores_empty_objects(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    let written = seed(storage, "artwork/empty.bin", &[]);
    assert_eq!(written.size_bytes, 0);
    assert_stored(storage, "artwork/empty.bin", &[]);
}

pub fn write_new_never_replaces_an_existing_object(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    let original = seed(storage, ASSET, b"original");
    expect_kind(
        storage.write_new(ASSET, b"other device"),
        RemoteErrorKind::RemoteConflict,
    );
    assert_stored(storage, ASSET, b"original");
    assert_eq!(storage.stat(ASSET).expect("stat"), Some(original));
}

pub fn read_of_a_missing_object_fails_as_not_found(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    expect_kind(read_all(storage, ASSET), RemoteErrorKind::ObjectNotFound);
    if storage.capabilities().range_read {
        expect_kind(
            read_window(storage, ASSET, 0, 8),
            RemoteErrorKind::ObjectNotFound,
        );
    }
}

pub fn read_range_returns_the_requested_window(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    if !supports_range_read(storage) {
        return;
    }
    let bytes = sample(100);
    seed(storage, ASSET, &bytes);
    for (offset, length) in [(0_usize, 10_usize), (37, 20), (90, 10), (0, 100)] {
        assert_eq!(
            read_window(storage, ASSET, offset as u64, length as u64).expect("range read"),
            bytes[offset..offset + length],
            "window {offset}+{length}"
        );
    }
}

pub fn read_range_clamps_a_window_past_the_end(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    if !supports_range_read(storage) {
        return;
    }
    let bytes = sample(100);
    seed(storage, ASSET, &bytes);
    assert_eq!(
        read_window(storage, ASSET, 90, 50).expect("range read"),
        bytes[90..]
    );
}

pub fn read_range_rejects_a_window_starting_at_or_past_the_end(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    if !supports_range_read(storage) {
        return;
    }
    seed(storage, ASSET, &sample(100));
    for offset in [100, 150] {
        let mut received = Vec::new();
        expect_kind(
            storage.read_range(ASSET, offset, 1, &mut received),
            RemoteErrorKind::RemoteIntegrityFailed,
        );
        assert!(received.is_empty());
    }
}

pub fn read_range_with_zero_length_transfers_nothing(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    if !supports_range_read(storage) {
        return;
    }
    seed(storage, ASSET, &sample(100));
    assert!(read_window(storage, ASSET, 10, 0)
        .expect("empty range read")
        .is_empty());
}

pub fn conditional_replace_succeeds_while_the_revision_matches(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    if !supports_conditional_replace(storage) {
        return;
    }
    let first = seed(storage, MANIFEST_PATH, b"generation 1");
    let replaced = storage
        .conditional_replace(MANIFEST_PATH, b"generation 2!", revision(&first))
        .expect("conditional replace");
    assert_eq!(replaced.size_bytes, 13);
    assert_ne!(replaced.revision, first.revision);
    assert_eq!(
        storage.stat(MANIFEST_PATH).expect("stat"),
        Some(replaced.clone())
    );
    assert_stored(storage, MANIFEST_PATH, b"generation 2!");

    storage
        .conditional_replace(MANIFEST_PATH, b"generation 3", revision(&replaced))
        .expect("replace again with the new revision");
    assert_stored(storage, MANIFEST_PATH, b"generation 3");
}

pub fn conditional_replace_fails_after_another_writer_moved_the_revision(
    fixture: &dyn ConformanceFixture,
) {
    let storage = fixture.storage();
    if !supports_conditional_replace(storage) {
        return;
    }
    let base = seed(storage, MANIFEST_PATH, b"generation 1");
    let other_device = storage
        .conditional_replace(MANIFEST_PATH, b"other device", revision(&base))
        .expect("other device publishes");
    expect_kind(
        storage.conditional_replace(MANIFEST_PATH, b"this device", revision(&base)),
        RemoteErrorKind::RemoteConflict,
    );
    assert_stored(storage, MANIFEST_PATH, b"other device");
    assert_eq!(
        storage.stat(MANIFEST_PATH).expect("stat"),
        Some(other_device)
    );
}

pub fn conditional_replace_fails_for_a_missing_object(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    if !supports_conditional_replace(storage) {
        return;
    }
    expect_kind(
        storage.conditional_replace(MANIFEST_PATH, b"generation 1", "any-revision"),
        RemoteErrorKind::RemoteConflict,
    );
    assert_eq!(storage.stat(MANIFEST_PATH).expect("stat"), None);
}

pub fn list_returns_every_object_under_a_directory_recursively(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    for path in [
        "media/cd/two.flac",
        "media/ab/one.flac",
        "media-g/ef/three.flac",
        "stems/ab/vocals.ogg",
        MANIFEST_PATH,
    ] {
        seed(storage, path, path.as_bytes());
    }

    let media = storage.list("media").expect("list media");
    assert_eq!(
        listed_paths(&media),
        ["media/ab/one.flac", "media/cd/two.flac"]
    );
    for object in &media {
        assert_eq!(
            Some(object.metadata.clone()),
            storage.stat(&object.path).expect("stat")
        );
    }
    assert_eq!(
        listed_paths(&storage.list("media-g").expect("list media-g")),
        ["media-g/ef/three.flac"]
    );
    assert_eq!(
        listed_paths(&storage.list("media/ab").expect("list media/ab")),
        ["media/ab/one.flac"]
    );
}

pub fn list_of_the_repository_root_returns_every_object(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    for path in [MANIFEST_PATH, "stems/ab/vocals.ogg", "media/ab/one.flac"] {
        seed(storage, path, path.as_bytes());
    }
    assert_eq!(
        listed_paths(&storage.list("").expect("list root")),
        [MANIFEST_PATH, "media/ab/one.flac", "stems/ab/vocals.ogg"]
    );
}

pub fn list_of_an_absent_directory_is_empty(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    seed(storage, ASSET, b"asset");
    assert!(storage.list("artwork").expect("list artwork").is_empty());
}

pub fn delete_removes_the_object_from_stat_read_and_list(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    seed(storage, ASSET, b"asset");
    storage.delete(ASSET).expect("delete");
    assert_eq!(storage.stat(ASSET).expect("stat"), None);
    expect_kind(read_all(storage, ASSET), RemoteErrorKind::ObjectNotFound);
    assert!(storage.list("media").expect("list").is_empty());

    seed(storage, ASSET, b"uploaded again");
    assert_stored(storage, ASSET, b"uploaded again");
}

pub fn delete_of_an_absent_object_succeeds(fixture: &dyn ConformanceFixture) {
    fixture
        .storage()
        .delete(ASSET)
        .expect("delete of an absent object");
}

pub fn paths_outside_the_repository_root_are_rejected(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    for path in ["", "../escape.json", "/absolute.json", "media//double.flac"] {
        expect_kind(storage.stat(path), RemoteErrorKind::PermissionDenied);
        expect_kind(read_all(storage, path), RemoteErrorKind::PermissionDenied);
        expect_kind(
            storage.write_new(path, b"bytes"),
            RemoteErrorKind::PermissionDenied,
        );
        expect_kind(storage.delete(path), RemoteErrorKind::PermissionDenied);
    }
    expect_kind(storage.list("../"), RemoteErrorKind::PermissionDenied);
}

pub fn a_scripted_precondition_failure_rejects_one_conditional_write(
    fixture: &dyn ConformanceFixture,
) {
    let storage = fixture.storage();
    fixture.script(Fault::FailPrecondition { path: ASSET.into() });
    expect_kind(
        storage.write_new(ASSET, b"asset"),
        RemoteErrorKind::RemoteConflict,
    );
    assert_eq!(storage.stat(ASSET).expect("stat"), None);
    seed(storage, ASSET, b"asset");

    if !supports_conditional_replace(storage) {
        return;
    }
    let base = seed(storage, MANIFEST_PATH, b"generation 1");
    fixture.script(Fault::FailPrecondition {
        path: MANIFEST_PATH.into(),
    });
    expect_kind(
        storage.conditional_replace(MANIFEST_PATH, b"generation 2", revision(&base)),
        RemoteErrorKind::RemoteConflict,
    );
    assert_stored(storage, MANIFEST_PATH, b"generation 1");
    storage
        .conditional_replace(MANIFEST_PATH, b"generation 2", revision(&base))
        .expect("the retry passes once the scripted failure fired");
    assert_stored(storage, MANIFEST_PATH, b"generation 2");
}

pub fn an_interrupted_read_keeps_the_received_prefix_for_resumption(
    fixture: &dyn ConformanceFixture,
) {
    let storage = fixture.storage();
    let bytes = sample(64);
    seed(storage, ASSET, &bytes);
    fixture.script(Fault::InterruptTransfer {
        path: ASSET.into(),
        after_bytes: 24,
    });

    let mut received = Vec::new();
    let error = storage
        .read(ASSET, &mut received)
        .expect_err("the transfer is interrupted");
    assert_eq!(error.kind, RemoteErrorKind::NetworkUnavailable);
    assert!(error.retryable);
    assert!(received.len() <= 24);
    assert!(bytes.starts_with(&received));

    if storage.capabilities().range_read {
        let offset = received.len() as u64;
        storage
            .read_range(ASSET, offset, 64 - offset, &mut received)
            .expect("resume with a range read");
        assert_eq!(received, bytes);
    } else {
        assert_eq!(read_all(storage, ASSET).expect("read again"), bytes);
    }
}

pub fn an_interrupted_range_read_resumes_from_the_received_offset(
    fixture: &dyn ConformanceFixture,
) {
    let storage = fixture.storage();
    if !supports_range_read(storage) {
        return;
    }
    let bytes = sample(64);
    seed(storage, ASSET, &bytes);
    fixture.script(Fault::InterruptTransfer {
        path: ASSET.into(),
        after_bytes: 8,
    });

    let mut received = Vec::new();
    let error = storage
        .read_range(ASSET, 16, 32, &mut received)
        .expect_err("the transfer is interrupted");
    assert!(error.retryable);
    assert!(received.len() <= 8);
    assert!(bytes[16..48].starts_with(&received));

    let resume_at = 16 + received.len() as u64;
    storage
        .read_range(ASSET, resume_at, 48 - resume_at, &mut received)
        .expect("resume");
    assert_eq!(received, bytes[16..48]);
}

pub fn a_partial_write_is_detectable_and_recoverable(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    let bytes = sample(64);
    fixture.script(Fault::PartialWrite {
        path: ASSET.into(),
        stored_bytes: 20,
    });

    let error = storage
        .write_new(ASSET, &bytes)
        .expect_err("the write is cut short");
    assert!(error.retryable);
    let partial = storage.stat(ASSET).expect("stat").expect("a prefix landed");
    assert_eq!(partial.size_bytes, 20);
    assert_eq!(read_all(storage, ASSET).expect("read"), bytes[..20]);

    expect_kind(
        storage.write_new(ASSET, &bytes),
        RemoteErrorKind::RemoteConflict,
    );
    storage.delete(ASSET).expect("delete the partial object");
    seed(storage, ASSET, &bytes);
    assert_stored(storage, ASSET, &bytes);
}

pub fn a_partial_conditional_replace_moves_the_revision(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    if !supports_conditional_replace(storage) {
        return;
    }
    let base = seed(storage, MANIFEST_PATH, b"generation 1");
    fixture.script(Fault::PartialWrite {
        path: MANIFEST_PATH.into(),
        stored_bytes: 5,
    });

    storage
        .conditional_replace(MANIFEST_PATH, b"generation 2", revision(&base))
        .expect_err("the replace is cut short");
    let partial = storage
        .stat(MANIFEST_PATH)
        .expect("stat")
        .expect("manifest exists");
    assert_eq!(partial.size_bytes, 5);
    assert_ne!(partial.revision, base.revision);
    expect_kind(
        storage.conditional_replace(MANIFEST_PATH, b"generation 2", revision(&base)),
        RemoteErrorKind::RemoteConflict,
    );
    storage
        .conditional_replace(MANIFEST_PATH, b"generation 2", revision(&partial))
        .expect("replace against the observed revision");
    assert_stored(storage, MANIFEST_PATH, b"generation 2");
}

pub fn a_stale_listing_misses_recent_changes_once(fixture: &dyn ConformanceFixture) {
    let storage = fixture.storage();
    seed(storage, "media/ab/old.flac", b"old");
    fixture.script(Fault::StaleListing);
    seed(storage, "media/cd/new.flac", b"new");
    storage.delete("media/ab/old.flac").expect("delete");

    assert_eq!(
        listed_paths(&storage.list("media").expect("stale list")),
        ["media/ab/old.flac"]
    );
    assert_eq!(
        listed_paths(&storage.list("media").expect("fresh list")),
        ["media/cd/new.flac"]
    );
}

pub fn a_revision_moving_between_stat_and_replace_rejects_the_replace(
    fixture: &dyn ConformanceFixture,
) {
    let storage = fixture.storage();
    if !supports_conditional_replace(storage) {
        return;
    }
    let base = seed(storage, MANIFEST_PATH, b"generation 1");
    fixture.script(Fault::MoveRevision {
        path: MANIFEST_PATH.into(),
        bytes: b"other device".to_vec(),
    });

    let observed = storage
        .stat(MANIFEST_PATH)
        .expect("stat")
        .expect("manifest exists");
    assert_eq!(observed, base);
    expect_kind(
        storage.conditional_replace(MANIFEST_PATH, b"this device", revision(&observed)),
        RemoteErrorKind::RemoteConflict,
    );
    assert_stored(storage, MANIFEST_PATH, b"other device");
}

pub fn a_revision_moving_after_a_read_is_visible_to_the_next_read(
    fixture: &dyn ConformanceFixture,
) {
    let storage = fixture.storage();
    let base = seed(storage, MANIFEST_PATH, b"generation 1");
    fixture.script(Fault::MoveRevision {
        path: MANIFEST_PATH.into(),
        bytes: b"generation 2 from another device".to_vec(),
    });

    assert_eq!(
        read_all(storage, MANIFEST_PATH).expect("first read"),
        b"generation 1"
    );
    assert_stored(storage, MANIFEST_PATH, b"generation 2 from another device");
    if storage.capabilities().conditional_replace {
        let moved = storage.stat(MANIFEST_PATH).expect("stat").expect("exists");
        assert_ne!(moved.revision, base.revision);
    }
}
