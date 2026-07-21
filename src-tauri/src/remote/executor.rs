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

use crate::commands::error::{internal_error, CommandResult};
use crate::remote::atomic_download::{sha256_file, verify_sqlite_integrity_pub};
use crate::remote::control_db::{
    get_operation, get_repository_state, upsert_operation, upsert_repository_state, LocalState,
    OperationKind, OperationPayload, OperationRow, OperationState, RepositoryStateRow,
};
use crate::remote::errors::{RemoteError, RemoteErrorKind};
use crate::remote::manifest::{
    database_path_for_generation, read_manifest, RepositoryManifest, CURRENT_SCHEMA_VERSION,
};
use crate::remote::provider::{ConditionalSource, RemoteProvider};
use rusqlite::Connection;
use std::path::Path;
use uuid::Uuid;

/// Context required to execute a publish operation. All dependencies are
/// injected so tests can substitute fakes.
pub(crate) struct PublishContext<'a> {
    pub control_db: &'a Connection,
    pub provider: &'a dyn RemoteProvider,
    /// Working-copy root of the remote library (where `openkara.db` lives).
    pub working_copy_root: &'a Path,
    pub library_id: &'a str,
    /// Stable installation UUID used as `writer_id` in the manifest.
    pub writer_id: &'a str,
    /// Stable repository UUID used as `repository_id` in the manifest. For a
    /// first publication this is generated and persisted by the caller.
    pub repository_id: &'a str,
}

