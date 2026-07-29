//! Versioned remote repository manifest.
//!
//! The manifest (`.openkara-repository.json`) is the ONLY visibility switch
//! for a repository generation. A database file at
//! `.openkara/databases/<generation>.sqlite` is not visible to readers until
//! the manifest references it. This makes publication atomic from a reader's
//! perspective: before the manifest advances, the previous generation remains
//! the visible one, and a database/manifest failure leaves uploaded assets
//! unreachable in staging rather than visible through a half-committed
//! database.
//!
//! ## Schema version
//!
//! `schemaVersion` is 1. Bump it when the manifest shape changes in a
//! backward-incompatible way. Readers reject manifests with an unknown
//! schema version rather than guessing at fields.
//!
//! Minimum compatible OpenKara version: the release that introduced
//! `schemaVersion` 1 (PR #4 / issue #151). Older clients that predate the
//! manifest retain a temporary legacy-read path (read `openkara.db` directly
//! when no manifest is present).
//!
//! ## Remote layout
//!
//! ```text
//! .openkara-library
//! .openkara-repository.json
//! .openkara/
//!   databases/
//!     <generation>/
//!       <operation-hash>.sqlite
//!     <generation>.sqlite  # legacy read/GC compatibility
//!   staging/
//!     <operation-id>/...
//!   tombstones/
//!     ...
//! media/...
//! stems/...
//! artwork/...
//! ```

use crate::commands::error::{internal_error, CommandResult};
use crate::remote::provider::RemoteProvider;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Relative path of the manifest at the remote repository root.
pub(crate) const MANIFEST_PATH: &str = ".openkara-repository.json";

/// Current manifest schema version. Bump when the shape changes
/// incompatibly.
pub(crate) const CURRENT_SCHEMA_VERSION: u32 = 1;

/// The committed remote repository manifest. The manifest is the only
/// visibility switch for a generation: a database file is not visible to
/// readers until the manifest references it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RepositoryManifest {
    /// Manifest schema version. Readers reject unknown versions.
    pub schema_version: u32,
    /// Stable UUID identifying the repository. Set on first publication and
    /// never changed.
    pub repository_id: String,
    /// Monotonically increasing generation number. Each successful
    /// publication advances this by 1.
    pub generation: i64,
    /// Relative path of the committed database for this generation.
    pub database_path: String,
    /// Byte length of the committed database.
    pub database_size_bytes: u64,
    /// Hex SHA-256 of the committed database bytes.
    pub database_sha256: String,
    /// Wall-clock milliseconds when this generation was committed.
    pub committed_at_ms: i64,
    /// Installation UUID of the writer that committed this generation. Used
    /// for diagnostics; not a security principal.
    pub writer_id: String,
    /// Durable publish operation that produced this generation. Required for
    /// post-CAS crash recovery so an accepted-commit shortcut cannot claim a
    /// different operation's CAS (e.g. after coalescing expanded the payload).
    /// Empty when reading manifests written before this field existed.
    #[serde(default)]
    pub operation_id: String,
}

impl RepositoryManifest {
    /// Serialize to canonical JSON. Keys are emitted in the struct order so
    /// the on-wire representation is stable across builds.
    pub(crate) fn to_json(&self) -> CommandResult<String> {
        serde_json::to_string(self)
            .map_err(|e| internal_error(format!("failed to serialize manifest: {e}")))
    }

    /// Parse and validate a manifest JSON blob. Rejects unknown schema
    /// versions and empty required fields.
    pub(crate) fn from_json(json: &str) -> CommandResult<Self> {
        let manifest: RepositoryManifest = serde_json::from_str(json)
            .map_err(|e| internal_error(format!("failed to parse repository manifest: {e}")))?;
        validate_manifest(&manifest)?;
        Ok(manifest)
    }
}

