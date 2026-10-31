//! The identity of the base snapshot a Local Working Copy rebases from
//! (ADR 0040).
//!
//! The control database records, next to the base generation, the SHA-256
//! of that generation's snapshot. The core writer only rebases from a
//! retained base whose bytes still hash to it, so this module is where an
//! identity is proven before it is recorded: never from bytes that may
//! carry unpublished edits, and never from today's remote.

use std::path::Path;

use rusqlite::Connection;

use crate::commands::error::{CommandError, CommandResult};
use crate::remote::control_db::{
    get_repository_state, list_operations_for_library, upsert_repository_state, LocalState,
    OperationKind, OperationRow, OperationState, RepositoryStateRow,
};
use crate::remote::types::current_unix_time_ms;

/// The base identity to hand the core writer for `op`: the recorded
/// identity of the base generation, and only when `op` expects that very
/// generation. The operation's own `source_db_digest` is the working copy
/// before the mutation, not its base, and never stands in for it.
pub(crate) fn base_identity_for_operation(
    op: &OperationRow,
    state: &RepositoryStateRow,
) -> Option<String> {
    (op.expected_generation == Some(state.local_base_generation))
        .then(|| state.local_base_sha256.clone())
        .flatten()
}

/// Make sure the base of the recorded base generation is retained under a
/// recorded identity, and return that identity.
///
/// A working copy from an older release has no recorded identity, and may
/// keep its base in the single file those releases used. Its
/// `local_db_digest` is the digest recorded when that generation was
/// installed: a base, old or new layout, is adopted only when it hashes to
/// it. A clean working copy that still hashes to the identity becomes the
/// base when none was retained. Anything else stays unproven, and the next
/// rebase stops with a conflict instead of guessing.
pub(crate) fn ensure_base_identity(
    control_db: &Connection,
    library_id: &str,
    working_copy_root: &Path,
) -> CommandResult<Option<String>> {
    let Some(mut state) = get_repository_state(control_db, library_id)? else {
        return Ok(None);
    };
    let recorded = state.local_base_sha256.clone();
    let Some(trusted) = recorded.clone().or_else(|| state.local_db_digest.clone()) else {
        return Ok(None);
    };
    let generation = state.local_base_generation;

    let mut proven =
        openkara_remote::verified_base_snapshot(working_copy_root, generation, Some(&trusted))
            .map_err(CommandError::from)?
            .is_some();

    if !proven {
        let adoption = openkara_remote::adopt_legacy_base_snapshot(
            working_copy_root,
            generation,
            Some(&trusted),
        )
        .map_err(CommandError::from)?;
        proven = matches!(
            adoption,
            openkara_remote::LegacyBaseAdoption::Adopted
                | openkara_remote::LegacyBaseAdoption::AlreadyRetained
        );
    }

    // Without a recorded identity, `local_db_digest` may have been recorded
    // again from a working copy that a mutation changed just before a crash:
    // such a copy is never trusted while a publication of that mutation is
    // still unfinished.
    let working_copy_may_be_base =
        recorded.is_some() || !has_unfinished_publication(control_db, library_id)?;
    if !proven && working_copy_may_be_base {
        proven = retain_clean_working_copy(&state, working_copy_root, &trusted)?;
    }

    if !proven {
        return Ok(None);
    }
    if recorded.as_deref() != Some(trusted.as_str()) {
        state.local_base_sha256 = Some(trusted.clone());
        state.updated_at_ms = current_unix_time_ms();
        upsert_repository_state(control_db, &state)?;
    }
    Ok(Some(trusted))
}

/// Whether a publish operation of `library_id` may still carry edits the
/// remote does not have.
fn has_unfinished_publication(control_db: &Connection, library_id: &str) -> CommandResult<bool> {
    Ok(list_operations_for_library(control_db, library_id)?
        .iter()
        .any(|op| {
            op.operation_kind == OperationKind::Publish
                && !matches!(
                    op.state,
                    OperationState::Completed | OperationState::Cancelled
                )
        }))
}