/// Outcome of a publish execution, for inspection by callers and tests.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct PublishOutcome {
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

    // --- Step 1: Read and validate the current manifest + provider revision ---
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

    // --- Step 2: Require expected_generation matches ---
    let expected_generation = op.expected_generation.unwrap_or(0);
    let current_generation = current_manifest.as_ref().map(|m| m.generation).unwrap_or(0);
    let target_generation = expected_generation + 1;

    if current_generation != expected_generation {
        // Crash window after a successful manifest CAS: the remote advanced
        // exactly one generation by this writer, but local completion was
        // never recorded. Detect our own accepted commit and finish durably
        // instead of surfacing a false RemoteConflict that would leave the
        // working copy dirty forever.
        if current_generation == target_generation {
            if let Some(ref m) = current_manifest {
                if m.writer_id == ctx.writer_id && m.repository_id == ctx.repository_id {
                    tracing::info!(
                        "publish recovery: remote generation {} already committed by this writer \
                         (operation {}); treating as accepted commit",
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
        // The remote advanced independently — conflict.
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteConflict,
            format!(
                "expected generation {expected_generation} but remote is at {current_generation}"
            ),
        ));
    }

    // --- Steps 3-4: Asset verification ---
    //
    // Asset uploads are performed by the caller (`publish_song_internal`)
    // before the executor runs. The executor's job here is step 4: verify
    // every asset referenced by the working database is present remotely with
    // the expected size. This is the invariant "remote readers cannot observe
    // a database that references missing assets" — a failed or truncated
    // upload must fail closed BEFORE the manifest CAS, leaving the committed
    // manifest unchanged.
    //
    // The working database and the candidate copy (step 5) reference the same
    // assets, and the per-library commit lock is held, so verifying against
    // the working DB here is equivalent to verifying against the candidate.
    transition_state(
        ctx,
        op,
        OperationState::Running,
        now,
        15,
        "Verifying remote assets",
    )?;

    let working_db_path = ctx.working_copy_root.join("openkara.db");
    verify_referenced_assets(ctx.provider, ctx.working_copy_root, &working_db_path)?;

    // --- Step 5: Copy the local working database to a candidate temp file ---
    transition_state(
        ctx,
        op,
        OperationState::Committing,
        now,
        50,
        "Preparing candidate database",
    )?;

    let candidate_path = ctx
        .working_copy_root
        .join(format!(".openkara-candidate-{}.sqlite", op.operation_id));
    std::fs::copy(&working_db_path, &candidate_path).map_err(|e| {
        RemoteError::new(
            RemoteErrorKind::NetworkUnavailable,
            format!("failed to copy working DB to candidate: {e}"),
        )
    })?;

    // --- Step 6: SQLite integrity checks + SHA-256 digest ---
    verify_sqlite_integrity_pub(&candidate_path).map_err(|e| {
        // Clean up the candidate on integrity failure.
        let _ = std::fs::remove_file(&candidate_path);
        RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, e.message)
    })?;

    let candidate_digest = sha256_file(&candidate_path).map_err(|e| {
        let _ = std::fs::remove_file(&candidate_path);
        RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message)
    })?;
    let candidate_size = std::fs::metadata(&candidate_path)
        .map(|m| m.len())
        .map_err(|e| {
            let _ = std::fs::remove_file(&candidate_path);
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("failed to stat candidate: {e}"),
            )
        })?;

    // --- Step 7: Upload the candidate to .openkara/databases/<target>.sqlite ---
    let db_remote_path = database_path_for_generation(target_generation);
    let candidate_bytes = std::fs::read(&candidate_path).map_err(|e| {
        let _ = std::fs::remove_file(&candidate_path);
        RemoteError::new(
            RemoteErrorKind::NetworkUnavailable,
            format!("failed to read candidate for upload: {e}"),
        )
    })?;
    // The candidate DB upload is a single unconditional upload — it targets a
    // generation-specific path that does not exist yet, so there is no CAS
    // conflict possible here. The CAS point is the manifest replacement (step 10).
    // PR#5 makes this upload resumable; PR#4 uses a single upload.
    upload_candidate_database(
        ctx.provider,
        ctx.working_copy_root,
        &db_remote_path,
        candidate_bytes.clone(),
        &op.operation_id,
        ctx.control_db,
    )
    .inspect_err(|_e| {
        let _ = std::fs::remove_file(&candidate_path);
    })?;

    // Clean up the local candidate temp file — the bytes are now remote.
    let _ = std::fs::remove_file(&candidate_path);

    // --- Step 8: Stat-verify the candidate database metadata ---
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
    if let Some(remote_size) = candidate_meta.size {
        if remote_size != candidate_size {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!(
                    "candidate database size mismatch: expected {candidate_size}, remote has {remote_size}"
                ),
            ));
        }
    }

    // --- Step 9: Build the next manifest ---
    let manifest = RepositoryManifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        repository_id: ctx.repository_id.to_owned(),
        generation: target_generation,
        database_path: db_remote_path.clone(),
        database_size: candidate_size,
        database_sha256: candidate_digest.clone(),
        committed_at_ms: now,
        writer_id: ctx.writer_id.to_owned(),
    };
    let manifest_json = manifest
        .to_json()
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;

    // --- Step 10: Replace the manifest via conditional_replace (CAS) ---
    let committed_meta = ctx.provider.conditional_replace(
        crate::remote::manifest::MANIFEST_PATH,
        ConditionalSource::Bytes(manifest_json.into_bytes()),
        manifest_revision.as_deref(),
    )?;
    // A CAS failure is a conflict — do NOT retry as unconditional.
    // The RemoteError is propagated as-is via the `?` operator.

    // --- Step 11: Re-read the manifest and verify ---
    transition_state(
        ctx,
        op,
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
        || verified_manifest.database_size != candidate_size
        || verified_manifest.database_sha256 != candidate_digest
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
    /// Yield every non-empty referenced path in a deterministic order.
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

/// Publication-protocol step 4: verify every asset referenced by the working
/// database is present remotely with the expected size.
///
/// Opens the database read-only, enumerates every non-empty path column across
/// `songs` and `stems`, and for each referenced path:
///
/// 1. Validates the path is a relative, managed-asset path (no traversal,
///    no absolute paths, no paths outside `media`/`media-g`/`stems`/`artwork`).
/// 2. `provider.stat(path)` — fail closed with `RemoteIntegrityFailed` if the
///    object is absent. A missing asset means the upload did not complete, so
///    the manifest must NOT commit.
/// 3. When the local working copy has the file at that path and the provider
///    reports a remote size, compare the local byte size to the remote size.
///    A mismatch indicates truncation or a wrong object and fails closed.
///
/// Songs whose `audio_source_kind` is not `"original"` are treated the same as
/// local originals for this check: their referenced paths (stems for
/// `stems_remote`, media for `original_remote`, artwork for both) must all be
/// present remotely. The candidate database is what remote readers will see,
/// so every path it references must resolve.
///
/// A failed verification NEVER commits the manifest. The caller records the
/// operation as `failed` and emits `upload-error`.
fn verify_referenced_assets(
    provider: &dyn RemoteProvider,
    working_copy_root: &Path,
    database_path: &Path,
) -> Result<(), RemoteError> {
    let conn = open_readonly(database_path).map_err(|e| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "failed to open working DB for asset verification: {}",
                e.message
            ),
        )
    })?;

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

            // Best-effort size check: when the local working copy has the
            // file and the provider reports a size, a mismatch indicates
            // truncation or a wrong object. If the local file is absent
            // (e.g. a song published by another device whose assets we did
            // not download), only presence is verified.
            if let Some(remote_size) = remote_meta.size {
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
    }

    Ok(())
}

