use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{RemoteError, RemoteErrorKind, RemoteResult};

pub const MANIFEST_PATH: &str = ".openkara-repository.json";
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryManifest {
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
    pub fn to_json(&self) -> RemoteResult<String> {
        serde_json::to_string(self).map_err(|error| {
            RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!("failed to serialize manifest: {error}"),
            )
        })
    }

    pub fn from_json(json: &str) -> RemoteResult<Self> {
        let manifest: Self = serde_json::from_str(json).map_err(|error| {
            RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!("failed to parse repository manifest: {error}"),
            )
        })?;
        validate_manifest(&manifest)?;
        Ok(manifest)
    }
}

pub fn validate_manifest(manifest: &RepositoryManifest) -> RemoteResult<()> {
    if manifest.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "unsupported repository manifest schema version {}: expected {}",
                manifest.schema_version, CURRENT_SCHEMA_VERSION
            ),
        ));
    }
    if manifest.generation < 1 {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "repository manifest generation must be >= 1, got {}",
                manifest.generation
            ),
        ));
    }
    if manifest.repository_id.trim().is_empty() {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "repository manifest repository_id must not be empty",
        ));
    }
    if manifest.database_path.trim().is_empty() {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "repository manifest database_path must not be empty",
        ));
    }
    validate_database_path(&manifest.database_path, manifest.generation)?;
    if manifest.database_sha256.trim().is_empty() {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "repository manifest database_sha256 must not be empty",
        ));
    }
    if manifest.writer_id.trim().is_empty() {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "repository manifest writer_id must not be empty",
        ));
    }
    Ok(())
}

pub fn database_path_for_generation(generation: i64) -> String {
    format!(".openkara/databases/{generation}.sqlite")
}

pub fn database_directory_for_generation(generation: i64) -> String {
    format!(".openkara/databases/{generation}")
}

/// Two concurrent writers must never upload to the same object before
/// manifest CAS.
pub fn database_path_for_operation(generation: i64, operation_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(operation_id.as_bytes());
    let digest = hasher.finalize();
    let mut operation_hash = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(operation_hash, "{byte:02x}");
    }
    format!(
        "{}/{operation_hash}.sqlite",
        database_directory_for_generation(generation)
    )
}

fn validate_database_path(path: &str, generation: i64) -> RemoteResult<()> {
    let prefix = ".openkara/databases/";
    let Some(rest) = path.strip_prefix(prefix) else {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "repository manifest database_path must stay under .openkara/databases/",
        ));
    };

    // Legacy schema-v1 path.
    if rest == format!("{generation}.sqlite") {
        return Ok(());
    }

    // Hardened schema-v1 path: `<generation>/<64-lower-hex>.sqlite`.
    let Some((generation_part, filename)) = rest.split_once('/') else {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "repository manifest database_path has an invalid layout",
        ));
    };
    if generation_part != generation.to_string()
        || filename.contains('/')
        || filename.contains('\\')
    {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "repository manifest database_path generation does not match the manifest",
        ));
    }
    let Some(digest) = filename.strip_suffix(".sqlite") else {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "repository manifest database_path must end in .sqlite",
        ));
    };
    let valid_digest = digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
    if !valid_digest {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "repository manifest database_path operation hash is invalid",
        ));
    }
    Ok(())
}

pub fn staging_dir_for_operation(operation_id: &str) -> String {
    format!(".openkara/staging/{operation_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn manifest_round_trips_through_json() {
        let manifest = sample_manifest(5);
        let json = manifest.to_json().unwrap();
        let restored = RepositoryManifest::from_json(&json).unwrap();
        assert_eq!(restored, manifest);
    }

    #[test]
    fn manifest_backward_compatible_without_operation_id() {
        let json = r#"{
            "schema_version": 1,
            "repository_id": "repo-1",
            "generation": 1,
            "database_path": ".openkara/databases/1.sqlite",
            "database_size_bytes": 100,
            "database_sha256": "abc",
            "committed_at_ms": 1000,
            "writer_id": "w1"
        }"#;
        let manifest = RepositoryManifest::from_json(json).unwrap();
        assert_eq!(manifest.operation_id, "");
    }

    #[test]
    fn reject_wrong_schema_version() {
        let mut manifest = sample_manifest(1);
        manifest.schema_version = 99;
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn reject_zero_or_negative_generation() {
        let mut manifest = sample_manifest(1);
        manifest.generation = 0;
        assert!(validate_manifest(&manifest).is_err());
        manifest.generation = -1;
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn reject_empty_repository_id() {
        let mut manifest = sample_manifest(1);
        manifest.repository_id = "  ".to_owned();
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn reject_traversal_and_generation_mismatch_in_database_path() {
        let mut manifest = sample_manifest(3);
        manifest.database_path = ".openkara/databases/../../outside.sqlite".to_owned();
        assert!(validate_manifest(&manifest).is_err());

        manifest.database_path = database_path_for_operation(4, "wrong-generation");
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn database_path_for_operation_validates_and_is_deterministic() {
        let path_a = database_path_for_operation(7, "operation-a");
        let path_b = database_path_for_operation(7, "operation-a");
        let path_c = database_path_for_operation(7, "operation-b");
        assert_eq!(path_a, path_b);
        assert_ne!(path_a, path_c);
        assert!(path_a.starts_with(".openkara/databases/7/"));
        assert!(path_a.ends_with(".sqlite"));

        let mut manifest = sample_manifest(7);
        manifest.database_path = path_a;
        assert!(validate_manifest(&manifest).is_ok());

        assert_eq!(
            database_path_for_generation(5),
            ".openkara/databases/5.sqlite"
        );
        assert_eq!(
            staging_dir_for_operation("op-42"),
            ".openkara/staging/op-42"
        );
    }
}