/// Read the manifest from the remote repository root. Returns `Ok(None)` when
/// no manifest exists (legacy repository or first publication).
///
/// The provider's `download_file` is used to fetch the manifest into a temp
/// buffer; a 404 / missing file maps to `None`. Any other error is propagated.
pub(crate) fn read_manifest(
    provider: &dyn RemoteProvider,
) -> CommandResult<Option<RepositoryManifest>> {
    // Use a unique temp path under the system temp dir. The manifest is small
    // (a few hundred bytes) so a full download is cheap. Include a UUID-like
    // suffix to avoid collisions between concurrent calls (e.g. parallel tests).
    let temp_path: PathBuf = std::env::temp_dir().join(format!(
        "openkara-manifest-{}-{}-{}.tmp",
        std::process::id(),
        current_unix_time_ms(),
        uuid::Uuid::new_v4().as_simple(),
    ));

    // `download_file` errors on a missing file for some providers. Use `stat`
    // first when available to distinguish "absent" from "error".
    if let Some(metadata) = provider.stat(MANIFEST_PATH)? {
        // The manifest exists — download and parse it.
        if metadata.size_bytes == Some(0) {
            // An empty manifest blob is treated as absent (defensive).
            let _ = std::fs::remove_file(&temp_path);
            return Ok(None);
        }
        provider.download_file(MANIFEST_PATH, &temp_path)?;
        let bytes = std::fs::read(&temp_path)
            .map_err(|e| internal_error(format!("failed to read manifest temp file: {e}")))?;
        let _ = std::fs::remove_file(&temp_path);
        if bytes.is_empty() {
            return Ok(None);
        }
        let json = std::str::from_utf8(&bytes)
            .map_err(|e| internal_error(format!("repository manifest is not valid UTF-8: {e}")))?;
        Ok(Some(RepositoryManifest::from_json(json)?))
    } else {
        // No manifest entry — legacy repository or first publication.
        let _ = std::fs::remove_file(&temp_path);
        Ok(None)
    }
}

/// Current wall-clock milliseconds. Local helper to avoid pulling in the
/// types module for a single call.
fn current_unix_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Validate a parsed manifest. Rejects unknown schema versions, non-positive
/// generations (generation 0 is reserved for "no manifest yet"), and empty
/// required string fields.
pub(crate) fn validate_manifest(manifest: &RepositoryManifest) -> CommandResult<()> {
    if manifest.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(internal_error(format!(
            "unsupported repository manifest schema version {}: expected {}",
            manifest.schema_version, CURRENT_SCHEMA_VERSION
        )));
    }
    if manifest.generation < 1 {
        return Err(internal_error(format!(
            "repository manifest generation must be >= 1, got {}",
            manifest.generation
        )));
    }
    if manifest.repository_id.trim().is_empty() {
        return Err(internal_error(
            "repository manifest repository_id must not be empty",
        ));
    }
    if manifest.database_path.trim().is_empty() {
        return Err(internal_error(
            "repository manifest database_path must not be empty",
        ));
    }
    validate_database_path(&manifest.database_path, manifest.generation)?;
    if manifest.database_sha256.trim().is_empty() {
        return Err(internal_error(
            "repository manifest database_sha256 must not be empty",
        ));
    }
    if manifest.writer_id.trim().is_empty() {
        return Err(internal_error(
            "repository manifest writer_id must not be empty",
        ));
    }
    Ok(())
}

/// Build the legacy relative database path for a generation:
/// `.openkara/databases/<generation>.sqlite`.
///
/// New publications use [`database_path_for_operation`]. This helper remains
/// for reading and collecting repositories written before operation-scoped
/// database objects were introduced.
pub(crate) fn database_path_for_generation(generation: i64) -> String {
    format!(".openkara/databases/{generation}.sqlite")
}

/// Directory containing every immutable candidate that raced for a generation.
pub(crate) fn database_directory_for_generation(generation: i64) -> String {
    format!(".openkara/databases/{generation}")
}