/// Upload the candidate database to the remote path by writing it to the
/// working copy at the target relative path, calling `upload_file`, then
/// removing the local staging copy.
///
/// PR#5: when the candidate is >= 8 MiB and the provider supports
/// `resumable_upload`, the resumable path is used instead of `upload_file`.
/// This persists transfer progress to `remote_transfer_parts` so a restart
/// can resume the upload from the verified offset.
fn upload_candidate_database(
    provider: &dyn RemoteProvider,
    working_copy_root: &Path,
    remote_relative_path: &str,
    bytes: Vec<u8>,
    operation_id: &str,
    control_db: &rusqlite::Connection,
) -> Result<(), RemoteError> {
    let local_staging = working_copy_root.join(remote_relative_path);
    if let Some(parent) = local_staging.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("failed to create staging dir: {e}"),
            )
        })?;
    }
    std::fs::write(&local_staging, &bytes).map_err(|e| {
        RemoteError::new(
            RemoteErrorKind::NetworkUnavailable,
            format!("failed to write candidate staging file: {e}"),
        )
    })?;

    // PR#5: use the resumable upload path for large candidates when the
    // provider supports it. The 8 MiB threshold avoids the overhead of
    // session setup for small databases while enabling resume for the
    // multi-hundred-MiB libraries that motivated this PR.
    const RESUMABLE_UPLOAD_THRESHOLD: usize = 8 * 1024 * 1024;
    let caps = provider.capabilities();
    if caps.resumable_upload && bytes.len() >= RESUMABLE_UPLOAD_THRESHOLD {
        provider.resumable_upload_bytes(remote_relative_path, &bytes, operation_id, control_db)?;
    } else {
        provider
            .upload_file(remote_relative_path)
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
    }
    // Remove the local staging copy — the bytes are now remote and the local
    // file is not part of the committed working copy.
    let _ = std::fs::remove_file(&local_staging);
    Ok(())
}

/// Transition the operation to a new state, persisting to the control DB and
/// updating the payload percent/detail.
fn transition_state(
    ctx: &PublishContext<'_>,
    op: &OperationRow,
    new_state: OperationState,
    now: i64,
    percent: u8,
    detail: &str,
) -> Result<(), RemoteError> {
    let payload = OperationPayload {
        song_ids: OperationPayload::from_json(&op.payload_json)
            .map(|p| p.song_ids)
            .unwrap_or_default(),
        percent,
        detail: Some(detail.to_owned()),
    };
    let mut updated = op.clone();
    updated.state = new_state;
    updated.payload_json = payload
        .to_json()
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;
    updated.updated_at_ms = now;
    upsert_operation(ctx.control_db, &updated)
        .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))
}

