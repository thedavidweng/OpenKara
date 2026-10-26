use std::io::Write;

use crate::error::{RemoteError, RemoteErrorKind, RemoteResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageCapabilities {
    pub conditional_replace: bool,
    pub range_read: bool,
}

impl StorageCapabilities {
    /// Publish Changes commits by conditionally replacing the manifest, so a
    /// storage without conditional replace can only be read.
    pub fn can_publish(self) -> bool {
        self.conditional_replace
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMetadata {
    pub size_bytes: u64,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedObject {
    pub path: String,
    pub metadata: ObjectMetadata,
}

/// Byte-level access to the objects of one Remote Repository.
///
/// Paths are relative to the repository root, use `/` separators, and pass
/// [`validate_object_path`]. A storage that also declares
/// `conditional_replace` reports a revision for every object, and every write
/// produces a new revision.
pub trait RepositoryStorage {
    fn capabilities(&self) -> StorageCapabilities;

    /// Returns `None` when no object exists at `path`.
    fn stat(&self, path: &str) -> RemoteResult<Option<ObjectMetadata>>;

    /// Streams the whole object into `sink` and returns the byte count. When
    /// the transfer fails midway, `sink` keeps the prefix that arrived, so the
    /// caller can resume with [`RepositoryStorage::read_range`].
    fn read(&self, path: &str, sink: &mut dyn Write) -> RemoteResult<u64>;

    /// Streams bytes `[offset, offset + length)` into `sink`, clamped at the
    /// end of the object, and returns the byte count. A zero `length` returns
    /// 0 without a transfer. A window that starts at or past the end of the
    /// object fails with `RemoteIntegrityFailed`. Interrupted transfers leave
    /// the received prefix in `sink`, as for [`RepositoryStorage::read`].
    fn read_range(
        &self,
        path: &str,
        offset: u64,
        length: u64,
        sink: &mut dyn Write,
    ) -> RemoteResult<u64>;

    /// Creates the object and any parent directories. Fails with
    /// `RemoteConflict` when an object already exists at `path`; it never
    /// replaces one.
    fn write_new(&self, path: &str, bytes: &[u8]) -> RemoteResult<ObjectMetadata>;

    /// Replaces the object only while its revision still equals
    /// `expected_revision`. Fails with `RemoteConflict` when the revision moved
    /// or the object is absent, and with `ProviderCapabilityUnavailable` when
    /// the storage cannot replace conditionally.
    fn conditional_replace(
        &self,
        path: &str,
        bytes: &[u8],
        expected_revision: &str,
    ) -> RemoteResult<ObjectMetadata>;

    /// Lists every object under the directory `prefix`, recursively, sorted
    /// by path. An empty prefix lists the whole repository. `media` matches
    /// `media/a` but not `media-g/a`.
    fn list(&self, prefix: &str) -> RemoteResult<Vec<ListedObject>>;

    /// Deleting an absent object succeeds.
    fn delete(&self, path: &str) -> RemoteResult<()>;
}

/// Rejects paths that could address anything outside the repository root.
pub fn validate_object_path(path: &str) -> RemoteResult<()> {
    if path.is_empty() {
        return Err(invalid_path(path));
    }
    validate_prefix(path)
}

/// Like [`validate_object_path`], but accepts the empty repository root.
pub fn validate_prefix(prefix: &str) -> RemoteResult<()> {
    if prefix.is_empty() {
        return Ok(());
    }
    let malformed = prefix.contains(['\\', '\0'])
        || prefix
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..");
    if malformed {
        return Err(invalid_path(prefix));
    }
    Ok(())
}

pub fn is_under_prefix(path: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn invalid_path(path: &str) -> RemoteError {
    RemoteError::new(
        RemoteErrorKind::PermissionDenied,
        format!("repository object path {path:?} is outside the repository root"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_paths_must_stay_inside_the_repository_root() {
        for path in [
            "",
            "/media/a",
            "media/",
            "media//a",
            "../a",
            "media/./a",
            "media\\a",
        ] {
            assert!(validate_object_path(path).is_err(), "{path:?} accepted");
        }
        assert!(validate_object_path(".openkara-repository.json").is_ok());
        assert!(validate_object_path(".openkara/databases/1/ab.sqlite").is_ok());
        assert!(validate_prefix("").is_ok());
    }

    #[test]
    fn prefixes_match_whole_directory_names() {
        assert!(is_under_prefix("media/a", "media"));
        assert!(is_under_prefix("media/a/b", "media/a"));
        assert!(!is_under_prefix("media-g/a", "media"));
        assert!(!is_under_prefix("media", "media"));
        assert!(is_under_prefix("media", ""));
    }

    #[test]
    fn only_conditional_replace_makes_a_storage_publishable() {
        let read_only = StorageCapabilities {
            conditional_replace: false,
            range_read: true,
        };
        let publishable = StorageCapabilities {
            conditional_replace: true,
            range_read: false,
        };
        assert!(!read_only.can_publish());
        assert!(publishable.can_publish());
    }
}