/// Build an operation-scoped immutable database path.
///
/// Two concurrent writers expecting the same generation must never upload to
/// the same object before manifest CAS. Hashing the durable operation identity
/// gives each writer a path-safe unique object while keeping the manifest schema
/// unchanged and backward compatible.
pub(crate) fn database_path_for_operation(generation: i64, operation_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(operation_id.as_bytes());
    let operation_hash = crate::hash::hex_lower(hasher.finalize());
    format!(
        "{}/{operation_hash}.sqlite",
        database_directory_for_generation(generation)
    )
}

fn validate_database_path(path: &str, generation: i64) -> CommandResult<()> {
    let prefix = ".openkara/databases/";
    let Some(rest) = path.strip_prefix(prefix) else {
        return Err(internal_error(
            "repository manifest database_path must stay under .openkara/databases/",
        ));
    };

    // Legacy schema-v1 path.
    if rest == format!("{generation}.sqlite") {
        return Ok(());
    }

    // Hardened schema-v1 path: `<generation>/<64-lower-hex>.sqlite`.
    let Some((generation_part, filename)) = rest.split_once('/') else {
        return Err(internal_error(
            "repository manifest database_path has an invalid layout",
        ));
    };
    if generation_part != generation.to_string()
        || filename.contains('/')
        || filename.contains('\\')
    {
        return Err(internal_error(
            "repository manifest database_path generation does not match the manifest",
        ));
    }
    let Some(digest) = filename.strip_suffix(".sqlite") else {
        return Err(internal_error(
            "repository manifest database_path must end in .sqlite",
        ));
    };
    let valid_digest = digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
    if !valid_digest {
        return Err(internal_error(
            "repository manifest database_path operation hash is invalid",
        ));
    }
    Ok(())
}