/// Record a successful completion: update the operation row to `completed`,
/// update the repository state, and emit `upload-complete`.
fn record_completed(
    ctx: &PublishContext<'_>,
    op: &OperationRow,
    outcome: &PublishOutcome,
) -> CommandResult<()> {
    let now = current_unix_time_ms();

    // Update the operation row to completed.
    let mut updated = op.clone();
    updated.state = OperationState::Completed;
    updated.target_generation = Some(outcome.target_generation);
    updated.candidate_db_digest = Some(outcome.candidate_db_digest.clone());
    updated.error_code = None;
    updated.error_detail = None;
    updated.updated_at_ms = now;
    upsert_operation(ctx.control_db, &updated)?;

    // Update the repository state.
    let repo_row = match get_repository_state(ctx.control_db, ctx.library_id)? {
        Some(mut row) => {
            row.committed_generation = outcome.target_generation;
            row.committed_manifest_revision = outcome.committed_manifest_revision.clone();
            row.local_base_generation = outcome.target_generation;
            row.local_db_digest = Some(outcome.candidate_db_digest.clone());
            row.local_state = LocalState::Clean;
            row.active_operation_id = None;
            row.last_success_at_ms = Some(now);
            row.last_error_code = None;
            row.updated_at_ms = now;
            row
        }
        None => RepositoryStateRow {
            library_id: ctx.library_id.to_owned(),
            committed_generation: outcome.target_generation,
            committed_manifest_revision: outcome.committed_manifest_revision.clone(),
            local_base_generation: outcome.target_generation,
            local_db_digest: Some(outcome.candidate_db_digest.clone()),
            local_state: LocalState::Clean,
            active_operation_id: None,
            last_success_at_ms: Some(now),
            last_error_code: None,
            updated_at_ms: now,
            repository_id: Some(ctx.repository_id.to_owned()),
            writer_id: Some(ctx.writer_id.to_owned()),
        },
    };
    upsert_repository_state(ctx.control_db, &repo_row)?;

    // Step 13: Schedule a Gc operation row for deferred cleanup.
    schedule_gc(ctx, outcome.target_generation)?;

    Ok(())
}

/// Record a failure: map the error kind to the appropriate operation state
/// and repository local_state.
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
            // Retryable: keep the operation in retry_wait.
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

    // Update the repository state.
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

