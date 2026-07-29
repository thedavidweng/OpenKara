//! Transactional publish operation executor.
//!
//! Implements the 13-step publication protocol from issue #151:
//!
//! 1. Read and validate the current manifest + provider revision (stat).
//! 2. Require `manifest.generation == operation.expected_generation`.
//! 3. Upload changed assets to final / staging paths.
//! 4. Verify every uploaded object by `stat` + expected size/digest.
//! 5. Copy the local working database to a candidate temp file.
//! 6. SQLite integrity checks + SHA-256 digest.
//! 7. Upload the candidate to `.openkara/databases/<target>.sqlite`.
//! 8. Stat-verify the candidate database metadata.
//! 9. Build the next manifest referencing only verified objects.
//! 10. Replace `.openkara-repository.json` via `conditional_replace` (CAS).
//! 11. Re-read the manifest and verify generation/path/size/digest.
//! 12. Commit local operation state as `completed` + emit `upload-complete`.
//! 13. Schedule a `Gc` operation row for deferred cleanup.
//!
//! ## Defect #2 fix
//!
//! Completion (`upload-complete` / `Completed` state) is stored and emitted
//! ONLY after the manifest is committed and re-verified (step 11). The
//! asset-upload portion never emits `upload-complete`. A background-thread
//! error is persisted to the operation row and emitted as `upload-error`
//! rather than discarded.
//!
//! ## Conflict handling
//!
//! A failed manifest CAS (step 10) marks the operation `conflicted` and the
//! repository `Conflicted`. It NEVER retries as an unconditional overwrite.
//! The winning remote manifest + database are pulled to a conflict candidate
//! (not the active working DB) for the conflict-resolution actions.

use crate::commands::error::{database_error, internal_error, CommandResult};
use crate::remote::atomic_download::{sha256_file, verify_sqlite_integrity_pub};
use crate::remote::control_db::{
    get_operation, get_repository_state, list_operations_for_library, upsert_operation,
    upsert_repository_state, LocalState, OperationKind, OperationPayload, OperationRow,
    OperationState, RepositoryStateRow,
};
use crate::remote::errors::{RemoteError, RemoteErrorKind};
use crate::remote::manifest::{
    database_directory_for_generation, database_path_for_generation, database_path_for_operation,
    read_manifest, RepositoryManifest, CURRENT_SCHEMA_VERSION,
};
use crate::remote::provider::{ConditionalSource, RemoteProvider};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub(crate) struct PublishContext<'a> {
    pub control_db: &'a Connection,
    pub provider: &'a dyn RemoteProvider,
    pub working_copy_root: &'a Path,
    pub library_id: &'a str,
    pub writer_id: &'a str,
    pub repository_id: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct PublishOutcome {
    #[allow(dead_code)]
    pub operation_id: String,
    pub target_generation: i64,
    pub committed_manifest_revision: Option<String>,
    pub candidate_db_digest: String,
}

/// Execute the transactional publish protocol for a single operation row.
///
/// The operation row must already exist in `remote_operations` (created by the
/// mutation outbox in PR#2). This function drives it through the state machine:
/// `pending → running → committing → verifying → completed` (or a failure
/// state).
///
/// The caller is responsible for acquiring the per-library commit lock
/// (`commit_lock(library_id)`) before calling this, so two concurrent
/// manifest commits for the same library cannot run.
pub(crate) fn execute_publish(ctx: &PublishContext<'_>, operation_id: &str) -> CommandResult<()> {
    let op = get_operation(ctx.control_db, operation_id)?
        .ok_or_else(|| internal_error(format!("operation {operation_id} not found")))?;

    // If the operation is already terminal, do not re-execute.
    if matches!(
        op.state,
        OperationState::Completed
            | OperationState::Failed
            | OperationState::Conflicted
            | OperationState::Cancelled
    ) {
        return Ok(());
    }

    let result = run_publish_protocol(ctx, &op);
    match result {
        Ok(outcome) => {
            record_completed(ctx, &op, &outcome)?;
            Ok(())
        }
        Err(remote_err) => {
            record_failure(ctx, &op, &remote_err)?;
            Err(remote_err.to_command_error())
        }
    }
}

/// Run the 13-step publication protocol. Returns the outcome on success or a
/// typed `RemoteError` on failure. State transitions are persisted at each
/// milestone so a crash leaves the operation in a resumable state.
fn run_publish_protocol(
    ctx: &PublishContext<'_>,
    op: &OperationRow,
) -> Result<PublishOutcome, RemoteError> {
    let now = current_unix_time_ms();

    transition_state(
        ctx,
        op,
        OperationState::Running,
        now,
        5,
        "Reading remote manifest",
    )?;

    let caps = ctx.provider.capabilities();
    if !caps.conditional_replace {
        return Err(RemoteError::new(
            RemoteErrorKind::ProviderCapabilityUnavailable,
            "provider does not support conditional replacement; publication is blocked",
        ));
    }

    let current_manifest = read_manifest(ctx.provider)
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;

    let manifest_revision: Option<String> = ctx
        .provider
        .stat(crate::remote::manifest::MANIFEST_PATH)
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?
        .and_then(|m| m.revision);

    // If a manifest exists but the provider returns no revision (CAS token),
    // fail closed. Without a revision, conditional_replace becomes a
    // conditional-create instead of a compare-and-swap, which violates the
    // CAS guarantee and can cause lost updates on providers that omit ETags
    // (e.g. some WebDAV servers).
    if current_manifest.is_some() && manifest_revision.is_none() {
        return Err(RemoteError::new(
            RemoteErrorKind::ProviderCapabilityUnavailable,
            "manifest exists but provider returned no revision (CAS token); \
             cannot safely replace without a compare-and-swap guard",
        ));
    }

    let expected_generation = op.expected_generation.unwrap_or(0);
    let current_generation = current_manifest.as_ref().map(|m| m.generation).unwrap_or(0);
    let target_generation = expected_generation + 1;

    if current_generation != expected_generation {
        // Crash window after a successful manifest CAS: the remote advanced
        // exactly one generation by this writer, but local completion was
        // never recorded. Detect our own accepted commit and finish durably
        // instead of surfacing a false RemoteConflict that would leave the
        // working copy dirty forever.
        //
        // Identity is operation-scoped: writer_id alone is insufficient after
        // coalescing may have expanded the payload and cleared a prior
        // candidate. Require operation_id (when present on the manifest) and
        // an exact match against the durable immutable candidate digest/size.
        if current_generation == target_generation {
            if let Some(ref m) = current_manifest {
                if is_accepted_commit_for_operation(m, ctx, op) {
                    tracing::info!(
                        "publish recovery: remote generation {} accepted for operation {} \
                         (digest match); finishing durably",
                        current_generation,
                        op.operation_id
                    );
                    return Ok(PublishOutcome {
                        operation_id: op.operation_id.clone(),
                        target_generation: current_generation,
                        committed_manifest_revision: manifest_revision,
                        candidate_db_digest: m.database_sha256.clone(),
                    });
                }
            }
        }
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteConflict,
            format!(
                "expected generation {expected_generation} but remote is at {current_generation}"
            ),
        ));
    }

    let payload = OperationPayload::from_json(&op.payload_json).map_err(|error| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "publish operation {} has invalid payload JSON: {}",
                op.operation_id, error.message
            ),
        )
    })?;
    let persisted_candidate = load_persisted_candidate(ctx, op, &payload)?;
    let working_db_path = ctx.working_copy_root.join("openkara.db");

    // --- Steps 3-4: Asset verification for a new freeze ---
    //
    // Before the first freeze, the working DB is stable under the per-library
    // commit lock. Capture a deterministic fingerprint of every candidate-
    // referenced remote object's size/revision so retries can verify the same
    // asset set without consulting mutable local files.
    let fresh_asset_fingerprint = if persisted_candidate.is_none() {
        transition_state(
            ctx,
            op,
            OperationState::Running,
            now,
            15,
            "Verifying remote assets",
        )?;
        Some(verify_referenced_assets(
            ctx.provider,
            ctx.working_copy_root,
            &working_db_path,
            true,
        )?)
    } else {
        None
    };

    transition_state(
        ctx,
        op,
        OperationState::Committing,
        now,
        50,
        "Preparing candidate database",
    )?;

    let (
        candidate_relative,
        candidate_path,
        candidate_digest,
        candidate_size,
        candidate_assets_fingerprint,
        op,
    ) = if let Some(candidate) = persisted_candidate {
        // The candidate DB, not the mutable working DB, defines this retry's
        // asset set. Recompute remote metadata and require the exact fingerprint
        // captured before the original freeze. This detects missing, truncated,
        // or replaced remote assets without reading newer local bytes.
        let actual_fingerprint =
            verify_referenced_assets(ctx.provider, ctx.working_copy_root, &candidate.path, false)?;
        if actual_fingerprint != candidate.asset_fingerprint {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!(
                    "remote asset fingerprint changed for operation {}",
                    op.operation_id
                ),
            ));
        }
        tracing::info!(
            "reusing immutable candidate for operation {} (sha256={})",
            op.operation_id,
            candidate.digest
        );
        (
            candidate.relative_path,
            candidate.path,
            candidate.digest,
            candidate.size_bytes,
            candidate.asset_fingerprint,
            op.clone(),
        )
    } else {
        let relative = format!(".openkara/candidates/{}.sqlite", op.operation_id);
        let path = ctx.working_copy_root.join(&relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!("failed to create candidate dir: {e}"),
                )
            })?;
        }
        let _ = std::fs::remove_file(&path);
        std::fs::copy(&working_db_path, &path).map_err(|e| {
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("failed to copy working DB to candidate: {e}"),
            )
        })?;

        // Machine-local control metadata must not ship with a generation
        // candidate. Fail closed: open/cleanup failure aborts publication
        // before integrity/digest/upload so outbox rows cannot CAS.
        {
            let cand = rusqlite::Connection::open(&path).map_err(|e| {
                let _ = std::fs::remove_file(&path);
                RemoteError::new(
                    RemoteErrorKind::RemoteIntegrityFailed,
                    format!("failed to open candidate for outbox sanitation: {e}"),
                )
            })?;
            crate::remote::library_outbox::clear_all_library_publish_outbox(&cand).map_err(
                |e| {
                    let _ = std::fs::remove_file(&path);
                    RemoteError::new(
                        RemoteErrorKind::RemoteIntegrityFailed,
                        format!(
                            "failed to clear machine-local outbox from candidate: {}",
                            e.message
                        ),
                    )
                },
            )?;
            let remaining: i64 =
                match cand.query_row("SELECT COUNT(*) FROM remote_publish_outbox", [], |row| {
                    row.get(0)
                }) {
                    Ok(n) => n,
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("no such table") {
                            0
                        } else {
                            let _ = std::fs::remove_file(&path);
                            return Err(RemoteError::new(
                                RemoteErrorKind::RemoteIntegrityFailed,
                                format!("failed to verify outbox cleanup on candidate: {e}"),
                            ));
                        }
                    }
                };
            if remaining > 0 {
                let _ = std::fs::remove_file(&path);
                return Err(RemoteError::new(
                    RemoteErrorKind::RemoteIntegrityFailed,
                    format!(
                        "candidate still has {remaining} remote_publish_outbox row(s) after cleanup"
                    ),
                ));
            }
        }

        verify_sqlite_integrity_pub(&path).map_err(|e| {
            let _ = std::fs::remove_file(&path);
            RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, e.message)
        })?;

        let digest = sha256_file(&path).map_err(|e| {
            let _ = std::fs::remove_file(&path);
            RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message)
        })?;
        let size = std::fs::metadata(&path).map(|m| m.len()).map_err(|e| {
            let _ = std::fs::remove_file(&path);
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("failed to stat candidate: {e}"),
            )
        })?;
        let asset_fingerprint = fresh_asset_fingerprint.ok_or_else(|| {
            RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                "new candidate is missing its verified remote asset fingerprint",
            )
        })?;
        let updated = persist_candidate_identity(
            ctx,
            op,
            &relative,
            size,
            &digest,
            &asset_fingerprint,
            "candidate_ready",
        )?;
        (relative, path, digest, size, asset_fingerprint, updated)
    };

    let db_remote_path = database_path_for_operation(target_generation, &op.operation_id);
    let candidate_bytes = std::fs::read(&candidate_path).map_err(|e| {
        RemoteError::new(
            RemoteErrorKind::NetworkUnavailable,
            format!("failed to read candidate for upload: {e}"),
        )
    })?;
    // Generation-specific path: no CAS conflict here. The CAS point is the
    // manifest replacement (step 10). Do NOT delete the local candidate on
    // upload failure — a restart must resume the same immutable bytes.
    upload_candidate_database(
        ctx.provider,
        ctx.working_copy_root,
        &db_remote_path,
        &candidate_bytes,
        &candidate_digest,
        candidate_size,
        &op.operation_id,
        ctx.control_db,
    )?;
    // Verify the remote object matches the immutable candidate digest by
    // downloading it back to a temp path. Size-only checks are insufficient
    // when a resumable session could have mixed two candidates of equal size.
    verify_remote_candidate_digest(
        ctx.provider,
        &db_remote_path,
        candidate_size,
        &candidate_digest,
        &op.operation_id,
        ctx.working_copy_root,
    )?;
    let op = persist_candidate_identity(
        ctx,
        &op,
        &candidate_relative,
        candidate_size,
        &candidate_digest,
        &candidate_assets_fingerprint,
        "candidate_uploaded",
    )?;

    let candidate_meta = ctx
        .provider
        .stat(&db_remote_path)
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
    let candidate_meta = candidate_meta.ok_or_else(|| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "candidate database was not found after upload",
        )
    })?;
    if let Some(remote_size) = candidate_meta.size_bytes {
        if remote_size != candidate_size {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!(
                    "candidate database size mismatch: expected {candidate_size}, remote has {remote_size}"
                ),
            ));
        }
    }

    // Close the candidate-upload race window: immediately before manifest CAS,
    // re-stat every candidate-referenced asset and require the exact metadata
    // fingerprint captured before freeze.
    let pre_cas_asset_fingerprint =
        verify_referenced_assets(ctx.provider, ctx.working_copy_root, &candidate_path, false)?;
    if pre_cas_asset_fingerprint != candidate_assets_fingerprint {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "remote asset fingerprint changed before manifest CAS for operation {}",
                op.operation_id
            ),
        ));
    }

    let manifest = RepositoryManifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        repository_id: ctx.repository_id.to_owned(),
        generation: target_generation,
        database_path: db_remote_path.clone(),
        database_size_bytes: candidate_size,
        database_sha256: candidate_digest.clone(),
        committed_at_ms: now,
        writer_id: ctx.writer_id.to_owned(),
        operation_id: op.operation_id.clone(),
    };
    let manifest_json = manifest
        .to_json()
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;

    let committed_meta = ctx.provider.conditional_replace(
        crate::remote::manifest::MANIFEST_PATH,
        ConditionalSource::Bytes(manifest_json.into_bytes()),
        manifest_revision.as_deref(),
    )?;
    // A CAS failure is a conflict — do NOT retry as unconditional.
    // The RemoteError is propagated as-is via the `?` operator.

    transition_state(
        ctx,
        &op,
        OperationState::Verifying,
        now,
        90,
        "Verifying manifest",
    )?;

    let verified_manifest = read_manifest(ctx.provider)
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
    let verified_manifest = verified_manifest.ok_or_else(|| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "manifest was not found after commit",
        )
    })?;
    if verified_manifest.generation != target_generation
        || verified_manifest.database_path != db_remote_path
        || verified_manifest.database_size_bytes != candidate_size
        || verified_manifest.database_sha256 != candidate_digest
        || verified_manifest.operation_id != op.operation_id
    {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "manifest verification failed: committed manifest does not match",
        ));
    }

    Ok(PublishOutcome {
        operation_id: op.operation_id.clone(),
        target_generation,
        committed_manifest_revision: committed_meta.revision,
        candidate_db_digest: candidate_digest,
    })
}