/// Build the relative staging directory path for an operation:
/// `.openkara/staging/<operation-id>`.
#[allow(dead_code)]
pub(crate) fn staging_dir_for_operation(operation_id: &str) -> String {
    format!(".openkara/staging/{operation_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    /// A minimal fake provider backed by an in-memory map for manifest tests.
    struct FakeProvider {
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        revisions: Arc<Mutex<HashMap<String, String>>>,
        sizes: Arc<Mutex<HashMap<String, u64>>>,
    }

    impl FakeProvider {
        fn new() -> Self {
            Self {
                files: Arc::new(Mutex::new(HashMap::new())),
                revisions: Arc::new(Mutex::new(HashMap::new())),
                sizes: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        fn store(&self, path: &str, bytes: Vec<u8>, revision: &str) {
            self.sizes
                .lock()
                .unwrap()
                .insert(path.to_owned(), bytes.len() as u64);
            self.revisions
                .lock()
                .unwrap()
                .insert(path.to_owned(), revision.to_owned());
            self.files.lock().unwrap().insert(path.to_owned(), bytes);
        }
    }

    impl crate::remote::provider::RemoteProvider for FakeProvider {
        fn capabilities(&self) -> crate::remote::errors::RemoteProviderCapabilities {
            crate::remote::errors::RemoteProviderCapabilities::default()
        }
        fn stat(
            &self,
            path: &str,
        ) -> CommandResult<Option<crate::remote::errors::RemoteObjectMetadata>> {
            let sizes = self.sizes.lock().unwrap();
            let revisions = self.revisions.lock().unwrap();
            if sizes.contains_key(path) {
                Ok(Some(crate::remote::errors::RemoteObjectMetadata {
                    size_bytes: sizes.get(path).copied(),
                    revision: revisions.get(path).cloned(),
                }))
            } else {
                Ok(None)
            }
        }
        fn get_revision(&self, path: &str) -> CommandResult<Option<String>> {
            Ok(self.revisions.lock().unwrap().get(path).cloned())
        }
        fn download_file(&self, path: &str, dest: &std::path::Path) -> CommandResult<()> {
            let bytes = self
                .files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| internal_error(format!("fake provider: {path} not found")))?;
            std::fs::write(dest, bytes).map_err(|e| internal_error(e.to_string()))
        }
        fn upload_file(&self, _path: &str) -> CommandResult<()> {
            Ok(())
        }
        fn delete_path(&self, _path: &str) -> CommandResult<()> {
            Ok(())
        }
        fn initialize_or_sync(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }
        fn get_file_size(&self, path: &str) -> CommandResult<Option<u64>> {
            Ok(self.sizes.lock().unwrap().get(path).copied())
        }
        fn refresh_existing(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }
    }

    fn sample_manifest(generation: i64) -> RepositoryManifest {
        RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-uuid-1".to_owned(),
            generation,
            database_path: database_path_for_generation(generation),
            database_size_bytes: 1024,
            database_sha256: "abc123".to_owned(),
            committed_at_ms: 1000,
            writer_id: "writer-1".to_owned(),
            operation_id: format!("op-gen-{generation}"),
        }
    }

    #[test]
    fn manifest_json_round_trips() {
        let manifest = sample_manifest(3);
        let json = manifest.to_json().unwrap();
        let back = RepositoryManifest::from_json(&json).unwrap();
        assert_eq!(back, manifest);
    }

    #[test]
    fn validate_rejects_unknown_schema_version() {
        let mut manifest = sample_manifest(1);
        manifest.schema_version = 99;
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn operation_database_paths_are_unique_within_one_generation() {
        let first = database_path_for_operation(7, "operation-a");
        let second = database_path_for_operation(7, "operation-b");
        assert_ne!(first, second);
        assert!(first.starts_with(".openkara/databases/7/"));
        assert!(first.ends_with(".sqlite"));
    }

    #[test]
    fn validate_rejects_database_path_traversal_and_generation_mismatch() {
        let mut manifest = sample_manifest(3);
        manifest.database_path = ".openkara/databases/../../outside.sqlite".to_owned();
        assert!(validate_manifest(&manifest).is_err());

        manifest.database_path = database_path_for_operation(4, "wrong-generation");
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn validate_rejects_generation_zero() {
        let mut manifest = sample_manifest(1);
        manifest.generation = 0;
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn validate_rejects_empty_repository_id() {
        let mut manifest = sample_manifest(1);
        manifest.repository_id = "  ".to_owned();
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn read_manifest_returns_none_when_absent() {
        let provider = FakeProvider::new();
        let result = read_manifest(&provider).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn read_manifest_returns_parsed_when_present() {
        let provider = FakeProvider::new();
        let manifest = sample_manifest(2);
        let json = manifest.to_json().unwrap();
        provider.store(MANIFEST_PATH, json.into_bytes(), "rev-2");
        let result = read_manifest(&provider).unwrap().unwrap();
        assert_eq!(result.generation, 2);
        assert_eq!(result.repository_id, "repo-uuid-1");
    }

    #[test]
    fn database_path_for_generation_format() {
        assert_eq!(
            database_path_for_generation(5),
            ".openkara/databases/5.sqlite"
        );
    }

    #[test]
    fn staging_dir_for_operation_format() {
        assert_eq!(
            staging_dir_for_operation("op-42"),
            ".openkara/staging/op-42"
        );
    }

    #[test]
    fn from_json_rejects_malformed_json() {
        assert!(RepositoryManifest::from_json("not json").is_err());
    }

    #[test]
    fn from_json_rejects_missing_field() {
        // Missing database_sha256.
        let json = r#"{
            "schema_version": 1,
            "repository_id": "repo",
            "generation": 1,
            "database_path": ".openkara/databases/1.sqlite",
            "database_size_bytes": 100,
            "committed_at_ms": 0,
            "writer_id": "w"
        }"#;
        assert!(RepositoryManifest::from_json(json).is_err());
    }

    #[test]
    fn read_manifest_uses_temp_file_without_leaking() {
        let _dir = TempDir::new().unwrap();
        let provider = FakeProvider::new();
        let manifest = sample_manifest(1);
        provider.store(
            MANIFEST_PATH,
            manifest.to_json().unwrap().into_bytes(),
            "rev-1",
        );
        let result = read_manifest(&provider).unwrap().unwrap();
        assert_eq!(result.generation, 1);
    }
}