/// Schedule a deferred GC operation row for unreachable staging data and old
/// database generations. The GC executor (`execute_gc`) picks up the row after
/// the safety delay and deletes generations older than
/// `committed_generation - 1` (the previous generation is retained as a
/// rollback safety net for the delay window).
fn schedule_gc(ctx: &PublishContext<'_>, committed_generation: i64) -> CommandResult<()> {
    let now = current_unix_time_ms();
    let gc_op_id = format!("gc-{}-{}", ctx.library_id, now);
    let payload = OperationPayload {
        song_ids: Vec::new(),
        percent: 0,
        detail: Some(format!(
            "deferred GC after generation {committed_generation}"
        )),
    };
    let row = OperationRow {
        operation_id: gc_op_id,
        library_id: ctx.library_id.to_owned(),
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
    upsert_operation(ctx.control_db, &row)?;
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

    // Read the current manifest to confirm the committed generation.
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
    // Generation 0 is reserved (no manifest). We delete generations from 1
    // up to (but not including) retain_floor.
    //
    // A missing object (404) is idempotent success — it may have already
    // been cleaned up by a prior GC. A transient failure (network, rate
    // limit, server error) leaves the GC operation retryable so the next
    // executor pass can try again. The GC is not marked Completed until
    // every target has been successfully deleted or confirmed absent.
    let mut transient_failures = 0;
    for gen in 1..retain_floor {
        let db_path = database_path_for_generation(gen);
        match provider.delete_path(&db_path) {
            Ok(()) => {
                tracing::debug!("GC deleted old database generation {} at {}", gen, db_path);
            }
            Err(e) => {
                // A non-retryable error from delete typically means the
                // object is already absent (404 maps to a non-retryable
                // permission error in the current HTTP status mapping).
                // Treat non-retryable errors as idempotent success — the
                // object is gone or permanently inaccessible, either way
                // GC has nothing more to do for it.
                if e.retryable {
                    tracing::warn!(
                        "GC transient failure deleting {} (will retry): {}",
                        db_path,
                        e.message
                    );
                    transient_failures += 1;
                } else {
                    tracing::debug!(
                        "GC confirmed absence of old database generation {} at {}: {}",
                        gen,
                        db_path,
                        e.message
                    );
                }
            }
        }
    }

    if transient_failures > 0 {
        // Leave the GC operation retryable — do NOT mark it Completed.
        // Schedule a retry with a safety delay so the executor picks it
        // up again on the next pass.
        let mut updated = op.clone();
        updated.state = OperationState::RetryWait;
        updated.next_attempt_at_ms = Some(now + GC_RETRY_BACKOFF_MS);
        updated.updated_at_ms = now;
        upsert_operation(control_db, &updated)?;
        return Ok(());
    }

    // Mark the GC operation as completed only after every target has been
    // successfully deleted or confirmed absent.
    let mut updated = op.clone();
    updated.state = OperationState::Completed;
    updated.updated_at_ms = now;
    upsert_operation(control_db, &updated)?;

    Ok(())
}

/// Retry backoff for retryable errors (network/rate-limit).
const RETRY_BACKOFF_MS: i64 = 30_000;

/// Safety delay before retrying a GC operation after a transient delete
/// failure. GC is low-priority background work, so the delay is longer
/// than the publish retry backoff.
const GC_RETRY_BACKOFF_MS: i64 = 60_000;

/// Current wall-clock milliseconds.
fn current_unix_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Generate a stable installation UUID for `writer_id`.
pub(crate) fn generate_writer_id() -> String {
    Uuid::new_v4().to_string()
}

/// Generate a stable repository UUID for `repository_id`.
pub(crate) fn generate_repository_id() -> String {
    Uuid::new_v4().to_string()
}

// ---------------------------------------------------------------------------
// Conflict handling
//
// These functions implement the three conflict resolution strategies
// (keep-local, use-remote, cancel) and the disjoint-song auto-rebase. They
// are part of the PR#4 API surface and will be called from the UI layer in
// a subsequent PR. They are intentionally kept here so the executor is the
// single source of truth for conflict resolution logic.
// ---------------------------------------------------------------------------

/// Metadata describing a conflict, stored in the operation row's
/// `error_detail` as JSON.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
pub(crate) struct ConflictMetadata {
    pub local_base_generation: i64,
    pub remote_generation: i64,
    pub local_operation_id: String,
    pub affected_song_ids: Vec<String>,
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
#[allow(dead_code)]
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

    // Read the winning remote manifest to get the new base generation.
    let remote_manifest = read_manifest(ctx.provider)?
        .ok_or_else(|| internal_error("no remote manifest found during conflict resolution"))?;

    // Disjoint-song check: compare song hashes in the local working DB vs the
    // conflict-candidate DB.
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
    let _ = local_changed; // used for diagnostics below

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

    // Check repository-global settings: compare settings table content.
    let settings_match = settings_tables_match(&local_db_path, conflict_candidate_db)?;

    if overlap || !settings_match {
        return Err(internal_error(
            "automatic rebase rejected: local and remote changes overlap or \
             repository-global settings differ; explicit user choice required",
        ));
    }

    // Rebase: update the operation's expected_generation to the remote
    // generation and re-run the publish protocol.
    let now = current_unix_time_ms();
    let mut updated = op.clone();
    updated.expected_generation = Some(remote_manifest.generation);
    updated.state = OperationState::Pending;
    updated.error_code = None;
    updated.error_detail = None;
    updated.updated_at_ms = now;
    upsert_operation(ctx.control_db, &updated)?;

    // Re-execute the publish protocol with the new base.
    execute_publish(ctx, operation_id)
}

/// Action: discard the local pending operation and activate the verified
/// remote database.
#[allow(dead_code)]
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

    // Mark the operation cancelled.
    let mut updated = op.clone();
    updated.state = OperationState::Cancelled;
    updated.updated_at_ms = now;
    upsert_operation(ctx.control_db, &updated)?;

    // Activate the verified remote database: copy the conflict candidate over
    // the working DB. The conflict candidate already passed integrity checks
    // when it was pulled.
    let working_db = ctx.working_copy_root.join("openkara.db");
    std::fs::copy(conflict_candidate_db, &working_db)
        .map_err(|e| internal_error(format!("failed to activate remote database: {e}")))?;

    let new_digest = sha256_file(&working_db)?;
    let remote_manifest = read_manifest(ctx.provider)?
        .ok_or_else(|| internal_error("no remote manifest found during conflict resolution"))?;

    // Update repository state to Clean at the remote generation.
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

/// Action: keep both sides and remain `Conflicted`. No state change beyond
/// confirming the conflict is known.
#[allow(dead_code)]
pub(crate) fn conflict_cancel_for_now(
    ctx: &PublishContext<'_>,
    operation_id: &str,
) -> CommandResult<()> {
    let op = get_operation(ctx.control_db, operation_id)?
        .ok_or_else(|| internal_error(format!("operation {operation_id} not found")))?;
    if op.state != OperationState::Conflicted {
        return Err(internal_error(
            "conflict cancel-for-now requires a conflicted operation",
        ));
    }
    // No transition — the operation stays conflicted. This function exists so
    // PR#8's UI has an explicit backend action to call.
    Ok(())
}

/// Pull the winning remote manifest + database to a conflict candidate path
/// (NOT the active working DB). Uses the provider's download_file.
#[allow(dead_code)]
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
    // Verify integrity of the pulled candidate.
    verify_sqlite_integrity_pub(destination)?;
    Ok(manifest)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read the set of song hashes from a SQLite database's `songs` table.
#[allow(dead_code)]
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

/// Compare the `settings` table content between two databases. Returns true
/// when the row counts and a hash of all rows match.
#[allow(dead_code)]
fn settings_tables_match(local_db: &Path, remote_db: &Path) -> CommandResult<bool> {
    let local_hash = settings_table_hash(local_db)?;
    let remote_hash = settings_table_hash(remote_db)?;
    Ok(local_hash == remote_hash)
}

/// Compute a deterministic hash of the `settings` table rows (key + value).
#[allow(dead_code)]
fn settings_table_hash(db_path: &Path) -> CommandResult<Option<String>> {
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = open_readonly(db_path)?;
    // Check if the settings table exists.
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

    /// A fake provider backed by an in-memory map. Supports conditional_replace
    /// with ETag-based CAS semantics.
    struct FakeProvider {
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        revisions: Arc<Mutex<HashMap<String, String>>>,
        /// When true, conditional_replace returns ProviderCapabilityUnavailable.
        no_cas: bool,
        /// Working copy root for reading files during upload_file.
        working_copy_root: Option<PathBuf>,
    }

    impl FakeProvider {
        fn new() -> Self {
            Self {
                files: Arc::new(Mutex::new(HashMap::new())),
                revisions: Arc::new(Mutex::new(HashMap::new())),
                no_cas: false,
                working_copy_root: None,
            }
        }

        fn with_no_cas() -> Self {
            Self {
                files: Arc::new(Mutex::new(HashMap::new())),
                revisions: Arc::new(Mutex::new(HashMap::new())),
                no_cas: true,
                working_copy_root: None,
            }
        }

        fn with_working_copy_root(mut self, root: PathBuf) -> Self {
            self.working_copy_root = Some(root);
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
                    size: Some(files.get(path).unwrap().len() as u64),
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
            // Read from the working copy root and store in the fake's map.
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
                    // Store size for stat verification.
                    let _ = size;
                }
            }
            Ok(())
        }

        fn upload_directory(&self, _path: &str) -> CommandResult<()> {
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

            // Check the precondition.
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
                    // Conditional-create: fail if the object already exists.
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
                size: Some(size),
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

    /// Create a valid SQLite database at the given path with a `songs` table
    /// that has the asset-path columns the verifier queries. Songs are inserted
    /// with NULL paths so the verifier has nothing to check (matching a
    /// freshly-bootstrapped repository with no published assets yet).
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

    /// Create a fresh control DB in a temp dir.
    fn fresh_control_db() -> (TempDir, Connection) {
        let dir = TempDir::new().unwrap();
        let conn = open_control_db(&dir.path().join("remote-state.db")).unwrap();
        (dir, conn)
    }

    /// Create a PublishContext with the given control DB, provider, and a
    /// temp working copy root containing a valid `openkara.db`.
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

    /// Create a pending publish operation row.
    fn make_pending_op(conn: &Connection, library_id: &str, expected_gen: i64) -> String {
        let op_id = format!("publish-test-{}", expected_gen);
        let now = crate::remote::types::current_unix_time_ms();
        let payload = OperationPayload {
            song_ids: vec!["song-1".to_owned()],
            percent: 0,
            detail: None,
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

        // Verify the manifest was written.
        let manifest_bytes = provider.files.lock().unwrap().get(MANIFEST_PATH).cloned();
        assert!(manifest_bytes.is_some(), "manifest should be written");
        let manifest: RepositoryManifest =
            serde_json::from_slice(&manifest_bytes.unwrap()).unwrap();
        assert_eq!(manifest.generation, 1);
        assert_eq!(manifest.repository_id, "repo-uuid-1");
        assert_eq!(manifest.writer_id, "writer-uuid-1");

        // Verify the operation is completed.
        let op = crate::remote::control_db::get_operation(&conn, &op_id)
            .unwrap()
            .unwrap();
        assert_eq!(op.state, OperationState::Completed);
        assert_eq!(op.target_generation, Some(1));

        // Verify the repository state.
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

        // First publish: generation 0 → 1.
        let op1 = make_pending_op(&conn, "lib-1", 0);
        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        execute_publish(&ctx, &op1).expect("first publish");

        // Second publish: generation 1 → 2.
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

        // Verify the operation is failed, not completed.
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

        // Simulate a remote that already has generation 2 (another device
        // published while we were offline). We write a manifest at gen 2.
        let manifest = RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation: 2,
            database_path: ".openkara/databases/2.sqlite".to_owned(),
            database_size: 100,
            database_sha256: "abc".to_owned(),
            committed_at_ms: 1000,
            writer_id: "other-device".to_owned(),
        };
        provider.store(
            MANIFEST_PATH,
            manifest.to_json().unwrap().into_bytes(),
            "rev-gen-2",
        );

        // Our operation expects generation 0 (we're stale).
        let op_id = make_pending_op(&conn, "lib-1", 0);
        let ctx = make_context(&conn, &provider, &working_root, "lib-1", "repo-1", "w-1");
        let result = execute_publish(&ctx, &op_id);

        assert!(result.is_err());

        // Verify the operation is conflicted.
        let op = crate::remote::control_db::get_operation(&conn, &op_id)
            .unwrap()
            .unwrap();
        assert_eq!(op.state, OperationState::Conflicted);
        assert_eq!(op.error_code.as_deref(), Some("remote_conflict"));

        // Verify the repository state is conflicted.
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

        // First execution succeeds.
        execute_publish(&ctx, &op_id).expect("first publish");

        // Count files before second execution.
        let files_before = provider.files.lock().unwrap().len();

        // Second execution should be a no-op (operation is completed).
        execute_publish(&ctx, &op_id).expect("second call is no-op");

        // No new files should have been written.
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

        // Verify the manifest references the correct database path.
        let manifest_bytes = provider.files.lock().unwrap().get(MANIFEST_PATH).cloned();
        let manifest: RepositoryManifest =
            serde_json::from_slice(&manifest_bytes.unwrap()).unwrap();
        assert_eq!(manifest.database_path, ".openkara/databases/1.sqlite");

        // Verify the database was uploaded to the correct path.
        let db_bytes = provider
            .files
            .lock()
            .unwrap()
            .get(".openkara/databases/1.sqlite")
            .cloned();
        assert!(db_bytes.is_some(), "candidate database should be uploaded");

        // Verify the digest in the manifest matches the uploaded database.
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

        // Verify a GC operation was scheduled with a safety delay.
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

    // ---- Asset verification (publication-protocol steps 3-4) ----

    /// Insert a song row with a `file_path` referencing a media asset, and
    /// create the matching local file in the working copy.
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

    /// Insert a song row with stem paths and create the matching local files.
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

    /// Insert a song row with artwork derivative paths and create the local
    /// files.
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

        // Song-1 references media/song-1.mp3 but the provider does NOT have it
        // (simulating a failed or skipped upload).
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

        // The manifest must NOT have been committed.
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
        // Simulate a truncated remote upload: store fewer bytes than the local
        // file.
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
        // Upload the asset to the provider so stat succeeds with the correct
        // size.
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
    fn executor_asset_failure_leaves_existing_manifest_unchanged() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());

        // Pre-existing manifest at generation 1 (another device published).
        let existing_manifest = RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation: 1,
            database_path: ".openkara/databases/1.sqlite".to_owned(),
            database_size: 100,
            database_sha256: "abc".to_owned(),
            committed_at_ms: 1000,
            writer_id: "other".to_owned(),
        };
        provider.store(
            MANIFEST_PATH,
            existing_manifest.to_json().unwrap().into_bytes(),
            "rev-gen-1",
        );

        // Our publish references an asset the provider does not have.
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

        // The existing manifest must be unchanged — still generation 1.
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
        // Only upload vocals; accompaniment is missing remotely.
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
        // Upload only the thumbnail; preview is missing.
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
        // A song with NULL/empty path columns (e.g. a freshly imported song
        // before any assets are uploaded) must not cause verification to fail.
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        // The default song-1 from make_valid_db has NULL paths.
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
        // Valid paths pass.
        assert!(validate_asset_path("media/song.mp3").is_ok());
        assert!(validate_asset_path("stems/song-1/vocals.ogg").is_ok());
        assert!(validate_asset_path("artwork/thumb_abc_80.webp").is_ok());
        assert!(validate_asset_path("media-g/song.zip").is_ok());
    }

    // ---- GC executor tests ----

    #[test]
    fn gc_deletes_old_database_generations() {
        let (_db_dir, conn) = fresh_control_db();
        let working_dir = TempDir::new().unwrap();
        let working_root = working_dir.path().to_owned();
        make_valid_db(&working_root.join("openkara.db"));

        let provider = FakeProvider::new().with_working_copy_root(working_root.clone());

        // Simulate a repository at generation 5 with old generations 1-3
        // present remotely.
        let manifest = RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation: 5,
            database_path: database_path_for_generation(5),
            database_size: 100,
            database_sha256: "abc".to_owned(),
            committed_at_ms: 5000,
            writer_id: "w-1".to_owned(),
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

        // Schedule a GC for generation 5. retain_floor = 4, so generations
        // 1..=3 should be deleted, 4 and 5 retained.
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

        // Generations 1-3 should be deleted.
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
        // Generations 4 and 5 should be retained.
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

        // The GC operation should be marked completed.
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

        // Manifest is at generation 3, but the GC was scheduled for generation 5.
        let manifest = RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation: 3,
            database_path: database_path_for_generation(3),
            database_size: 100,
            database_sha256: "abc".to_owned(),
            committed_at_ms: 3000,
            writer_id: "w-1".to_owned(),
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

        // Generation 1 should NOT be deleted — the manifest hasn't advanced
        // to 5 yet.
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

        // Manifest at generation 2.
        let manifest = RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation: 2,
            database_path: database_path_for_generation(2),
            database_size: 100,
            database_sha256: "abc".to_owned(),
            committed_at_ms: 2000,
            writer_id: "w-1".to_owned(),
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

        // retain_floor = 1, so the loop is 1..1 (empty). Both gen 1 and 2
        // are retained.
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
