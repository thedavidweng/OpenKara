use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::{ConformanceFixture, Fault};
use crate::error::{kind_from_io_error, RemoteError, RemoteErrorKind, RemoteResult};
use crate::storage::{
    is_under_prefix, validate_object_path, validate_prefix, ListedObject, ObjectMetadata,
    RepositoryStorage, StorageCapabilities,
};

/// A Remote Repository held in memory, with scriptable [`Fault`]s. Clones
/// share the same repository, so a clone stands in for another device.
#[derive(Clone)]
pub struct MemoryStorage {
    state: Arc<Mutex<State>>,
    capabilities: StorageCapabilities,
}

#[derive(Default)]
struct State {
    objects: BTreeMap<String, StoredObject>,
    revisions_issued: u64,
    faults: Vec<Fault>,
    stale_listings: VecDeque<BTreeMap<String, StoredObject>>,
}

#[derive(Clone)]
struct StoredObject {
    bytes: Vec<u8>,
    revision: String,
}

impl StoredObject {
    fn metadata(&self) -> ObjectMetadata {
        ObjectMetadata {
            size_bytes: self.bytes.len() as u64,
            revision: Some(self.revision.clone()),
        }
    }
}

enum Precondition<'a> {
    Absent,
    Revision(&'a str),
}

impl State {
    fn put(&mut self, path: &str, bytes: Vec<u8>) -> ObjectMetadata {
        self.revisions_issued += 1;
        let object = StoredObject {
            bytes,
            revision: format!("memory-revision-{}", self.revisions_issued),
        };
        let metadata = object.metadata();
        self.objects.insert(path.to_owned(), object);
        metadata
    }

    fn take_fault(&mut self, matches: impl Fn(&Fault) -> bool) -> Option<Fault> {
        let index = self.faults.iter().position(matches)?;
        Some(self.faults.remove(index))
    }

    fn move_revision_after_access(&mut self, path: &str) {
        let moved = self.take_fault(
            |fault| matches!(fault, Fault::MoveRevision { path: target, .. } if target == path),
        );
        if let Some(Fault::MoveRevision { bytes, .. }) = moved {
            self.put(path, bytes);
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::with_capabilities(StorageCapabilities {
            conditional_replace: true,
            range_read: true,
        })
    }

    pub fn with_capabilities(capabilities: StorageCapabilities) -> Self {
        Self {
            state: Arc::default(),
            capabilities,
        }
    }

    pub fn script(&self, fault: Fault) {
        let mut state = self.state();
        if fault == Fault::StaleListing {
            let snapshot = state.objects.clone();
            state.stale_listings.push_back(snapshot);
        } else {
            state.faults.push(fault);
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn transfer(
        &self,
        path: &str,
        window: Option<(u64, u64)>,
        sink: &mut dyn Write,
    ) -> RemoteResult<u64> {
        let (bytes, interrupt_after) = {
            let mut state = self.state();
            let object = state.objects.get(path).ok_or_else(|| not_found(path))?;
            let bytes = match window {
                None => object.bytes.clone(),
                Some((offset, length)) => range_of(&object.bytes, offset, length)?.to_vec(),
            };
            let interrupt = state.take_fault(|fault| {
                matches!(fault, Fault::InterruptTransfer { path: target, .. } if target == path)
            });
            let interrupt_after = match interrupt {
                Some(Fault::InterruptTransfer { after_bytes, .. }) => Some(after_bytes),
                _ => None,
            };
            (bytes, interrupt_after)
        };

        if let Some(after_bytes) = interrupt_after {
            let delivered =
                usize::try_from(after_bytes).map_or(bytes.len(), |n| n.min(bytes.len()));
            sink.write_all(&bytes[..delivered]).map_err(sink_error)?;
            return Err(RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("transfer of {path} was interrupted"),
            ));
        }
        sink.write_all(&bytes).map_err(sink_error)?;
        self.state().move_revision_after_access(path);
        Ok(bytes.len() as u64)
    }

    fn store(
        &self,
        path: &str,
        bytes: &[u8],
        precondition: Precondition<'_>,
    ) -> RemoteResult<ObjectMetadata> {
        let mut state = self.state();
        let scripted_failure = state.take_fault(
            |fault| matches!(fault, Fault::FailPrecondition { path: target } if target == path),
        );
        let precondition_holds = scripted_failure.is_none()
            && match (precondition, state.objects.get(path)) {
                (Precondition::Absent, existing) => existing.is_none(),
                (Precondition::Revision(expected), Some(existing)) => existing.revision == expected,
                (Precondition::Revision(_), None) => false,
            };
        if !precondition_holds {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteConflict,
                format!("precondition failed for {path}"),
            ));
        }

        let partial = state.take_fault(
            |fault| matches!(fault, Fault::PartialWrite { path: target, .. } if target == path),
        );
        if let Some(Fault::PartialWrite { stored_bytes, .. }) = partial {
            let stored = usize::try_from(stored_bytes).map_or(bytes.len(), |n| n.min(bytes.len()));
            state.put(path, bytes[..stored].to_vec());
            return Err(RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("write of {path} was interrupted"),
            ));
        }
        Ok(state.put(path, bytes.to_vec()))
    }
}

fn range_of(bytes: &[u8], offset: u64, length: u64) -> RemoteResult<&[u8]> {
    let size = bytes.len() as u64;
    if offset >= size {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("range starts at {offset}, past the end of a {size}-byte object"),
        ));
    }
    let end = offset.saturating_add(length).min(size);
    Ok(&bytes[offset as usize..end as usize])
}