#[derive(Debug)]
struct PersistedCandidate {
    relative_path: String,
    path: PathBuf,
    digest: String,
    size_bytes: u64,
    asset_fingerprint: String,
}

/// Load and validate an operation-scoped immutable candidate.
///
/// Once any candidate identity field is present, the operation has crossed the
/// freeze boundary. Recovery must either resume the exact same bytes and remote
/// asset identity or fail closed; it must never rebuild from a working DB that
/// may contain later local mutations.
fn load_persisted_candidate(
    ctx: &PublishContext<'_>,
    op: &OperationRow,
    payload: &OperationPayload,
) -> Result<Option<PersistedCandidate>, RemoteError> {
    let identity_claimed = payload.candidate_relative_path.is_some()
        || payload.candidate_size.is_some()
        || payload.candidate_sha256.is_some()
        || payload.candidate_assets_fingerprint.is_some()
        || op.candidate_db_digest.is_some();
    if !identity_claimed {
        return Ok(None);
    }

    let incomplete_identity = |field: &str| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "persisted candidate identity for operation {} is incomplete: missing {field}",
                op.operation_id
            ),
        )
    };
    let relative_path = payload
        .candidate_relative_path
        .clone()
        .ok_or_else(|| incomplete_identity("candidate_relative_path"))?;
    let expected_size = payload
        .candidate_size
        .ok_or_else(|| incomplete_identity("candidate_size"))?;
    let expected_digest = payload
        .candidate_sha256
        .clone()
        .ok_or_else(|| incomplete_identity("candidate_sha256"))?;
    let expected_asset_fingerprint = payload
        .candidate_assets_fingerprint
        .clone()
        .ok_or_else(|| incomplete_identity("candidate_assets_fingerprint"))?;
    let row_digest = op
        .candidate_db_digest
        .as_deref()
        .ok_or_else(|| incomplete_identity("candidate_db_digest"))?;
    if row_digest != expected_digest.as_str() {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "persisted candidate digest identity mismatch for operation {}",
                op.operation_id
            ),
        ));
    }

    let expected_relative_path = format!(".openkara/candidates/{}.sqlite", op.operation_id);
    if relative_path != expected_relative_path {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "persisted candidate path for operation {} is not operation-scoped: {}",
                op.operation_id, relative_path
            ),
        ));
    }

    let candidate_path = ctx.working_copy_root.join(&relative_path);
    let metadata = std::fs::metadata(&candidate_path).map_err(|error| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "persisted candidate for operation {} is missing or unreadable: {error}",
                op.operation_id
            ),
        )
    })?;
    if !metadata.is_file() {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "persisted candidate for operation {} is not a regular file",
                op.operation_id
            ),
        ));
    }
    if metadata.len() != expected_size {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "persisted candidate size mismatch for operation {}: expected {}, got {}",
                op.operation_id,
                expected_size,
                metadata.len()
            ),
        ));
    }

    let actual_digest = sha256_file(&candidate_path).map_err(|error| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "failed to hash persisted candidate for operation {}: {}",
                op.operation_id, error.message
            ),
        )
    })?;
    if actual_digest != expected_digest {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "persisted candidate digest mismatch for operation {}: expected {}, got {}",
                op.operation_id, expected_digest, actual_digest
            ),
        ));
    }

    verify_sqlite_integrity_pub(&candidate_path).map_err(|error| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "persisted candidate integrity check failed for operation {}: {}",
                op.operation_id, error.message
            ),
        )
    })?;

    Ok(Some(PersistedCandidate {
        relative_path,
        path: candidate_path,
        digest: expected_digest,
        size_bytes: expected_size,
        asset_fingerprint: expected_asset_fingerprint,
    }))
}

/// Managed top-level directories that hold repository assets. A database
/// path column must reference a file under one of these, otherwise the path
/// is rejected before it reaches the provider.
const ASSET_TOP_LEVEL_DIRS: &[&str] = &["media", "media-g", "stems", "artwork"];

/// A path column row extracted from the working database. Every non-empty
/// field is a managed-asset path that must be present remotely before the
/// manifest commits.
struct AssetPathRow {
    file_path: Option<String>,
    cdg_path: Option<String>,
    artwork_thumb_path: Option<String>,
    artwork_preview_path: Option<String>,
    vocals_path: Option<String>,
    accomp_path: Option<String>,
    drums_path: Option<String>,
    bass_path: Option<String>,
    other_path: Option<String>,
}

impl AssetPathRow {
    fn referenced_paths(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for field in [
            &self.file_path,
            &self.cdg_path,
            &self.artwork_thumb_path,
            &self.artwork_preview_path,
            &self.vocals_path,
            &self.accomp_path,
            &self.drums_path,
            &self.bass_path,
            &self.other_path,
        ] {
            if let Some(s) = field.as_deref() {
                if !s.is_empty() {
                    out.push(s);
                }
            }
        }
        out
    }
}

/// Validate that a database-referenced path is a relative, managed-asset path.
/// Rejects absolute paths, `..` traversal, empty segments, and paths outside
/// the managed top-level directories. This mirrors the safety check in the
/// integrity audit so a corrupted or hostile database row cannot cause the
/// executor to stat an arbitrary provider path.
fn validate_asset_path(path: &str) -> Result<(), RemoteError> {
    if path.is_empty() {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            "asset path is empty",
        ));
    }
    if std::path::Path::new(path).is_absolute() {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("asset path is absolute: {path}"),
        ));
    }
    let mut depth: i32 = 0;
    for (i, component) in path.split('/').enumerate() {
        if component.is_empty() {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!("asset path has empty segment: {path}"),
            ));
        }
        if component == "." || component == ".." {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!("asset path has traversal component: {path}"),
            ));
        }
        if i == 0 && !ASSET_TOP_LEVEL_DIRS.contains(&component) {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!("asset path outside managed dirs: {path}"),
            ));
        }
        depth += 1;
    }
    if depth < 2 {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("asset path refers to a managed root, not a file: {path}"),
        ));
    }
    Ok(())
}

