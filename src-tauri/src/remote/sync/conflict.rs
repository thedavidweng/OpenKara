//! Exit paths from a Pre-Publish Conflict.
//!
//! The executor has implemented the two resolution strategies since the
//! publish protocol landed, but nothing ever called them: the repository could
//! enter `Conflicted`, Settings would render the word in red, and there was no
//! way back. A safety stop with no exit is not a safety stop.
//!
//! This module is the seam between the commands layer and
//! `executor::conflict_*`. It owns everything the executor deliberately does
//! not: locating the active remote repository, pulling the winning remote
//! database to a candidate path, and cleaning that candidate up afterwards.

use std::path::PathBuf;

use crate::commands::error::{CommandError, CommandResult};
use crate::config::RegisteredLibrary;
use crate::library::error::LibraryError;
use crate::remote::control_db::{get_repository_state, LocalState};
use crate::remote::executor::{
    conflict_keep_local_as_new_generation, conflict_use_remote, pull_conflict_candidate,
    PublishContext,
};
use crate::remote::provider::create_provider;
use crate::remote::types::load_app_config;
use crate::AppState;

/// What the user chose when told their repository changed underneath them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictResolution {
    /// Rebase the local pending changes onto the winning remote generation and
    /// publish them. Refused by the executor when the two sides touch the same
    /// songs — an automatic merge there would silently pick a winner.
    KeepLocal,
    /// Discard the local pending operation and adopt the remote database.
    UseRemote,
}

/// Resolve the conflict blocking the active remote repository.
pub fn resolve_active_remote_conflict(
    state: &AppState,
    resolution: ConflictResolution,
) -> CommandResult<()> {
    if state.remote.control_db_degraded {
        return Err(CommandError::from(LibraryError::Internal(
            "remote control database is unavailable; conflict resolution is \
             disabled until the control plane is repaired"
                .to_string(),
        )));
    }

    let config = load_app_config(&state.shell.app_data_dir)?;
    let Some(active_library) = config.active_library() else {
        return Err(CommandError::from(LibraryError::Internal(
            "no library is currently active".to_string(),
        )));
    };
    if !matches!(active_library, RegisteredLibrary::Remote { .. }) {
        return Err(CommandError::from(LibraryError::Internal(
            "the active library is not a remote repository".to_string(),
        )));
    }
    let library_id = active_library.id().to_owned();
    let working_copy_root = active_library.working_copy_root().ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "remote repository is missing a cached working copy".to_string(),
        ))
    })?;

    let control_db =
        state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;

    let repo_state = get_repository_state(&control_db, &library_id)?.ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "the repository has no recorded state".to_string(),
        ))
    })?;
    if repo_state.local_state != LocalState::Conflicted {
        return Err(CommandError::from(LibraryError::Internal(
            "the repository is not in a conflicted state".to_string(),
        )));
    }
    let operation_id = repo_state.active_operation_id.clone().ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "the conflicted repository has no operation to resolve".to_string(),
        ))
    })?;
    let (repository_id, writer_id) = (
        repo_state.repository_id.clone().unwrap_or_default(),
        repo_state.writer_id.clone().unwrap_or_default(),
    );

    let provider = create_provider(&state.shell.app_data_dir, active_library)?;
    let ctx = PublishContext {
        control_db: &control_db,
        provider: provider.as_ref(),
        working_copy_root: &working_copy_root,
        library_id: &library_id,
        writer_id: &writer_id,
        repository_id: &repository_id,
    };

    // Both strategies compare against the winning remote database, so it is
    // pulled to a candidate path rather than over the working copy — the local
    // side is still the user's only copy of their pending changes until they
    // choose to discard it.
    let candidate = candidate_path(&working_copy_root, &operation_id);
    let result =
        pull_conflict_candidate(provider.as_ref(), &candidate).and_then(|_| match resolution {
            ConflictResolution::KeepLocal => {
                conflict_keep_local_as_new_generation(&ctx, &operation_id, &candidate)
            }
            ConflictResolution::UseRemote => conflict_use_remote(&ctx, &operation_id, &candidate),
        });
    let _ = std::fs::remove_file(&candidate);
    result
}

fn candidate_path(working_copy_root: &std::path::Path, operation_id: &str) -> PathBuf {
    working_copy_root.join(format!(
        ".openkara/candidates/{operation_id}.conflict-candidate.sqlite"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_never_lands_on_the_working_database() {
        // The whole point of a candidate is that the user's pending changes
        // survive until they pick a side, so this path must not collide with
        // the working copy's own database.
        let root = std::path::Path::new("/tmp/repo");
        let candidate = candidate_path(root, "op-1");

        assert_ne!(candidate, root.join("openkara.db"));
        assert!(candidate.starts_with(root.join(".openkara/candidates")));
    }

    #[test]
    fn resolutions_deserialize_from_the_ipc_spelling() {
        assert_eq!(
            serde_json::from_str::<ConflictResolution>("\"keep_local\"").unwrap(),
            ConflictResolution::KeepLocal
        );
        assert_eq!(
            serde_json::from_str::<ConflictResolution>("\"use_remote\"").unwrap(),
            ConflictResolution::UseRemote
        );
    }
}