fn not_found(path: &str) -> RemoteError {
    RemoteError::new(
        RemoteErrorKind::ObjectNotFound,
        format!("no object at {path}"),
    )
}

fn sink_error(error: std::io::Error) -> RemoteError {
    RemoteError::new(kind_from_io_error(&error), error.to_string())
}

impl RepositoryStorage for MemoryStorage {
    fn capabilities(&self) -> StorageCapabilities {
        self.capabilities
    }

    fn stat(&self, path: &str) -> RemoteResult<Option<ObjectMetadata>> {
        validate_object_path(path)?;
        let mut state = self.state();
        let metadata = state.objects.get(path).map(StoredObject::metadata);
        state.move_revision_after_access(path);
        Ok(metadata)
    }

    fn read(&self, path: &str, sink: &mut dyn Write) -> RemoteResult<u64> {
        validate_object_path(path)?;
        self.transfer(path, None, sink)
    }

    fn read_range(
        &self,
        path: &str,
        offset: u64,
        length: u64,
        sink: &mut dyn Write,
    ) -> RemoteResult<u64> {
        validate_object_path(path)?;
        if !self.capabilities.range_read {
            return Err(RemoteError::from_kind(
                RemoteErrorKind::ProviderCapabilityUnavailable,
            ));
        }
        if length == 0 {
            return Ok(0);
        }
        self.transfer(path, Some((offset, length)), sink)
    }

    fn write_new(&self, path: &str, bytes: &[u8]) -> RemoteResult<ObjectMetadata> {
        validate_object_path(path)?;
        self.store(path, bytes, Precondition::Absent)
    }

    fn conditional_replace(
        &self,
        path: &str,
        bytes: &[u8],
        expected_revision: &str,
    ) -> RemoteResult<ObjectMetadata> {
        validate_object_path(path)?;
        if !self.capabilities.conditional_replace {
            return Err(RemoteError::from_kind(
                RemoteErrorKind::ProviderCapabilityUnavailable,
            ));
        }
        self.store(path, bytes, Precondition::Revision(expected_revision))
    }

    fn list(&self, prefix: &str) -> RemoteResult<Vec<ListedObject>> {
        validate_prefix(prefix)?;
        let mut state = self.state();
        let stale = state.stale_listings.pop_front();
        let objects = stale.as_ref().unwrap_or(&state.objects);
        Ok(objects
            .iter()
            .filter(|(path, _)| is_under_prefix(path, prefix))
            .map(|(path, object)| ListedObject {
                path: path.clone(),
                metadata: object.metadata(),
            })
            .collect())
    }

    fn delete(&self, path: &str) -> RemoteResult<()> {
        validate_object_path(path)?;
        self.state().objects.remove(path);
        Ok(())
    }
}

impl ConformanceFixture for MemoryStorage {
    fn storage(&self) -> &dyn RepositoryStorage {
        self
    }

    fn script(&self, fault: Fault) {
        MemoryStorage::script(self, fault);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod publishable {
        use super::*;
        crate::storage_conformance_suite!(MemoryStorage::new());
    }

    mod read_only_without_ranges {
        use super::*;
        crate::storage_conformance_suite!(MemoryStorage::with_capabilities(StorageCapabilities {
            conditional_replace: false,
            range_read: false,
        }));
    }

    #[test]
    fn clones_share_one_repository_like_two_devices() {
        let desktop = MemoryStorage::new();
        let phone = desktop.clone();
        let written = desktop
            .write_new("media/ab/song.flac", b"song")
            .expect("desktop uploads");
        assert_eq!(
            phone.stat("media/ab/song.flac").expect("phone stats"),
            Some(written)
        );
    }

    #[test]
    fn scripted_faults_only_fire_on_their_own_path() {
        let storage = MemoryStorage::new();
        storage.write_new("media/a.flac", b"aaaa").expect("seed a");
        storage.write_new("media/b.flac", b"bbbb").expect("seed b");
        storage.script(Fault::InterruptTransfer {
            path: "media/a.flac".into(),
            after_bytes: 1,
        });
        storage.script(Fault::FailPrecondition {
            path: "media/c.flac".into(),
        });

        let mut received = Vec::new();
        storage
            .read("media/b.flac", &mut received)
            .expect("other paths read normally");
        storage
            .write_new("media/d.flac", b"dddd")
            .expect("other paths write normally");
        assert!(storage.read("media/a.flac", &mut Vec::new()).is_err());
        assert!(storage.write_new("media/c.flac", b"cccc").is_err());
    }

    #[test]
    fn a_partial_write_waits_for_a_write_whose_precondition_holds() {
        let storage = MemoryStorage::new();
        storage
            .write_new("media/a.flac", b"original")
            .expect("seed");
        storage.script(Fault::PartialWrite {
            path: "media/a.flac".into(),
            stored_bytes: 2,
        });

        let conflict = storage
            .write_new("media/a.flac", b"replacement")
            .expect_err("the object exists");
        assert_eq!(conflict.kind, RemoteErrorKind::RemoteConflict);
        let revision = storage
            .stat("media/a.flac")
            .expect("stat")
            .and_then(|metadata| metadata.revision)
            .expect("revision");
        storage
            .conditional_replace("media/a.flac", b"replacement", &revision)
            .expect_err("the pending partial write fires");
        let mut stored = Vec::new();
        storage.read("media/a.flac", &mut stored).expect("read");
        assert_eq!(stored, b"re");
    }
}