/// Publication-protocol step 4: verify every asset referenced by the selected
/// database snapshot and return a deterministic fingerprint of remote metadata.
///
/// For a new freeze, `database_path` is the locked working DB and
/// `compare_local_size` is true, so provider size is checked against local asset
/// bytes. For a retry, `database_path` is the immutable candidate and local
/// files are ignored because they may already belong to a newer mutation.
/// Remote path, size, and revision are fingerprinted in both modes, allowing a
/// retry to detect a missing, truncated, or replaced remote object.
fn verify_referenced_assets(
    provider: &dyn RemoteProvider,
    working_copy_root: &Path,
    database_path: &Path,
    compare_local_size: bool,
) -> Result<String, RemoteError> {
    let conn = open_readonly(database_path).map_err(|e| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("failed to open DB for asset verification: {}", e.message),
        )
    })?;
    let mut remote_identities: BTreeMap<String, (Option<u64>, Option<String>)> = BTreeMap::new();

    let has_cdg_path = crate::cache::column_exists(&conn, "songs", "cdg_path")
        .map_err(|e| RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, e.to_string()))?;
    let has_artwork_thumb = crate::cache::column_exists(&conn, "songs", "artwork_thumb_path")
        .map_err(|e| RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, e.to_string()))?;
    let has_artwork_preview =
        crate::cache::column_exists(&conn, "songs", "artwork_preview_path")
            .map_err(|e| RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, e.to_string()))?;
    let has_stems = crate::cache::column_exists(&conn, "stems", "song_hash")
        .map_err(|e| RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, e.to_string()))?;

    let cdg_col = if has_cdg_path { "s.cdg_path" } else { "NULL" };
    let artwork_thumb_col = if has_artwork_thumb {
        "s.artwork_thumb_path"
    } else {
        "NULL"
    };
    let artwork_preview_col = if has_artwork_preview {
        "s.artwork_preview_path"
    } else {
        "NULL"
    };
    let stems_join = if has_stems {
        "LEFT JOIN stems st ON st.song_hash = s.hash"
    } else {
        "LEFT JOIN (SELECT NULL AS song_hash, NULL AS vocals_path, NULL AS accomp_path, NULL AS drums_path, NULL AS bass_path, NULL AS other_path) st ON 0=1"
    };
    let stems_cols = if has_stems {
        "st.vocals_path, st.accomp_path, st.drums_path, st.bass_path, st.other_path"
    } else {
        "NULL, NULL, NULL, NULL, NULL"
    };

    let sql = format!(
        "SELECT s.file_path, {cdg_col}, {artwork_thumb_col}, {artwork_preview_col}, {stems_cols}
         FROM songs s
         {stems_join}
         ORDER BY s.hash ASC"
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, e.to_string()))?;
    let rows: Vec<AssetPathRow> = stmt
        .query_map([], |row| {
            Ok(AssetPathRow {
                file_path: row.get(0)?,
                cdg_path: row.get(1)?,
                artwork_thumb_path: row.get(2)?,
                artwork_preview_path: row.get(3)?,
                vocals_path: row.get(4)?,
                accomp_path: row.get(5)?,
                drums_path: row.get(6)?,
                bass_path: row.get(7)?,
                other_path: row.get(8)?,
            })
        })
        .map_err(|e| RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, e.to_string()))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, e.to_string()))?;

    for row in &rows {
        for path in row.referenced_paths() {
            validate_asset_path(path)?;

            let remote_meta = provider
                .stat(path)
                .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
            let remote_meta = remote_meta.ok_or_else(|| {
                RemoteError::new(
                    RemoteErrorKind::RemoteIntegrityFailed,
                    format!("asset referenced by database is missing remotely: {path}"),
                )
            })?;

            // Local files are authoritative only before the first candidate
            // freeze. On retry, the same path may contain bytes from a newer
            // local mutation; the persisted remote fingerprint is authoritative.
            if compare_local_size {
                if let Some(remote_size) = remote_meta.size_bytes {
                    let local_path = working_copy_root.join(path);
                    if let Ok(local_meta) = std::fs::metadata(&local_path) {
                        let local_size = local_meta.len();
                        if local_size != remote_size {
                            return Err(RemoteError::new(
                                RemoteErrorKind::RemoteIntegrityFailed,
                                format!(
                                    "asset size mismatch for {path}: local {local_size}, remote {remote_size}"
                                ),
                            ));
                        }
                    }
                }
            }
            remote_identities.insert(
                path.to_owned(),
                (remote_meta.size_bytes, remote_meta.revision.clone()),
            );
        }
    }

    let encoded = serde_json::to_vec(&remote_identities).map_err(|error| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("failed to encode remote asset fingerprint: {error}"),
        )
    })?;
    let mut hasher = Sha256::new();
    hasher.update(encoded);
    Ok(crate::hash::hex_lower(hasher.finalize()))
}

/// Upload the immutable candidate database to the generation-specific remote
/// path. When the provider supports resumable upload and the candidate is
/// large enough, progress is bound to `expected_digest` so a restart cannot
/// append a different candidate's bytes into the same session.
fn upload_candidate_database(
    provider: &dyn RemoteProvider,
    working_copy_root: &Path,
    remote_relative_path: &str,
    bytes: &[u8],
    expected_digest: &str,
    expected_size: u64,
    operation_id: &str,
    control_db: &rusqlite::Connection,
) -> Result<(), RemoteError> {
    // Staging path used only for providers that read from the working copy
    // via `upload_file`. The immutable candidate under `.openkara/candidates/`
    // is the durable source of truth and is not deleted here.
    let local_staging = working_copy_root.join(remote_relative_path);
    if let Some(parent) = local_staging.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("failed to create staging dir: {e}"),
            )
        })?;
    }
    std::fs::write(&local_staging, bytes).map_err(|e| {
        RemoteError::new(
            RemoteErrorKind::NetworkUnavailable,
            format!("failed to write candidate staging file: {e}"),
        )
    })?;

    const RESUMABLE_UPLOAD_THRESHOLD: u64 = 8 * 1024 * 1024;
    let caps = provider.capabilities();
    if caps.resumable_upload && expected_size >= RESUMABLE_UPLOAD_THRESHOLD {
        let now = current_unix_time_ms();
        let existing = crate::remote::control_db::list_transfer_parts(control_db, operation_id)
            .unwrap_or_default()
            .into_iter()
            .find(|p| {
                p.relative_path == remote_relative_path
                    && p.direction == crate::remote::control_db::TransferDirection::Upload
            });
        if let Some(row) = existing {
            let digest_ok = row.expected_digest.as_deref() == Some(expected_digest);
            let size_ok = row.expected_size == Some(expected_size as i64);
            if !digest_ok || !size_ok {
                let _ = crate::remote::control_db::delete_transfer_parts(control_db, operation_id);
            }
        } else {
            let _ = crate::remote::control_db::upsert_transfer_part(
                control_db,
                &crate::remote::control_db::TransferPartRow {
                    operation_id: operation_id.to_owned(),
                    relative_path: remote_relative_path.to_owned(),
                    direction: crate::remote::control_db::TransferDirection::Upload,
                    expected_size: Some(expected_size as i64),
                    expected_digest: Some(expected_digest.to_owned()),
                    provider_revision: None,
                    provider_session_id: None,
                    transferred_bytes: 0,
                    state: "pending".to_owned(),
                    updated_at_ms: now,
                },
            );
        }
        provider.resumable_upload_bytes(remote_relative_path, bytes, operation_id, control_db)?;
    } else {
        provider
            .upload_file(remote_relative_path)
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
    }
    // Staging is not the durable candidate — safe to remove after a successful
    // upload attempt. The `.openkara/candidates/<op>.sqlite` file remains until
    // local completion is recorded.
    let _ = std::fs::remove_file(&local_staging);
    Ok(())
}

fn verify_remote_candidate_digest(
    provider: &dyn RemoteProvider,
    remote_relative_path: &str,
    expected_size: u64,
    expected_digest: &str,
    operation_id: &str,
    working_copy_root: &Path,
) -> Result<(), RemoteError> {
    let verify_path = working_copy_root.join(format!(
        ".openkara/candidates/{operation_id}.remote-verify.sqlite"
    ));
    if let Some(parent) = verify_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(&verify_path);
    provider
        .download_file(remote_relative_path, &verify_path)
        .map_err(|e| {
            let _ = std::fs::remove_file(&verify_path);
            RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!(
                    "failed to re-download candidate for digest verify: {}",
                    e.message
                ),
            )
        })?;
    let actual_size = std::fs::metadata(&verify_path)
        .map(|m| m.len())
        .map_err(|e| {
            let _ = std::fs::remove_file(&verify_path);
            RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!("failed to stat re-downloaded candidate: {e}"),
            )
        })?;
    if actual_size != expected_size {
        let _ = std::fs::remove_file(&verify_path);
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("remote candidate size mismatch: expected {expected_size}, got {actual_size}"),
        ));
    }
    let actual_digest = sha256_file(&verify_path).map_err(|e| {
        let _ = std::fs::remove_file(&verify_path);
        RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, e.message)
    })?;
    let _ = std::fs::remove_file(&verify_path);
    if actual_digest != expected_digest {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "remote candidate digest mismatch: expected {expected_digest}, got {actual_digest}"
            ),
        ));
    }
    Ok(())
}

/// Whether a remote generation at `expected+1` is this operation's accepted
/// CAS (post-CAS crash recovery), not some other publish by the same writer.
///
/// Requirements (all must hold):
/// - `writer_id` and `repository_id` match the local context
/// - when the manifest carries `operation_id`, it must equal this operation
/// - the operation still has a durable immutable candidate identity
/// - manifest `database_sha256` (and size when known) match that candidate
///
/// Ops whose candidate identity was cleared (e.g. after coalesce expanded the
/// payload) must not take this shortcut — they would otherwise mark a larger
/// change set complete against an older A-only remote DB.
fn is_accepted_commit_for_operation(
    manifest: &RepositoryManifest,
    ctx: &PublishContext<'_>,
    op: &OperationRow,
) -> bool {
    if manifest.writer_id != ctx.writer_id || manifest.repository_id != ctx.repository_id {
        return false;
    }
    if !manifest.operation_id.is_empty() && manifest.operation_id != op.operation_id {
        return false;
    }
    let payload = OperationPayload::from_json(&op.payload_json).unwrap_or_default();
    let candidate_sha = payload
        .candidate_sha256
        .as_deref()
        .or(op.candidate_db_digest.as_deref());
    let Some(candidate_sha) = candidate_sha else {
        // No durable candidate identity → cannot claim a prior CAS.
        return false;
    };
    if manifest.database_sha256 != candidate_sha {
        return false;
    }
    if let Some(size) = payload.candidate_size {
        if manifest.database_size_bytes != size {
            return false;
        }
    }
    true
}

fn transition_state(
    ctx: &PublishContext<'_>,
    op: &OperationRow,
    new_state: OperationState,
    now: i64,
    percent: u8,
    detail: &str,
) -> Result<(), RemoteError> {
    let mut payload = OperationPayload::from_json(&op.payload_json).unwrap_or_default();
    payload.percent = percent;
    payload.detail = Some(detail.to_owned());
    let mut updated = op.clone();
    updated.state = new_state;
    updated.payload_json = payload
        .to_json()
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
    updated.updated_at_ms = now;
    upsert_operation(ctx.control_db, &updated)
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))
}

/// Persist immutable candidate identity into the operation row so a crash
/// mid-upload can resume against the same bytes rather than a regenerated
/// candidate from a mutated working copy.
fn persist_candidate_identity(
    ctx: &PublishContext<'_>,
    op: &OperationRow,
    candidate_relative_path: &str,
    candidate_size: u64,
    candidate_sha256: &str,
    candidate_assets_fingerprint: &str,
    protocol_step: &str,
) -> Result<OperationRow, RemoteError> {
    let now = current_unix_time_ms();
    let mut payload = OperationPayload::from_json(&op.payload_json)
        .map_err(|error| RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, error.message))?;
    payload.candidate_relative_path = Some(candidate_relative_path.to_owned());
    payload.candidate_size = Some(candidate_size);
    payload.candidate_sha256 = Some(candidate_sha256.to_owned());
    payload.candidate_assets_fingerprint = Some(candidate_assets_fingerprint.to_owned());
    payload.protocol_step = Some(protocol_step.to_owned());
    let mut updated = op.clone();
    updated.candidate_db_digest = Some(candidate_sha256.to_owned());
    updated.payload_json = payload
        .to_json()
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
    updated.updated_at_ms = now;
    upsert_operation(ctx.control_db, &updated)
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
    Ok(updated)
}