fn retain_clean_working_copy(
    state: &RepositoryStateRow,
    working_copy_root: &Path,
    trusted: &str,
) -> CommandResult<bool> {
    if state.local_state != LocalState::Clean || state.active_operation_id.is_some() {
        return Ok(false);
    }
    let database = working_copy_root.join("openkara.db");
    if !database.is_file()
        || !crate::remote::atomic_download::sha256_file(&database)?.eq_ignore_ascii_case(trusted)
    {
        return Ok(false);
    }
    let retained = openkara_remote::retain_base_snapshot(
        working_copy_root,
        state.local_base_generation,
        &database,
    )
    .map_err(CommandError::from)?;
    Ok(retained.eq_ignore_ascii_case(trusted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::control_db::{open_control_db, upsert_operation};

    fn state(local_state: LocalState, local_db_digest: Option<String>) -> RepositoryStateRow {
        RepositoryStateRow {
            library_id: "lib-1".to_owned(),
            committed_generation: 4,
            committed_manifest_revision: None,
            local_base_generation: 4,
            local_db_digest,
            local_state,
            active_operation_id: None,
            last_success_at_ms: None,
            last_error_code: None,
            updated_at_ms: 0,
            repository_id: None,
            writer_id: None,
            local_base_sha256: None,
        }
    }

    fn operation(expected_generation: Option<i64>, source_db_digest: &str) -> OperationRow {
        OperationRow {
            operation_id: format!("op-{source_db_digest}"),
            library_id: "lib-1".to_owned(),
            operation_kind: OperationKind::Publish,
            state: OperationState::Pending,
            expected_generation,
            target_generation: None,
            source_db_digest: Some(source_db_digest.to_owned()),
            candidate_db_digest: None,
            payload_json: "{}".to_owned(),
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    fn working_copy() -> (tempfile::TempDir, Connection, String) {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("openkara.db");
        Connection::open(&database)
            .unwrap()
            .execute_batch("CREATE TABLE songs (hash TEXT PRIMARY KEY);")
            .unwrap();
        let digest = crate::remote::atomic_download::sha256_file(&database).unwrap();
        let control_db = open_control_db(&root.path().join("remote-state.db")).unwrap();
        (root, control_db, digest)
    }

    #[test]
    fn queued_operations_share_the_base_identity_whatever_their_source_digest() {
        let mut state = state(LocalState::Dirty, Some("working-digest".to_owned()));
        state.local_base_sha256 = Some("base-4".to_owned());

        for source in ["pre-op-a", "pre-op-b"] {
            assert_eq!(
                base_identity_for_operation(&operation(Some(4), source), &state).as_deref(),
                Some("base-4"),
                "{source}"
            );
        }
        assert_eq!(
            base_identity_for_operation(&operation(Some(3), "pre-op-c"), &state),
            None,
            "an operation expecting another generation has no base"
        );
        assert_eq!(
            base_identity_for_operation(&operation(None, "pre-op-d"), &state),
            None
        );
    }

    #[test]
    fn a_clean_working_copy_with_the_recorded_digest_becomes_the_base() {
        let (root, control_db, digest) = working_copy();
        upsert_repository_state(&control_db, &state(LocalState::Clean, Some(digest.clone())))
            .unwrap();

        let identity = ensure_base_identity(&control_db, "lib-1", root.path()).unwrap();

        assert_eq!(identity.as_deref(), Some(digest.as_str()));
        assert!(openkara_remote::base_snapshot_path(root.path(), 4).is_file());
        assert_eq!(
            get_repository_state(&control_db, "lib-1")
                .unwrap()
                .unwrap()
                .local_base_sha256,
            Some(digest)
        );
    }

    #[test]
    fn an_unrecorded_identity_never_trusts_the_working_copy_while_a_publication_is_unfinished() {
        for unfinished in [
            OperationState::Prepared,
            OperationState::Pending,
            OperationState::RetryWait,
            OperationState::Conflicted,
            OperationState::Failed,
        ] {
            let (root, control_db, digest) = working_copy();
            upsert_repository_state(&control_db, &state(LocalState::Clean, Some(digest))).unwrap();
            let mut op = operation(Some(4), "pre-op");
            op.state = unfinished;
            upsert_operation(&control_db, &op).unwrap();

            assert_eq!(
                ensure_base_identity(&control_db, "lib-1", root.path()).unwrap(),
                None,
                "{unfinished:?}"
            );
            assert!(
                !openkara_remote::base_snapshot_path(root.path(), 4).exists(),
                "{unfinished:?}"
            );
        }
    }

    #[test]
    fn a_working_copy_with_unpublished_changes_never_becomes_the_base() {
        let (root, control_db, digest) = working_copy();
        upsert_repository_state(&control_db, &state(LocalState::Dirty, Some(digest))).unwrap();

        assert_eq!(
            ensure_base_identity(&control_db, "lib-1", root.path()).unwrap(),
            None
        );
        assert!(!openkara_remote::base_snapshot_path(root.path(), 4).exists());
        assert_eq!(
            get_repository_state(&control_db, "lib-1")
                .unwrap()
                .unwrap()
                .local_base_sha256,
            None
        );
    }
}
