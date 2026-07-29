//! Versioned remote repository manifest.
//!
//! The manifest is the only visibility switch: a database file is not visible
//! until the manifest references it. (PR #4 / issue #151)

use crate::commands::error::{internal_error, CommandResult};
use crate::remote::provider::RemoteProvider;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub(crate) const MANIFEST_PATH: &str = ".openkara-repository.json";

pub(crate) const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RepositoryManifest {
    pub schema_version: u32,
    pub repository_id: String,
    pub generation: i64,
    pub database_path: String,
    pub database_size_bytes: u64,
    pub database_sha256: String,
    pub committed_at_ms: i64,
    pub writer_id: String,
    #[serde(default)]
    pub operation_id: String,
}

impl RepositoryManifest {
    pub(crate) fn to_json(&self) -> CommandResult<String> {
        serde_json::to_string(self)
            .map_err(|e| internal_error(format!("failed to serialize manifest: {e}")))
    }

    pub(crate) fn from_json(json: &str) -> CommandResult<Self> {
        let manifest: RepositoryManifest = serde_json::from_str(json)
            .map_err(|e| internal_error(format!("failed to parse repository manifest: {e}")))?;
        validate_manifest(&manifest)?;
        Ok(manifest)
    }
}

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
        let _ = std::fs::remove_file(&temp_path);
        Ok(None)
    }
}

fn current_unix_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

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

pub(crate) fn database_path_for_generation(generation: i64) -> String {
    format!(".openkara/databases/{generation}.sqlite")
}

pub(crate) fn database_directory_for_generation(generation: i64) -> String {
    format!(".openkara/databases/{generation}")
}

/// Two concurrent writers must never upload to the same object before
/// manifest CAS. (PR #4)
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