/// Record a successful completion: mark the operation `completed` and update
/// repository state in **one** control-DB SQLite transaction.
///
/// Repository becomes `Clean` only when:
/// - no remaining non-terminal publish operations exist for the library, AND
/// - the current working DB digest still equals the committed candidate digest
///
/// Otherwise the repository stays `Dirty` with `active_operation_id` pointing
/// at the next durable-queue survivor. This prevents an automatic pull from
/// overwriting uncommitted local mutations after a partial CAS completion.
fn record_completed(
    ctx: &PublishContext<'_>,
    op: &OperationRow,
    outcome: &PublishOutcome,
) -> CommandResult<()> {
    let now = current_unix_time_ms();
    let working_db_path = ctx.working_copy_root.join("openkara.db");
    let working_digest = sha256_file(&working_db_path).ok();
    let working_matches_candidate = working_digest.as_ref() == Some(&outcome.candidate_db_digest);

    let tx = ctx
        .control_db
        .unchecked_transaction()
        .map_err(|e| database_error(format!("failed to begin completion transaction: {e}")))?;

    let mut updated = op.clone();
    updated.state = OperationState::Completed;
    updated.target_generation = Some(outcome.target_generation);
    updated.candidate_db_digest = Some(outcome.candidate_db_digest.clone());
    updated.error_code = None;
    updated.error_detail = None;
    updated.updated_at_ms = now;
    upsert_operation(&tx, &updated)?;

    let remaining: Vec<OperationRow> = list_operations_for_library(&tx, ctx.library_id)?
        .into_iter()
        .filter(|row| {
            row.operation_kind == OperationKind::Publish
                && !row.state.is_terminal()
                && row.operation_id != op.operation_id
        })
        .collect();

    let next_active = {
        let mut cas: Vec<&OperationRow> = remaining
            .iter()
            .filter(|row| {
                row.candidate_db_digest.is_some()
                    || OperationPayload::from_json(&row.payload_json)
                        .map(|p| {
                            p.candidate_sha256.is_some()
                                || p.candidate_relative_path.is_some()
                                || p.candidate_assets_fingerprint.is_some()
                                || matches!(
                                    p.protocol_step.as_deref(),
                                    Some("candidate_ready" | "candidate_uploaded")
                                )
                        })
                        .unwrap_or(false)
            })
            .collect();
        if !cas.is_empty() {
            cas.sort_by_key(|row| row.created_at_ms);
            Some(cas[0].operation_id.clone())
        } else {
            let mut others = remaining.clone();
            others.sort_by_key(|row| row.created_at_ms);
            others.first().map(|row| row.operation_id.clone())
        }
    };

    let (local_state, active_operation_id) = if !remaining.is_empty() {
        (LocalState::Dirty, next_active)
    } else if working_matches_candidate {
        (LocalState::Clean, None)
    } else {
        // Working DB diverged after freeze (e.g. late mutation not yet
        // projected). Keep Dirty so automatic pull cannot overwrite it.
        (LocalState::Dirty, None)
    };

    let repo_row = match get_repository_state(&tx, ctx.library_id)? {
        Some(mut row) => {
            row.committed_generation = outcome.target_generation;
            row.committed_manifest_revision = outcome.committed_manifest_revision.clone();
            row.local_base_generation = outcome.target_generation;
            row.local_db_digest = Some(outcome.candidate_db_digest.clone());
            row.local_state = local_state;
            row.active_operation_id = active_operation_id;
            row.last_success_at_ms = Some(now);
            row.last_error_code = None;
            row.updated_at_ms = now;
            if row.repository_id.is_none() {
                row.repository_id = Some(ctx.repository_id.to_owned());
            }
            if row.writer_id.is_none() {
                row.writer_id = Some(ctx.writer_id.to_owned());
            }
            row
        }
        None => RepositoryStateRow {
            library_id: ctx.library_id.to_owned(),
            committed_generation: outcome.target_generation,
            committed_manifest_revision: outcome.committed_manifest_revision.clone(),
            local_base_generation: outcome.target_generation,
            local_db_digest: Some(outcome.candidate_db_digest.clone()),
            local_state,
            active_operation_id,
            last_success_at_ms: Some(now),
            last_error_code: None,
            updated_at_ms: now,
            repository_id: Some(ctx.repository_id.to_owned()),
            writer_id: Some(ctx.writer_id.to_owned()),
        },
    };
    upsert_repository_state(&tx, &repo_row)?;

    schedule_gc_on_conn(&tx, ctx.library_id, outcome.target_generation, now)?;

    tx.commit()
        .map_err(|e| database_error(format!("failed to commit completion transaction: {e}")))?;

    // The immutable candidate is no longer needed once local completion is
    // durable. Best-effort removal — a leftover is cleaned by deferred GC.
    if let Ok(payload) = OperationPayload::from_json(&op.payload_json) {
        if let Some(rel) = payload.candidate_relative_path {
            let path = ctx.working_copy_root.join(rel);
            let _ = std::fs::remove_file(path);
        }
    }

    Ok(())
}

fn record_failure(
    ctx: &PublishContext<'_>,
    op: &OperationRow,
    error: &RemoteError,
) -> CommandResult<()> {
    let now = current_unix_time_ms();
    let (new_state, new_local_state) = match error.kind {
        RemoteErrorKind::RemoteConflict => (OperationState::Conflicted, LocalState::Conflicted),
        RemoteErrorKind::AuthenticationExpired => {
            (OperationState::Failed, LocalState::ReauthRequired)
        }
        RemoteErrorKind::ProviderCapabilityUnavailable
        | RemoteErrorKind::PermissionDenied
        | RemoteErrorKind::RemoteIntegrityFailed
        | RemoteErrorKind::DiskFull => (OperationState::Failed, LocalState::Dirty),
        RemoteErrorKind::NetworkUnavailable | RemoteErrorKind::RateLimited => {
            (OperationState::RetryWait, LocalState::Publishing)
        }
        RemoteErrorKind::OperationCancelled => (OperationState::Cancelled, LocalState::Dirty),
        // StaleRequest is a playback-only abort (the user skipped past the
        // song). It is not an operation failure — treat it as cancelled,
        // mirroring OperationCancelled (Dirty so an in-flight publish stays
        // dirty rather than being reported clean).
        RemoteErrorKind::StaleRequest => (OperationState::Cancelled, LocalState::Dirty),
    };

    let (error_code, error_detail) = error.to_db_columns();

    let mut updated = op.clone();
    updated.state = new_state;
    updated.error_code = error_code;
    updated.error_detail = error_detail;
    updated.next_attempt_at_ms = if error.retryable {
        Some(now + RETRY_BACKOFF_MS)
    } else {
        None
    };
    updated.updated_at_ms = now;
    upsert_operation(ctx.control_db, &updated)?;

    let repo_row = match get_repository_state(ctx.control_db, ctx.library_id)? {
        Some(mut row) => {
            row.local_state = new_local_state;
            row.active_operation_id = Some(op.operation_id.clone());
            row.last_error_code = Some(error.code.clone());
            row.updated_at_ms = now;
            row
        }
        None => RepositoryStateRow {
            library_id: ctx.library_id.to_owned(),
            committed_generation: 0,
            committed_manifest_revision: None,
            local_base_generation: 0,
            local_db_digest: None,
            local_state: new_local_state,
            active_operation_id: Some(op.operation_id.clone()),
            last_success_at_ms: None,
            last_error_code: Some(error.code.clone()),
            updated_at_ms: now,
            repository_id: None,
            writer_id: None,
        },
    };
    upsert_repository_state(ctx.control_db, &repo_row)?;

    Ok(())
}

/// Safety delay before GC may begin deleting previous generations. Retaining
/// only `committed_generation - 1` is not sufficient without a time-based
/// guarantee: a reader that started a pull just before the manifest advanced
/// needs wall-clock time to finish. 5 minutes is the minimum safety window.
const GC_SAFETY_DELAY_MS: i64 = 300_000;

fn schedule_gc_on_conn(
    connection: &Connection,
    library_id: &str,
    committed_generation: i64,
    now: i64,
) -> CommandResult<()> {
    let gc_op_id = format!("gc-{library_id}-{now}");
    let payload = OperationPayload {
        song_ids: Vec::new(),
        percent: 0,
        detail: Some(format!(
            "deferred GC after generation {committed_generation}"
        )),
        ..Default::default()
    };
    let row = OperationRow {
        operation_id: gc_op_id,
        library_id: library_id.to_owned(),
        operation_kind: OperationKind::Gc,
        // RetryWait with a future next_attempt_at_ms enforces the safety
        // delay: the executor skips operations whose next_attempt is in the
        // future, so GC cannot run immediately after publish.
        state: OperationState::RetryWait,
        expected_generation: Some(committed_generation),
        target_generation: None,
        source_db_digest: None,
        candidate_db_digest: None,
        payload_json: payload.to_json()?,
        attempt_count: 0,
        next_attempt_at_ms: Some(now + GC_SAFETY_DELAY_MS),
        error_code: None,
        error_detail: None,
        created_at_ms: now,
        updated_at_ms: now,
    };
    upsert_operation(connection, &row)?;
    Ok(())
}

/// Execute a deferred GC operation. Deletes database generations older than
/// `committed_generation - 1` from the remote repository. The previous
/// generation (`committed_generation - 1`) is retained as a rollback safety
/// net so a reader that started a pull just before the manifest advanced can
/// still complete.
///
/// Staging directories (`.openkara/staging/<operation-id>`) for completed
/// operations are also removed. Assets in `media/`, `stems/`, and `artwork/`
/// are NOT deleted — they are referenced by the committed database and
/// removed only when a future generation's database no longer references them
/// (a future reference-counting GC pass).
pub(crate) fn execute_gc(
    provider: &dyn RemoteProvider,
    control_db: &Connection,
    library_id: &str,
    operation_id: &str,
) -> CommandResult<()> {
    let op = get_operation(control_db, operation_id)?
        .ok_or_else(|| internal_error(format!("GC operation {operation_id} not found")))?;

    if !matches!(op.operation_kind, OperationKind::Gc) {
        return Err(internal_error(format!(
            "operation {operation_id} is not a GC operation"
        )));
    }

    // Terminal operations are not re-executed.
    if matches!(
        op.state,
        OperationState::Completed | OperationState::Failed | OperationState::Cancelled
    ) {
        return Ok(());
    }

    let now = current_unix_time_ms();
    let committed_generation = op.expected_generation.unwrap_or(0);

    // The previous generation is retained as a rollback safety net. Generations
    // older than that are safe to delete because no reader can be mid-pull on
    // them (the manifest advanced at least two generations ago).
    let retain_floor = committed_generation.saturating_sub(1);

    let manifest = read_manifest(provider)?;
    if let Some(ref m) = manifest {
        if m.generation < committed_generation {
            // The remote has not yet advanced to the generation this GC was
            // scheduled for. Skip — a future GC will clean up after the
            // manifest advances.
            tracing::info!(
                "skipping GC for library {}: manifest generation {} < expected {}",
                library_id,
                m.generation,
                committed_generation
            );
            let mut updated = op.clone();
            updated.state = OperationState::Completed;
            updated.updated_at_ms = now;
            upsert_operation(control_db, &updated)?;
            return Ok(());
        }
    }

    // Delete old database generations: 1..=retain_floor-1.
    // Generation 0 is reserved. Both the legacy `<generation>.sqlite` object
    // and the operation-scoped `<generation>/` directory are attempted so
    // repositories created before and after this hardening remain collectible.
    //
    // Never delete the global `.openkara/staging` root here. Another device can
    // have an in-flight WebDAV staged upload under that prefix; deleting the
    // shared root would let a local GC corrupt a concurrent writer before CAS.
    let mut transient_failures = 0;
    let mut permanent_failures = 0;
    for gen in 1..retain_floor {
        for db_path in [
            database_path_for_generation(gen),
            database_directory_for_generation(gen),
        ] {
            match provider.delete_path(&db_path) {
                Ok(()) => {
                    tracing::debug!("GC deleted old database generation {} at {}", gen, db_path);
                }
                Err(e) => {
                    // Only explicit missing-object (404 / not found) is
                    // idempotent success. Permission/auth/capability errors
                    // must keep GC non-completed.
                    let msg = e.message.to_ascii_lowercase();
                    let is_not_found = msg.contains("not found")
                        || msg.contains("404")
                        || msg.contains("does not exist")
                        || msg.contains("was not found");
                    if is_not_found {
                        tracing::debug!(
                            "GC confirmed absence of old database generation {} at {}: {}",
                            gen,
                            db_path,
                            e.message
                        );
                    } else if e.retryable {
                        tracing::warn!(
                            "GC transient failure deleting {} (will retry): {}",
                            db_path,
                            e.message
                        );
                        transient_failures += 1;
                    } else {
                        tracing::warn!(
                            "GC permanent failure deleting {} (will not complete): {}",
                            db_path,
                            e.message
                        );
                        permanent_failures += 1;
                    }
                }
            }
        }
    }

    if permanent_failures > 0 {
        let mut updated = op.clone();
        updated.state = OperationState::Failed;
        updated.error_code = Some("gc_delete_failed".to_owned());
        updated.error_detail = Some(format!(
            "{permanent_failures} generation delete(s) failed permanently"
        ));
        updated.updated_at_ms = now;
        upsert_operation(control_db, &updated)?;
        return Ok(());
    }

    if transient_failures > 0 {
        // Leave the GC operation retryable — do NOT mark it Completed.
        let mut updated = op.clone();
        updated.state = OperationState::RetryWait;
        updated.next_attempt_at_ms = Some(now + GC_RETRY_BACKOFF_MS);
        updated.updated_at_ms = now;
        upsert_operation(control_db, &updated)?;
        return Ok(());
    }

    // Mark the GC operation as completed only after every target has been
    // successfully deleted or confirmed absent (404).
    let mut updated = op.clone();
    updated.state = OperationState::Completed;
    updated.updated_at_ms = now;
    upsert_operation(control_db, &updated)?;

    Ok(())
}

const RETRY_BACKOFF_MS: i64 = 30_000;

const GC_RETRY_BACKOFF_MS: i64 = 60_000;

fn current_unix_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(crate) fn generate_writer_id() -> String {
    Uuid::new_v4().to_string()
}

pub(crate) fn generate_repository_id() -> String {
    Uuid::new_v4().to_string()
}

/// Action: keep the local pending changes as a new generation after rebasing
/// onto the winning remote generation.
///
/// Automatic rebase is allowed ONLY when local and remote changes touch
/// disjoint song IDs AND repository-global settings are unchanged. The
/// disjoint-song check compares the set of song hashes that differ between
/// the local working DB and the remote conflict-candidate DB.
///
/// For PR#4, "repository-global settings unchanged" is approximated by
/// checking that the non-song tables (settings) have identical row counts
/// and content hashes between the two DBs. A full row-by-row merge is NOT
/// attempted.
pub(crate) fn conflict_keep_local_as_new_generation(
    ctx: &PublishContext<'_>,
    operation_id: &str,
    conflict_candidate_db: &Path,
) -> CommandResult<()> {
    let op = get_operation(ctx.control_db, operation_id)?
        .ok_or_else(|| internal_error(format!("operation {operation_id} not found")))?;

    if op.state != OperationState::Conflicted {
        return Err(internal_error(
            "conflict keep-local requires a conflicted operation",
        ));
    }

    let remote_manifest = read_manifest(ctx.provider)?
        .ok_or_else(|| internal_error("no remote manifest found during conflict resolution"))?;

    let local_db_path = ctx.working_copy_root.join("openkara.db");
    let local_songs = song_hashes_in_db(&local_db_path)?;
    let remote_songs = song_hashes_in_db(conflict_candidate_db)?;

    let local_changed: std::collections::HashSet<String> = op
        .payload_json
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let _ = local_changed;

    let remote_only: Vec<String> = remote_songs
        .iter()
        .filter(|s| !local_songs.contains(*s))
        .cloned()
        .collect();
    let local_only: Vec<String> = local_songs
        .iter()
        .filter(|s| !remote_songs.contains(*s))
        .cloned()
        .collect();

    // If the remote DB contains songs not in the local DB (or vice versa) and
    // the local operation's affected songs overlap with the remote-only set,
    // the changes are NOT disjoint — require explicit user choice.
    let affected_songs = OperationPayload::from_json(&op.payload_json)
        .map(|p| p.song_ids)
        .unwrap_or_default();
    let overlap = affected_songs
        .iter()
        .any(|s| remote_only.contains(s) || local_only.contains(s));

    let settings_match = settings_tables_match(&local_db_path, conflict_candidate_db)?;

    if overlap || !settings_match {
        return Err(internal_error(
            "automatic rebase rejected: local and remote changes overlap or \
             repository-global settings differ; explicit user choice required",
        ));
    }

    let now = current_unix_time_ms();
    let mut updated = op.clone();
    updated.expected_generation = Some(remote_manifest.generation);
    updated.state = OperationState::Pending;
    updated.error_code = None;
    updated.error_detail = None;
    updated.updated_at_ms = now;
    upsert_operation(ctx.control_db, &updated)?;

    execute_publish(ctx, operation_id)
}

pub(crate) fn conflict_use_remote(
    ctx: &PublishContext<'_>,
    operation_id: &str,
    conflict_candidate_db: &Path,
) -> CommandResult<()> {
    let op = get_operation(ctx.control_db, operation_id)?
        .ok_or_else(|| internal_error(format!("operation {operation_id} not found")))?;

    if op.state != OperationState::Conflicted {
        return Err(internal_error(
            "conflict use-remote requires a conflicted operation",
        ));
    }

    let now = current_unix_time_ms();

    let mut updated = op.clone();
    updated.state = OperationState::Cancelled;
    updated.updated_at_ms = now;
    upsert_operation(ctx.control_db, &updated)?;

    let working_db = ctx.working_copy_root.join("openkara.db");
    std::fs::copy(conflict_candidate_db, &working_db)
        .map_err(|e| internal_error(format!("failed to activate remote database: {e}")))?;

    let new_digest = sha256_file(&working_db)?;
    let remote_manifest = read_manifest(ctx.provider)?
        .ok_or_else(|| internal_error("no remote manifest found during conflict resolution"))?;

    let repo_row = match get_repository_state(ctx.control_db, ctx.library_id)? {
        Some(mut row) => {
            row.committed_generation = remote_manifest.generation;
            row.committed_manifest_revision = None;
            row.local_base_generation = remote_manifest.generation;
            row.local_db_digest = Some(new_digest);
            row.local_state = LocalState::Clean;
            row.active_operation_id = None;
            row.last_error_code = None;
            row.updated_at_ms = now;
            row
        }
        None => RepositoryStateRow {
            library_id: ctx.library_id.to_owned(),
            committed_generation: remote_manifest.generation,
            committed_manifest_revision: None,
            local_base_generation: remote_manifest.generation,
            local_db_digest: Some(new_digest),
            local_state: LocalState::Clean,
            active_operation_id: None,
            last_success_at_ms: Some(now),
            last_error_code: None,
            updated_at_ms: now,
            repository_id: None,
            writer_id: None,
        },
    };
    upsert_repository_state(ctx.control_db, &repo_row)?;
    Ok(())
}

/// Pull the winning remote manifest + database to a conflict candidate path
/// (NOT the active working DB). Uses the provider's download_file.
pub(crate) fn pull_conflict_candidate(
    provider: &dyn RemoteProvider,
    destination: &Path,
) -> CommandResult<RepositoryManifest> {
    let manifest = read_manifest(provider)?
        .ok_or_else(|| internal_error("no remote manifest found to pull conflict candidate"))?;
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| internal_error(format!("failed to create conflict candidate dir: {e}")))?;
    }
    provider.download_file(&manifest.database_path, destination)?;
    verify_sqlite_integrity_pub(destination)?;
    Ok(manifest)
}

fn song_hashes_in_db(db_path: &Path) -> CommandResult<std::collections::HashSet<String>> {
    if !db_path.exists() {
        return Ok(std::collections::HashSet::new());
    }
    let conn = open_readonly(db_path)?;
    let mut stmt = conn
        .prepare("SELECT hash FROM songs;")
        .map_err(|e| internal_error(format!("failed to prepare song query: {e}")))?;
    let hashes = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| internal_error(format!("failed to query songs: {e}")))?
        .collect::<rusqlite::Result<std::collections::HashSet<String>>>()
        .map_err(|e| internal_error(format!("failed to collect song hashes: {e}")))?;
    Ok(hashes)
}

fn settings_tables_match(local_db: &Path, remote_db: &Path) -> CommandResult<bool> {
    let local_hash = settings_table_hash(local_db)?;
    let remote_hash = settings_table_hash(remote_db)?;
    Ok(local_hash == remote_hash)
}

fn settings_table_hash(db_path: &Path) -> CommandResult<Option<String>> {
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = open_readonly(db_path)?;
    let exists: bool = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='settings' LIMIT 1;")
        .map_err(|e| internal_error(format!("settings check prepare failed: {e}")))?
        .exists::<&[&dyn rusqlite::ToSql]>(&[])
        .map_err(|e| internal_error(format!("settings check query failed: {e}")))?;
    if !exists {
        return Ok(None);
    }
    let mut stmt = conn
        .prepare("SELECT key, value FROM settings ORDER BY key;")
        .map_err(|e| internal_error(format!("settings query prepare failed: {e}")))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(format!(
                "{}={}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1).unwrap_or_default()
            ))
        })
        .map_err(|e| internal_error(format!("settings query failed: {e}")))?
        .collect::<rusqlite::Result<Vec<String>>>()
        .map_err(|e| internal_error(format!("settings collect failed: {e}")))?;
    let joined = rows.join("\n");
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(joined.as_bytes());
    Ok(Some(crate::hash::hex_lower(hasher.finalize())))
}

/// Open a SQLite database read-only.
fn open_readonly(path: &Path) -> CommandResult<Connection> {
    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    Connection::open_with_flags(path, flags)
        .map_err(|e| internal_error(format!("failed to open DB read-only: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::error::CommandResult;
    use crate::remote::control_db::{
        open_control_db, upsert_operation, OperationKind, OperationPayload, OperationRow,
        OperationState,
    };
    use crate::remote::errors::{
        RemoteError, RemoteErrorKind, RemoteObjectMetadata, RemoteProviderCapabilities,
    };
    use crate::remote::manifest::MANIFEST_PATH;
    use crate::remote::provider::{ConditionalSource, RemoteProvider};
    use rusqlite::Connection;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    struct FakeProvider {
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        revisions: Arc<Mutex<HashMap<String, String>>>,
        no_cas: bool,
        working_copy_root: Option<PathBuf>,
        mutate_asset_on_candidate_upload: Option<(String, Vec<u8>, String)>,
    }

    impl FakeProvider {
        fn new() -> Self {
            Self {
                files: Arc::new(Mutex::new(HashMap::new())),
                revisions: Arc::new(Mutex::new(HashMap::new())),
                no_cas: false,
                working_copy_root: None,
                mutate_asset_on_candidate_upload: None,
            }
        }

        fn with_no_cas() -> Self {
            Self {
                files: Arc::new(Mutex::new(HashMap::new())),
                revisions: Arc::new(Mutex::new(HashMap::new())),
                no_cas: true,
                working_copy_root: None,
                mutate_asset_on_candidate_upload: None,
            }
        }

        fn with_working_copy_root(mut self, root: PathBuf) -> Self {
            self.working_copy_root = Some(root);
            self
        }

        fn with_asset_mutation_on_candidate_upload(
            mut self,
            path: &str,
            bytes: Vec<u8>,
            revision: &str,
        ) -> Self {
            self.mutate_asset_on_candidate_upload =
                Some((path.to_owned(), bytes, revision.to_owned()));
            self
        }

        fn store(&self, path: &str, bytes: Vec<u8>, revision: &str) {
            self.revisions
                .lock()
                .unwrap()
                .insert(path.to_owned(), revision.to_owned());
            self.files.lock().unwrap().insert(path.to_owned(), bytes);
        }
    }

    impl RemoteProvider for FakeProvider {
        fn capabilities(&self) -> RemoteProviderCapabilities {
            RemoteProviderCapabilities {
                conditional_replace: !self.no_cas,
                resumable_upload: false,
                range_download: true,
                revision_metadata: true,
                server_side_move: false,
            }
        }

        fn stat(&self, path: &str) -> CommandResult<Option<RemoteObjectMetadata>> {
            let files = self.files.lock().unwrap();
            let revisions = self.revisions.lock().unwrap();
            if files.contains_key(path) {
                Ok(Some(RemoteObjectMetadata {
                    size_bytes: Some(files.get(path).unwrap().len() as u64),
                    revision: revisions.get(path).cloned(),
                }))
            } else {
                Ok(None)
            }
        }

        fn get_revision(&self, path: &str) -> CommandResult<Option<String>> {
            Ok(self.revisions.lock().unwrap().get(path).cloned())
        }

        fn download_file(&self, path: &str, dest: &Path) -> CommandResult<()> {
            let bytes = self
                .files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| internal_error(format!("fake provider: {path} not found")))?;
            std::fs::write(dest, bytes).map_err(|e| internal_error(e.to_string()))
        }

        fn upload_file(&self, path: &str) -> CommandResult<()> {
            if let Some(ref root) = self.working_copy_root {
                let local_path = root.join(path);
                if local_path.exists() {
                    let bytes = std::fs::read(&local_path)
                        .map_err(|e| internal_error(format!("fake upload_file read: {e}")))?;
                    let rev = format!(
                        "rev-{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos()
                    );
                    let size = bytes.len() as u64;
                    self.files.lock().unwrap().insert(path.to_owned(), bytes);
                    self.revisions
                        .lock()
                        .unwrap()
                        .insert(path.to_owned(), rev.clone());
                    let _ = size;
                    if path.starts_with(".openkara/databases/") {
                        if let Some((asset_path, bytes, revision)) =
                            &self.mutate_asset_on_candidate_upload
                        {
                            self.store(asset_path, bytes.clone(), revision);
                        }
                    }
                }
            }
            Ok(())
        }

        fn delete_path(&self, path: &str) -> CommandResult<()> {
            self.files.lock().unwrap().remove(path);
            self.revisions.lock().unwrap().remove(path);
            Ok(())
        }

        fn conditional_replace(
            &self,
            path: &str,
            source: ConditionalSource,
            expected_revision: Option<&str>,
        ) -> Result<RemoteObjectMetadata, RemoteError> {
            if self.no_cas {
                return Err(RemoteError::from_kind(
                    RemoteErrorKind::ProviderCapabilityUnavailable,
                ));
            }

            let bytes = source
                .read_bytes()
                .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;

            let mut revisions = self.revisions.lock().unwrap();
            let current_rev = revisions.get(path).cloned();

            match expected_revision {
                Some(expected) => {
                    if current_rev.as_deref() != Some(expected) {
                        return Err(RemoteError::new(
                            RemoteErrorKind::RemoteConflict,
                            format!(
                                "CAS mismatch: expected rev {expected}, found {:?}",
                                current_rev
                            ),
                        ));
                    }
                }
                None => {
                    if current_rev.is_some() {
                        return Err(RemoteError::new(
                            RemoteErrorKind::RemoteConflict,
                            "conditional-create failed: object already exists",
                        ));
                    }
                }
            }

            let new_rev = format!(
                "rev-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            let size = bytes.len() as u64;
            self.files.lock().unwrap().insert(path.to_owned(), bytes);
            revisions.insert(path.to_owned(), new_rev.clone());

            Ok(RemoteObjectMetadata {
                size_bytes: Some(size),
                revision: Some(new_rev),
            })
        }

        fn initialize_or_sync(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }

        fn get_file_size(&self, path: &str) -> CommandResult<Option<u64>> {
            Ok(self.files.lock().unwrap().get(path).map(|b| b.len() as u64))
        }

        fn refresh_existing(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }
    }

    fn make_valid_db(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS songs (
                hash TEXT PRIMARY KEY,
                file_path TEXT,
                cdg_path TEXT,
                artwork_thumb_path TEXT,
                artwork_preview_path TEXT
             );
             CREATE TABLE IF NOT EXISTS stems (
                song_hash TEXT PRIMARY KEY,
                vocals_path TEXT,
                accomp_path TEXT,
                drums_path TEXT,
                bass_path TEXT,
                other_path TEXT
             );
             CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT);
             INSERT OR IGNORE INTO songs (hash) VALUES ('song-1');
             INSERT OR IGNORE INTO settings (key, value) VALUES ('version', '1');",
        )
        .unwrap();
    }

    fn fresh_control_db() -> (TempDir, Connection) {
        let dir = TempDir::new().unwrap();
        let conn = open_control_db(&dir.path().join("remote-state.db")).unwrap();
        (dir, conn)
    }

    fn make_context<'a>(
        control_db: &'a Connection,
        provider: &'a dyn RemoteProvider,
        working_copy_root: &'a Path,
        library_id: &'a str,
        repository_id: &'a str,
        writer_id: &'a str,
    ) -> PublishContext<'a> {
        PublishContext {
            control_db,
            provider,
            working_copy_root,
            library_id,
            writer_id,
            repository_id,
        }
    }

    fn make_pending_op(conn: &Connection, library_id: &str, expected_gen: i64) -> String {
        let op_id = format!("publish-test-{}", expected_gen);
        let now = crate::remote::types::current_unix_time_ms();
        let payload = OperationPayload {
            song_ids: vec!["song-1".to_owned()],
            percent: 0,
            detail: None,
            ..Default::default()
        };
        let row = OperationRow {
            operation_id: op_id.clone(),
            library_id: library_id.to_owned(),
            operation_kind: OperationKind::Publish,
            state: OperationState::Pending,
            expected_generation: Some(expected_gen),
            target_generation: None,
            source_db_digest: None,
            candidate_db_digest: None,
            payload_json: payload.to_json().unwrap(),
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        upsert_operation(conn, &row).unwrap();
        op_id
    }

    fn persist_test_candidate(
        conn: &Connection,
        provider: &dyn RemoteProvider,
        working_root: &Path,
        operation_id: &str,
    ) -> PathBuf {
        let relative = format!(".openkara/candidates/{operation_id}.sqlite");
        let candidate = working_root.join(&relative);
        std::fs::create_dir_all(candidate.parent().unwrap()).unwrap();
        std::fs::copy(working_root.join("openkara.db"), &candidate).unwrap();
        let digest = sha256_file(&candidate).unwrap();
        let size = std::fs::metadata(&candidate).unwrap().len();
        let asset_fingerprint =
            verify_referenced_assets(provider, working_root, &candidate, true).unwrap();

        let mut op = get_operation(conn, operation_id).unwrap().unwrap();
        let mut payload = OperationPayload::from_json(&op.payload_json).unwrap();
        payload.candidate_relative_path = Some(relative);
        payload.candidate_size = Some(size);
        payload.candidate_sha256 = Some(digest.clone());
        payload.candidate_assets_fingerprint = Some(asset_fingerprint);
        payload.protocol_step = Some("candidate_ready".to_owned());
        op.candidate_db_digest = Some(digest);
        op.payload_json = payload.to_json().unwrap();
        op.state = OperationState::RetryWait;
        upsert_operation(conn, &op).unwrap();
        candidate
    }

    #[test]
    fn executor_publishes_first_generation_from_empty_repository() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        let op_id = make_pending_op(&conn, "lib-1", 0);

        let ctx = make_context(
            &conn,
            &provider,
            &working_root,
            "lib-1",
            "repo-uuid-1",
            "writer-uuid-1",
        );

        execute_publish(&ctx, &op_id).expect("publish should succeed");

        let manifest_bytes = provider.files.lock().unwrap().get(MANIFEST_PATH).cloned();
        assert!(manifest_bytes.is_some(), "manifest should be written");
        let manifest: RepositoryManifest =
            serde_json::from_slice(&manifest_bytes.unwrap()).unwrap();
        assert_eq!(manifest.generation, 1);
        assert_eq!(manifest.repository_id, "repo-uuid-1");
        assert_eq!(manifest.writer_id, "writer-uuid-1");

        let op = crate::remote::control_db::get_operation(&conn, &op_id)
            .unwrap()
            .unwrap();
        assert_eq!(op.state, OperationState::Completed);
        assert_eq!(op.target_generation, Some(1));

        let state = crate::remote::control_db::get_repository_state(&conn, "lib-1")
            .unwrap()
            .unwrap();
        assert_eq!(state.committed_generation, 1);
        assert_eq!(state.local_state, LocalState::Clean);
    }

    #[test]
    fn executor_advances_generation_on_subsequent_publish() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());

        let op1 = make_pending_op(&conn, "lib-1", 0);
        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        execute_publish(&ctx, &op1).expect("first publish");

        let op2 = make_pending_op(&conn, "lib-1", 1);
        execute_publish(&ctx, &op2).expect("second publish");

        let manifest_bytes = provider.files.lock().unwrap().get(MANIFEST_PATH).cloned();
        let manifest: RepositoryManifest =
            serde_json::from_slice(&manifest_bytes.unwrap()).unwrap();
        assert_eq!(manifest.generation, 2);
    }

    #[test]
    fn executor_fails_closed_for_provider_without_cas() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::with_no_cas();
        let op_id = make_pending_op(&conn, "lib-1", 0);

        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        let result = execute_publish(&ctx, &op_id);

        assert!(result.is_err());

        let op = crate::remote::control_db::get_operation(&conn, &op_id)
            .unwrap()
            .unwrap();
        assert_eq!(op.state, OperationState::Failed);
        assert_eq!(
            op.error_code.as_deref(),
            Some("provider_capability_unavailable")
        );
    }

    #[test]
    fn executor_detects_cas_conflict_on_stale_generation() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());

        let manifest = RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation: 2,
            database_path: ".openkara/databases/2.sqlite".to_owned(),
            database_size_bytes: 100,
            database_sha256: "abc".to_owned(),
            committed_at_ms: 1000,
            writer_id: "other-device".to_owned(),
            operation_id: "op-test".to_owned(),
        };
        provider.store(
            MANIFEST_PATH,
            manifest.to_json().unwrap().into_bytes(),
            "rev-gen-2",
        );

        let op_id = make_pending_op(&conn, "lib-1", 0);
        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        let result = execute_publish(&ctx, &op_id);

        assert!(result.is_err());

        let op = crate::remote::control_db::get_operation(&conn, &op_id)
            .unwrap()
            .unwrap();
        assert_eq!(op.state, OperationState::Conflicted);
        assert_eq!(op.error_code.as_deref(), Some("remote_conflict"));

        let state = crate::remote::control_db::get_repository_state(&conn, "lib-1")
            .unwrap()
            .unwrap();
        assert_eq!(state.local_state, LocalState::Conflicted);
    }

    #[test]
    fn executor_does_not_reexecute_terminal_operations() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        let op_id = make_pending_op(&conn, "lib-1", 0);

        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");

        execute_publish(&ctx, &op_id).expect("first publish");

        let files_before = provider.files.lock().unwrap().len();

        execute_publish(&ctx, &op_id).expect("second call is no-op");

        let files_after = provider.files.lock().unwrap().len();
        assert_eq!(files_before, files_after);
    }

    #[test]
    fn executor_verifies_committed_manifest() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        let op_id = make_pending_op(&conn, "lib-1", 0);

        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        execute_publish(&ctx, &op_id).expect("publish should succeed");

        let manifest_bytes = provider.files.lock().unwrap().get(MANIFEST_PATH).cloned();
        let manifest: RepositoryManifest =
            serde_json::from_slice(&manifest_bytes.unwrap()).unwrap();
        assert!(
            manifest.database_path.starts_with(".openkara/databases/1/"),
            "generation DB must be operation-scoped"
        );
        assert_ne!(manifest.database_path, ".openkara/databases/1.sqlite");

        let db_bytes = provider
            .files
            .lock()
            .unwrap()
            .get(&manifest.database_path)
            .cloned();
        assert!(db_bytes.is_some(), "candidate database should be uploaded");

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(db_bytes.unwrap());
        let computed = crate::hash::hex_lower(hasher.finalize());
        assert_eq!(manifest.database_sha256, computed);
    }

    #[test]
    fn executor_schedules_gc_operation() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        let op_id = make_pending_op(&conn, "lib-1", 0);

        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        execute_publish(&ctx, &op_id).expect("publish should succeed");

        let all_ops = crate::remote::control_db::list_operations_in_states(
            &conn,
            &[OperationState::Pending, OperationState::RetryWait],
        )
        .unwrap();
        let gc_ops: Vec<_> = all_ops
            .iter()
            .filter(|op| op.operation_kind == OperationKind::Gc)
            .collect();
        assert_eq!(gc_ops.len(), 1, "one GC operation should be scheduled");
        assert_eq!(gc_ops[0].expected_generation, Some(1));
        assert_eq!(
            gc_ops[0].state,
            OperationState::RetryWait,
            "GC must start in RetryWait so the safety delay is enforced"
        );
        assert!(
            gc_ops[0]
                .next_attempt_at_ms
                .is_some_and(|t| t > current_unix_time_ms()),
            "GC safety delay must put next_attempt_at_ms in the future"
        );
    }

    #[test]
    fn executor_persists_repository_and_writer_ids() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        let op_id = make_pending_op(&conn, "lib-1", 0);

        let ctx = make_context(
            &conn,
            &provider,
            &working_root,
            "lib-1",
            "repo-uuid-x",
            "writer-uuid-y",
        );
        execute_publish(&ctx, &op_id).expect("publish should succeed");

        let state = crate::remote::control_db::get_repository_state(&conn, "lib-1")
            .unwrap()
            .unwrap();
        assert_eq!(state.repository_id.as_deref(), Some("repo-uuid-x"));
        assert_eq!(state.writer_id.as_deref(), Some("writer-uuid-y"));
    }

    fn insert_song_with_media(
        db_path: &Path,
        song_hash: &str,
        media_rel: &str,
        working_root: &Path,
    ) {
        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO songs (hash, file_path) VALUES (?1, ?2)",
            rusqlite::params![song_hash, media_rel],
        )
        .unwrap();
        let local = working_root.join(media_rel);
        std::fs::create_dir_all(local.parent().unwrap()).unwrap();
        std::fs::write(&local, b"media-bytes").unwrap();
    }

    fn insert_song_with_stems(
        db_path: &Path,
        song_hash: &str,
        stem_files: &[&str],
        working_root: &Path,
    ) {
        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO songs (hash) VALUES (?1)",
            [song_hash],
        )
        .unwrap();
        let vocals = format!("stems/{song_hash}/vocals.ogg");
        let accomp = format!("stems/{song_hash}/accompaniment.ogg");
        conn.execute(
            "INSERT OR REPLACE INTO stems (song_hash, vocals_path, accomp_path) VALUES (?1, ?2, ?3)",
            rusqlite::params![song_hash, vocals, accomp],
        )
        .unwrap();
        for rel in stem_files {
            let local = working_root.join(rel);
            std::fs::create_dir_all(local.parent().unwrap()).unwrap();
            std::fs::write(&local, b"stem-bytes").unwrap();
        }
    }

    fn insert_song_with_artwork(
        db_path: &Path,
        song_hash: &str,
        thumb_rel: &str,
        preview_rel: &str,
        working_root: &Path,
    ) {
        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO songs (hash, artwork_thumb_path, artwork_preview_path) VALUES (?1, ?2, ?3)",
            rusqlite::params![song_hash, thumb_rel, preview_rel],
        )
        .unwrap();
        for rel in [thumb_rel, preview_rel] {
            let local = working_root.join(rel);
            std::fs::create_dir_all(local.parent().unwrap()).unwrap();
            std::fs::write(&local, b"artwork-bytes").unwrap();
        }
    }

    #[test]
    fn executor_fails_when_referenced_media_asset_is_missing() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        insert_song_with_media(
            &working_root.join("openkara.db"),
            "song-1",
            "media/song-1.mp3",
            &working_root,
        );

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        let op_id = make_pending_op(&conn, "lib-1", 0);
        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");

        let result = execute_publish(&ctx, &op_id);
        assert!(
            result.is_err(),
            "publish must fail when an asset is missing"
        );

        let op = crate::remote::control_db::get_operation(&conn, &op_id)
            .unwrap()
            .unwrap();
        assert_eq!(op.state, OperationState::Failed);
        assert_eq!(
            op.error_code.as_deref(),
            Some("remote_integrity_failed"),
            "missing asset must produce remote_integrity_failed"
        );

        let manifest = provider.files.lock().unwrap().get(MANIFEST_PATH).cloned();
        assert!(
            manifest.is_none(),
            "manifest must not be committed when an asset is missing"
        );
    }

    #[test]
    fn executor_fails_when_referenced_asset_size_mismatches() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        insert_song_with_media(
            &working_root.join("openkara.db"),
            "song-1",
            "media/song-1.mp3",
            &working_root,
        );

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        provider.store("media/song-1.mp3", b"trunc".to_vec(), "rev-1");

        let op_id = make_pending_op(&conn, "lib-1", 0);
        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        let result = execute_publish(&ctx, &op_id);
        assert!(
            result.is_err(),
            "publish must fail when remote asset size mismatches"
        );

        let op = crate::remote::control_db::get_operation(&conn, &op_id)
            .unwrap()
            .unwrap();
        assert_eq!(op.state, OperationState::Failed);
        assert_eq!(op.error_code.as_deref(), Some("remote_integrity_failed"));

        let manifest = provider.files.lock().unwrap().get(MANIFEST_PATH).cloned();
        assert!(
            manifest.is_none(),
            "manifest must not be committed on size mismatch"
        );
    }

    #[test]
    fn executor_succeeds_when_all_referenced_assets_present() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        insert_song_with_media(
            &working_root.join("openkara.db"),
            "song-1",
            "media/song-1.mp3",
            &working_root,
        );

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        provider
            .upload_file("media/song-1.mp3")
            .expect("fake upload should succeed");

        let op_id = make_pending_op(&conn, "lib-1", 0);
        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        execute_publish(&ctx, &op_id).expect("publish should succeed when all assets are present");

        let manifest = provider.files.lock().unwrap().get(MANIFEST_PATH).cloned();
        assert!(manifest.is_some(), "manifest should be committed");
    }

    #[test]
    fn executor_retry_uses_candidate_not_newer_working_db() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));
        insert_song_with_media(
            &working_root.join("openkara.db"),
            "song-1",
            "media/song-1.mp3",
            &working_root,
        );

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        provider.upload_file("media/song-1.mp3").unwrap();
        let op_id = make_pending_op(&conn, "lib-1", 0);
        persist_test_candidate(&conn, &provider, &working_root, &op_id);

        insert_song_with_media(
            &working_root.join("openkara.db"),
            "song-2",
            "media/song-2.mp3",
            &working_root,
        );

        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        execute_publish(&ctx, &op_id).expect("retry should publish the immutable candidate");

        let manifest_bytes = provider.files.lock().unwrap().get(MANIFEST_PATH).cloned();
        let manifest: RepositoryManifest =
            serde_json::from_slice(&manifest_bytes.expect("manifest committed")).unwrap();
        let uploaded = provider
            .files
            .lock()
            .unwrap()
            .get(&manifest.database_path)
            .cloned()
            .expect("candidate uploaded");
        let uploaded_path = working_root.join("uploaded-candidate.sqlite");
        std::fs::write(&uploaded_path, uploaded).unwrap();
        let uploaded_conn = Connection::open(&uploaded_path).unwrap();
        let song_2_count: i64 = uploaded_conn
            .query_row(
                "SELECT COUNT(*) FROM songs WHERE hash = 'song-2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            song_2_count, 0,
            "newer mutation must not leak into old operation"
        );

        let repo = get_repository_state(&conn, "lib-1").unwrap().unwrap();
        assert_eq!(repo.local_state, LocalState::Dirty);
    }

    #[test]
    fn executor_retry_detects_remote_asset_identity_change() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));
        insert_song_with_media(
            &working_root.join("openkara.db"),
            "song-1",
            "media/song-1.mp3",
            &working_root,
        );
        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        provider.upload_file("media/song-1.mp3").unwrap();
        let op_id = make_pending_op(&conn, "lib-1", 0);
        persist_test_candidate(&conn, &provider, &working_root, &op_id);

        provider.store("media/song-1.mp3", b"other-bytes".to_vec(), "rev-replaced");

        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        assert!(execute_publish(&ctx, &op_id).is_err());
        let op = get_operation(&conn, &op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Failed);
        assert_eq!(op.error_code.as_deref(), Some("remote_integrity_failed"));
        assert!(provider.files.lock().unwrap().get(MANIFEST_PATH).is_none());
    }

    #[test]
    fn executor_rechecks_asset_fingerprint_before_manifest_cas() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));
        insert_song_with_media(
            &working_root.join("openkara.db"),
            "song-1",
            "media/song-1.mp3",
            &working_root,
        );
        let provider = FakeProvider::new()
            .with_working_copy_root(working_root.clone())
            .with_asset_mutation_on_candidate_upload(
                "media/song-1.mp3",
                b"other-bytes".to_vec(),
                "rev-raced",
            );
        provider.upload_file("media/song-1.mp3").unwrap();
        let op_id = make_pending_op(&conn, "lib-1", 0);

        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        assert!(execute_publish(&ctx, &op_id).is_err());
        let op = get_operation(&conn, &op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Failed);
        assert_eq!(op.error_code.as_deref(), Some("remote_integrity_failed"));
        assert!(provider.files.lock().unwrap().get(MANIFEST_PATH).is_none());
    }

    #[test]
    fn executor_does_not_rebuild_missing_persisted_candidate() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));
        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        let op_id = make_pending_op(&conn, "lib-1", 0);
        let candidate = persist_test_candidate(&conn, &provider, &working_root, &op_id);
        std::fs::remove_file(&candidate).unwrap();

        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        assert!(execute_publish(&ctx, &op_id).is_err());
        assert!(
            !candidate.exists(),
            "missing candidate must not be regenerated"
        );
        let op = get_operation(&conn, &op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Failed);
        assert_eq!(op.error_code.as_deref(), Some("remote_integrity_failed"));
        assert!(provider.files.lock().unwrap().get(MANIFEST_PATH).is_none());
    }

    #[test]
    fn executor_does_not_rebuild_corrupted_persisted_candidate() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));
        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        let op_id = make_pending_op(&conn, "lib-1", 0);
        let candidate = persist_test_candidate(&conn, &provider, &working_root, &op_id);
        std::fs::write(&candidate, b"corrupt-candidate").unwrap();

        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        assert!(execute_publish(&ctx, &op_id).is_err());
        assert_eq!(
            std::fs::read(&candidate).unwrap(),
            b"corrupt-candidate".to_vec()
        );
        let op = get_operation(&conn, &op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Failed);
        assert_eq!(op.error_code.as_deref(), Some("remote_integrity_failed"));
        assert!(provider.files.lock().unwrap().get(MANIFEST_PATH).is_none());
    }

    #[test]
    fn executor_rejects_partial_persisted_candidate_identity() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));
        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        let op_id = make_pending_op(&conn, "lib-1", 0);

        let mut op = get_operation(&conn, &op_id).unwrap().unwrap();
        let mut payload = OperationPayload::from_json(&op.payload_json).unwrap();
        payload.candidate_relative_path =
            Some(format!(".openkara/candidates/{}.sqlite", op.operation_id));
        op.payload_json = payload.to_json().unwrap();
        op.state = OperationState::RetryWait;
        upsert_operation(&conn, &op).unwrap();

        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        assert!(execute_publish(&ctx, &op_id).is_err());
        let op = get_operation(&conn, &op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Failed);
        assert_eq!(op.error_code.as_deref(), Some("remote_integrity_failed"));
        assert!(provider.files.lock().unwrap().get(MANIFEST_PATH).is_none());
    }

    #[test]
    fn executor_rejects_invalid_publish_payload_instead_of_rebuilding() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));
        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        let op_id = make_pending_op(&conn, "lib-1", 0);

        let mut op = get_operation(&conn, &op_id).unwrap().unwrap();
        op.payload_json = "{not-json".to_owned();
        op.state = OperationState::RetryWait;
        upsert_operation(&conn, &op).unwrap();

        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        assert!(execute_publish(&ctx, &op_id).is_err());
        let op = get_operation(&conn, &op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Failed);
        assert_eq!(op.error_code.as_deref(), Some("remote_integrity_failed"));
        assert!(provider.files.lock().unwrap().get(MANIFEST_PATH).is_none());
    }

    #[test]
    fn executor_asset_failure_leaves_existing_manifest_unchanged() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());

        let existing_manifest = RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation: 1,
            database_path: ".openkara/databases/1.sqlite".to_owned(),
            database_size_bytes: 100,
            database_sha256: "abc".to_owned(),
            committed_at_ms: 1000,
            writer_id: "other".to_owned(),
            operation_id: "op-test".to_owned(),
        };
        provider.store(
            MANIFEST_PATH,
            existing_manifest.to_json().unwrap().into_bytes(),
            "rev-gen-1",
        );

        insert_song_with_media(
            &working_root.join("openkara.db"),
            "song-1",
            "media/song-1.mp3",
            &working_root,
        );

        let op_id = make_pending_op(&conn, "lib-1", 1);
        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        let result = execute_publish(&ctx, &op_id);
        assert!(result.is_err(), "publish must fail");

        let manifest_bytes = provider.files.lock().unwrap().get(MANIFEST_PATH).cloned();
        let manifest: RepositoryManifest =
            serde_json::from_slice(&manifest_bytes.unwrap()).unwrap();
        assert_eq!(
            manifest.generation, 1,
            "existing manifest must be unchanged"
        );
        assert_eq!(manifest.writer_id, "other");
    }

    #[test]
    fn executor_fails_when_referenced_stem_is_missing() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let vocals = "stems/song-1/vocals.ogg";
        let accomp = "stems/song-1/accompaniment.ogg";
        insert_song_with_stems(
            &working_root.join("openkara.db"),
            "song-1",
            &[vocals, accomp],
            &working_root,
        );

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        provider.upload_file(vocals).expect("upload vocals");

        let op_id = make_pending_op(&conn, "lib-1", 0);
        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        let result = execute_publish(&ctx, &op_id);
        assert!(
            result.is_err(),
            "publish must fail when a stem is missing remotely"
        );

        let op = crate::remote::control_db::get_operation(&conn, &op_id)
            .unwrap()
            .unwrap();
        assert_eq!(op.state, OperationState::Failed);
        assert_eq!(op.error_code.as_deref(), Some("remote_integrity_failed"));
        assert!(provider.files.lock().unwrap().get(MANIFEST_PATH).is_none());
    }

    #[test]
    fn executor_fails_when_referenced_artwork_is_missing() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let thumb = "artwork/thumb_abc_80.webp";
        let preview = "artwork/preview_abc_256.webp";
        insert_song_with_artwork(
            &working_root.join("openkara.db"),
            "song-1",
            thumb,
            preview,
            &working_root,
        );

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        provider.upload_file(thumb).expect("upload thumb");

        let op_id = make_pending_op(&conn, "lib-1", 0);
        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        let result = execute_publish(&ctx, &op_id);
        assert!(
            result.is_err(),
            "publish must fail when artwork is missing remotely"
        );

        let op = crate::remote::control_db::get_operation(&conn, &op_id)
            .unwrap()
            .unwrap();
        assert_eq!(op.error_code.as_deref(), Some("remote_integrity_failed"));
    }

    #[test]
    fn executor_skips_verification_for_empty_path_columns() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());
        let op_id = make_pending_op(&conn, "lib-1", 0);
        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        execute_publish(&ctx, &op_id).expect("publish with no asset paths should succeed");
    }

    #[test]
    fn validate_asset_path_rejects_traversal_and_absolute_paths() {
        assert!(validate_asset_path("").is_err());
        assert!(validate_asset_path("/etc/passwd").is_err());
        assert!(validate_asset_path("media/../../../etc/passwd").is_err());
        assert!(validate_asset_path("media/./song.mp3").is_err());
        assert!(validate_asset_path("media//song.mp3").is_err());
        assert!(validate_asset_path("other/song.mp3").is_err());
        assert!(
            validate_asset_path("media").is_err(),
            "bare dir is not a file"
        );
        assert!(validate_asset_path("stems").is_err());
        assert!(validate_asset_path("media/song.mp3").is_ok());
        assert!(validate_asset_path("stems/song-1/vocals.ogg").is_ok());
        assert!(validate_asset_path("artwork/thumb_abc_80.webp").is_ok());
        assert!(validate_asset_path("media-g/song.zip").is_ok());
    }

    #[test]
    fn gc_deletes_old_database_generations() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());

        let manifest = RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation: 5,
            database_path: database_path_for_generation(5),
            database_size_bytes: 100,
            database_sha256: "abc".to_owned(),
            committed_at_ms: 5000,
            writer_id: "w-1".to_owned(),
            operation_id: "op-test".to_owned(),
        };
        provider.store(
            MANIFEST_PATH,
            manifest.to_json().unwrap().into_bytes(),
            "rev-manifest-5",
        );
        for gen in 1..=5 {
            provider.store(
                &database_path_for_generation(gen),
                format!("db-gen-{gen}").into_bytes(),
                &format!("rev-db-{gen}"),
            );
        }

        let now = current_unix_time_ms();
        let gc_op_id = format!("gc-lib-1-{now}");
        let gc_row = OperationRow {
            operation_id: gc_op_id.clone(),
            library_id: "lib-1".to_owned(),
            operation_kind: OperationKind::Gc,
            state: OperationState::Pending,
            expected_generation: Some(5),
            target_generation: None,
            source_db_digest: None,
            candidate_db_digest: None,
            payload_json: OperationPayload {
                song_ids: Vec::new(),
                percent: 0,
                detail: None,
                ..Default::default()
            }
            .to_json()
            .unwrap(),
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        upsert_operation(&conn, &gc_row).unwrap();

        execute_gc(&provider, &conn, "lib-1", &gc_op_id).expect("GC should succeed");

        for gen in 1..=3 {
            assert!(
                provider
                    .files
                    .lock()
                    .unwrap()
                    .get(&database_path_for_generation(gen))
                    .is_none(),
                "generation {gen} should be deleted"
            );
        }
        for gen in 4..=5 {
            assert!(
                provider
                    .files
                    .lock()
                    .unwrap()
                    .get(&database_path_for_generation(gen))
                    .is_some(),
                "generation {gen} should be retained"
            );
        }

        let op = get_operation(&conn, &gc_op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Completed);
    }

    #[test]
    fn gc_skips_when_manifest_not_yet_advanced() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());

        let manifest = RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation: 3,
            database_path: database_path_for_generation(3),
            database_size_bytes: 100,
            database_sha256: "abc".to_owned(),
            committed_at_ms: 3000,
            writer_id: "w-1".to_owned(),
            operation_id: "op-test".to_owned(),
        };
        provider.store(
            MANIFEST_PATH,
            manifest.to_json().unwrap().into_bytes(),
            "rev-manifest-3",
        );
        provider.store(
            &database_path_for_generation(1),
            b"db-gen-1".to_vec(),
            "rev-1",
        );

        let now = current_unix_time_ms();
        let gc_op_id = format!("gc-lib-1-{now}");
        let gc_row = OperationRow {
            operation_id: gc_op_id.clone(),
            library_id: "lib-1".to_owned(),
            operation_kind: OperationKind::Gc,
            state: OperationState::Pending,
            expected_generation: Some(5),
            target_generation: None,
            source_db_digest: None,
            candidate_db_digest: None,
            payload_json: OperationPayload {
                song_ids: Vec::new(),
                percent: 0,
                detail: None,
                ..Default::default()
            }
            .to_json()
            .unwrap(),
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        upsert_operation(&conn, &gc_row).unwrap();

        execute_gc(&provider, &conn, "lib-1", &gc_op_id).expect("GC should not error");

        assert!(
            provider
                .files
                .lock()
                .unwrap()
                .get(&database_path_for_generation(1))
                .is_some(),
            "generation 1 should be retained when manifest is behind"
        );

        let op = get_operation(&conn, &gc_op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Completed);
    }

    #[test]
    fn gc_does_not_delete_committed_generation() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());

        let manifest = RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation: 2,
            database_path: database_path_for_generation(2),
            database_size_bytes: 100,
            database_sha256: "abc".to_owned(),
            committed_at_ms: 2000,
            writer_id: "w-1".to_owned(),
            operation_id: "op-test".to_owned(),
        };
        provider.store(
            MANIFEST_PATH,
            manifest.to_json().unwrap().into_bytes(),
            "rev-manifest-2",
        );
        provider.store(
            &database_path_for_generation(1),
            b"db-gen-1".to_vec(),
            "rev-1",
        );
        provider.store(
            &database_path_for_generation(2),
            b"db-gen-2".to_vec(),
            "rev-2",
        );

        let now = current_unix_time_ms();
        let gc_op_id = format!("gc-lib-1-{now}");
        let gc_row = OperationRow {
            operation_id: gc_op_id.clone(),
            library_id: "lib-1".to_owned(),
            operation_kind: OperationKind::Gc,
            state: OperationState::Pending,
            expected_generation: Some(2),
            target_generation: None,
            source_db_digest: None,
            candidate_db_digest: None,
            payload_json: OperationPayload {
                song_ids: Vec::new(),
                percent: 0,
                detail: None,
                ..Default::default()
            }
            .to_json()
            .unwrap(),
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        upsert_operation(&conn, &gc_row).unwrap();

        execute_gc(&provider, &conn, "lib-1", &gc_op_id).expect("GC should succeed");

        assert!(
            provider
                .files
                .lock()
                .unwrap()
                .get(&database_path_for_generation(1))
                .is_some(),
            "generation 1 (previous) should be retained"
        );
        assert!(
            provider
                .files
                .lock()
                .unwrap()
                .get(&database_path_for_generation(2))
                .is_some(),
            "generation 2 (committed) should be retained"
        );
    }
}
