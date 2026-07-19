//! Remote-mutation orchestration: wraps local DB mutations with the
//! Pre-Mutation Refresh / Pre-Publish Conflict / Publish Changes protocol
//! defined in `docs/references/contracts/library.md` and `CONTEXT.md`.
//!
//! # Why six entry points, not one
//!
//! An earlier architecture review suggested collapsing these into a single
//! `run_mutation(closure, manifest)` that inspects a manifest to decide
//! whether to prepare / sync_db / publish_song / publish_songs / mirror.
//! That would *not* deepen this module — it would replace a typed interface
//! with an untyped manifest, moving the caller's choice from "pick the right
//! fn" to "construct the right manifest struct" with less type safety and
//! no internal decision the module can make on its own.
//!
//! Each entry point encodes a real protocol variant that the caller knows
//! and the module cannot infer:
//!
//! | Wrapper                              | prepare | sync_db | publish         | When                                   |
//! |--------------------------------------|---------|---------|-----------------|----------------------------------------|
//! | `run_imported_songs_mutation`        | yes     | no      | songs (imported)| additive import; publish uploads DB    |
//! | `run_updated_songs_mutation`         | yes     | no      | songs (extracted)| metadata update; publish uploads DB   |
//! | `run_song_database_mutation`         | yes     | yes     | single song     | single-song DB-level change            |
//! | `run_song_database_mutation_with_result` | yes | yes     | song from result| same, song id only known after mutation|
//! | `run_songs_database_mutation`        | yes     | yes     | songs (extracted)| multi-song DB-level change            |
//! | `run_active_library_mirror_mutation` | no      | no      | mirror          | whole-library re-sync (e.g. maintenance)|
//! | `run_database_then_library_mirror_mutation` | yes | yes | mirror        | DB change + whole-library re-sync      |
//!
//! The deletion test confirms the set earns its keep: inlining
//! `prepare → mutate → sync_db → publish` at 18 call sites would scatter
//! the Pre-Mutation Refresh / Pre-Publish Conflict protocol across the
//! command layer. The typed wrappers concentrate it here.

use crate::{commands::error::CommandResult, library::Song, AppState};
use tauri::AppHandle;

// ---------------------------------------------------------------------------
// sync_backend: production delegates to sync::, test uses thread-local mock
// ---------------------------------------------------------------------------

#[cfg(not(test))]
mod sync_backend {
    use super::super::sync;
    use crate::commands::error::CommandResult;
    use crate::AppState;
    use std::path::Path;
    use tauri::AppHandle;

    pub fn prepare(app_data_dir: &Path) -> CommandResult<()> {
        sync::prepare_active_remote_database_for_mutation(app_data_dir)
    }

    pub fn sync_db(app_data_dir: &Path) -> CommandResult<()> {
        sync::sync_active_remote_database_if_needed(app_data_dir)
    }

    pub fn publish_song<R: tauri::Runtime>(
        state: &AppState,
        app_handle: &AppHandle<R>,
        song_id: &str,
    ) -> CommandResult<()> {
        sync::maybe_publish_song_to_bound_remote(state, app_handle, song_id)
    }

    pub fn publish_songs<R: tauri::Runtime>(
        state: &AppState,
        app_handle: &AppHandle<R>,
        song_ids: &[String],
    ) -> CommandResult<()> {
        sync::maybe_publish_songs_to_bound_remote(state, app_handle, song_ids)
    }

    pub fn mirror<R: tauri::Runtime>(
        state: &AppState,
        app_handle: &AppHandle<R>,
    ) -> CommandResult<()> {
        sync::sync_bound_remote_for_active_local_library(state, app_handle)
    }
}

#[cfg(test)]
mod sync_backend {
    use crate::commands::error::{CommandError, CommandResult};
    use crate::AppState;
    use std::cell::RefCell;
    use std::path::Path;
    use tauri::AppHandle;

    #[derive(Debug, Clone, PartialEq)]
    pub(super) enum SyncCall {
        Prepare,
        SyncDb,
        PublishSong(String),
        PublishSongs(Vec<String>),
        Mirror,
    }

    thread_local! {
        static CALLS: RefCell<Vec<SyncCall>> = const { RefCell::new(Vec::new()) };
        static PREPARE_RESULT: RefCell<Result<(), CommandError>> = const { RefCell::new(Ok(())) };
        static SYNC_DB_RESULT: RefCell<Result<(), CommandError>> = const { RefCell::new(Ok(())) };
        static PUBLISH_RESULT: RefCell<Result<(), CommandError>> = const { RefCell::new(Ok(())) };
        static MIRROR_RESULT: RefCell<Result<(), CommandError>> = const { RefCell::new(Ok(())) };
    }

    pub fn reset() {
        CALLS.with(|c| c.borrow_mut().clear());
        PREPARE_RESULT.with(|r| *r.borrow_mut() = Ok(()));
        SYNC_DB_RESULT.with(|r| *r.borrow_mut() = Ok(()));
        PUBLISH_RESULT.with(|r| *r.borrow_mut() = Ok(()));
        MIRROR_RESULT.with(|r| *r.borrow_mut() = Ok(()));
    }

    pub fn calls() -> Vec<SyncCall> {
        CALLS.with(|c| c.borrow().clone())
    }

    pub fn set_prepare_result(result: Result<(), CommandError>) {
        PREPARE_RESULT.with(|r| *r.borrow_mut() = result);
    }

    pub fn set_sync_db_result(result: Result<(), CommandError>) {
        SYNC_DB_RESULT.with(|r| *r.borrow_mut() = result);
    }

    #[allow(dead_code)]
    pub fn set_publish_result(result: Result<(), CommandError>) {
        PUBLISH_RESULT.with(|r| *r.borrow_mut() = result);
    }

    #[allow(dead_code)]
    pub fn set_mirror_result(result: Result<(), CommandError>) {
        MIRROR_RESULT.with(|r| *r.borrow_mut() = result);
    }

    pub fn prepare(_app_data_dir: &Path) -> CommandResult<()> {
        CALLS.with(|c| c.borrow_mut().push(SyncCall::Prepare));
        PREPARE_RESULT.with(|r| r.borrow().clone())
    }

    pub fn sync_db(_app_data_dir: &Path) -> CommandResult<()> {
        CALLS.with(|c| c.borrow_mut().push(SyncCall::SyncDb));
        SYNC_DB_RESULT.with(|r| r.borrow().clone())
    }

    pub fn publish_song<R: tauri::Runtime>(
        _state: &AppState,
        _app_handle: &AppHandle<R>,
        song_id: &str,
    ) -> CommandResult<()> {
        CALLS.with(|c| {
            c.borrow_mut()
                .push(SyncCall::PublishSong(song_id.to_owned()))
        });
        PUBLISH_RESULT.with(|r| r.borrow().clone())
    }

    pub fn publish_songs<R: tauri::Runtime>(
        _state: &AppState,
        _app_handle: &AppHandle<R>,
        song_ids: &[String],
    ) -> CommandResult<()> {
        CALLS.with(|c| {
            c.borrow_mut()
                .push(SyncCall::PublishSongs(song_ids.to_vec()))
        });
        PUBLISH_RESULT.with(|r| r.borrow().clone())
    }

    pub fn mirror<R: tauri::Runtime>(
        _state: &AppState,
        _app_handle: &AppHandle<R>,
    ) -> CommandResult<()> {
        CALLS.with(|c| c.borrow_mut().push(SyncCall::Mirror));
        MIRROR_RESULT.with(|r| r.borrow().clone())
    }
}

// ---------------------------------------------------------------------------
// Mutation functions — orchestrate prepare / mutate / sync / publish
// ---------------------------------------------------------------------------

/// Shared prefix for every mutation that starts with a Pre-Mutation Refresh:
/// `prepare → mutation`. Centralizes the `app_data_dir` extraction so a
/// future change to how the active remote is resolved touches one place.
fn prepare_and_mutate<T, F>(state: &AppState, mutation: F) -> CommandResult<T>
where
    F: FnOnce() -> CommandResult<T>,
{
    sync_backend::prepare(&state.shell.app_data_dir)?;
    mutation()
}

pub(crate) fn run_imported_songs_mutation<R, F>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
) -> CommandResult<crate::library::ImportSongsResult>
where
    R: tauri::Runtime,
    F: FnOnce() -> crate::library::ImportSongsResult,
{
    // ImportSongsResult is not a CommandResult, so call prepare directly.
    sync_backend::prepare(&state.shell.app_data_dir)?;
    let result = mutation();
    let imported_song_ids: Vec<String> = result
        .imported
        .iter()
        .map(|song| song.hash.clone())
        .collect();
    sync_backend::publish_songs(state, app_handle, &imported_song_ids)?;
    Ok(result)
}

pub(crate) fn run_updated_songs_mutation<R, T, F, S>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
    updated_song_ids: S,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce() -> CommandResult<T>,
    S: FnOnce(&T) -> Vec<String>,
{
    let result = prepare_and_mutate(state, mutation)?;
    let song_ids = updated_song_ids(&result);
    sync_backend::publish_songs(state, app_handle, &song_ids)?;
    Ok(result)
}

pub(crate) fn publish_song_to_active_remote_if_ready<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: &str,
) -> CommandResult<()> {
    sync_backend::publish_song(state, app_handle, song_id)
}

pub(crate) fn run_song_database_mutation<R, T, F>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: &str,
    mutation: F,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce() -> CommandResult<T>,
{
    let result = prepare_and_mutate(state, mutation)?;
    sync_backend::sync_db(&state.shell.app_data_dir)?;
    sync_backend::publish_song(state, app_handle, song_id)?;
    Ok(result)
}

pub(crate) fn run_song_database_mutation_with_result<R, T, F, S>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
    song_id: S,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce() -> CommandResult<T>,
    S: FnOnce(&T) -> Option<String>,
{
    let result = prepare_and_mutate(state, mutation)?;
    if let Some(song_id) = song_id(&result) {
        sync_backend::sync_db(&state.shell.app_data_dir)?;
        sync_backend::publish_song(state, app_handle, &song_id)?;
    }
    Ok(result)
}

pub(crate) fn run_songs_database_mutation<R, T, F, S>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
    song_ids: S,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce() -> CommandResult<T>,
    S: FnOnce(&T) -> Vec<String>,
{
    let result = prepare_and_mutate(state, mutation)?;
    let song_ids = song_ids(&result);
    if !song_ids.is_empty() {
        sync_backend::sync_db(&state.shell.app_data_dir)?;
        sync_backend::publish_songs(state, app_handle, &song_ids)?;
    }
    Ok(result)
}

pub(crate) fn run_active_library_mirror_mutation<R, T, F>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce() -> CommandResult<T>,
{
    // Mirror-only: no Pre-Mutation Refresh — a mirror is itself a full
    // re-sync and is used for maintenance operations that rebuild remote
    // state from the local working copy.
    let result = mutation()?;
    sync_backend::mirror(state, app_handle)?;
    Ok(result)
}

pub(crate) fn run_database_then_library_mirror_mutation<R, T, F>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce() -> CommandResult<T>,
{
    let result = prepare_and_mutate(state, mutation)?;
    sync_backend::sync_db(&state.shell.app_data_dir)?;
    sync_backend::mirror(state, app_handle)?;
    Ok(result)
}

pub(crate) fn song_ids_from_songs(songs: &[Song]) -> Vec<String> {
    songs.iter().map(|song| song.hash.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::error::internal_error;
    use crate::library::{ImportSongsResult, Song};
    use sync_backend::{calls, reset, SyncCall};

    fn test_state() -> AppState {
        AppState::test_fixture()
    }

    fn test_app_handle() -> AppHandle<impl tauri::Runtime> {
        tauri::test::mock_app().handle().clone()
    }

    fn song(hash: &str) -> Song {
        Song {
            hash: hash.to_owned(),
            file_path: None,
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: None,
            artist: None,
            album: None,
            duration_ms: 0,
            cover_art: None,
            has_cover_art: false,
            imported_at: 0,
            original_ext: None,
        }
    }

    // -- imported songs --

    #[test]
    fn imported_songs_prepares_then_publishes_hashes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        let result = run_imported_songs_mutation(&state, &handle, || ImportSongsResult {
            imported: vec![song("a"), song("b")],
            failed: vec![],
        })
        .unwrap();

        assert_eq!(result.imported.len(), 2);
        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::PublishSongs(vec!["a".into(), "b".into()]),
            ]
        );
    }

    // -- updated songs --

    #[test]
    fn updated_songs_prepares_then_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        let result = run_updated_songs_mutation(
            &state,
            &handle,
            || Ok(42u32),
            |_| vec!["x".into(), "y".into()],
        )
        .unwrap();

        assert_eq!(result, 42);
        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::PublishSongs(vec!["x".into(), "y".into()]),
            ]
        );
    }

    // -- publish_song_to_active_remote_if_ready --

    #[test]
    fn publish_song_delegates_directly() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        publish_song_to_active_remote_if_ready(&state, &handle, "song-42").unwrap();

        assert_eq!(calls(), vec![SyncCall::PublishSong("song-42".into())]);
    }

    // -- song database mutation --

    #[test]
    fn song_database_mutation_prepares_syncs_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_song_database_mutation(&state, &handle, "s1", || Ok("val")).unwrap();

        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::SyncDb,
                SyncCall::PublishSong("s1".into()),
            ]
        );
    }

    // -- song database mutation with result --

    #[test]
    fn mutation_with_result_some_id_syncs_and_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_song_database_mutation_with_result(
            &state,
            &handle,
            || Ok("out"),
            |_| Some("resolved-id".into()),
        )
        .unwrap();

        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::SyncDb,
                SyncCall::PublishSong("resolved-id".into()),
            ]
        );
    }

    #[test]
    fn mutation_with_result_none_skips_sync_and_publish() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_song_database_mutation_with_result(&state, &handle, || Ok("out"), |_| None).unwrap();

        assert_eq!(calls(), vec![SyncCall::Prepare]);
    }

    // -- songs database mutation --

    #[test]
    fn songs_database_mutation_nonempty_syncs_and_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_songs_database_mutation(&state, &handle, || Ok(()), |_| vec!["a".into(), "b".into()])
            .unwrap();

        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::SyncDb,
                SyncCall::PublishSongs(vec!["a".into(), "b".into()]),
            ]
        );
    }

    #[test]
    fn songs_database_mutation_empty_skips_sync_and_publish() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_songs_database_mutation(&state, &handle, || Ok(()), |_| vec![]).unwrap();

        assert_eq!(calls(), vec![SyncCall::Prepare]);
    }

    // -- mirror mutations --

    #[test]
    fn active_library_mirror_just_mirrors() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_active_library_mirror_mutation(&state, &handle, || Ok(99u32)).unwrap();

        assert_eq!(calls(), vec![SyncCall::Mirror]);
    }

    #[test]
    fn database_then_library_mirror_full_sequence() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_database_then_library_mirror_mutation(&state, &handle, || Ok("done")).unwrap();

        assert_eq!(
            calls(),
            vec![SyncCall::Prepare, SyncCall::SyncDb, SyncCall::Mirror]
        );
    }

    // -- error propagation --

    #[test]
    fn prepare_error_skips_mutation_and_publish() {
        reset();
        let state = test_state();
        let handle = test_app_handle();
        let mutation_ran = std::cell::Cell::new(false);

        sync_backend::set_prepare_result(Err(internal_error("prepare failed")));

        let err = run_updated_songs_mutation(
            &state,
            &handle,
            || {
                mutation_ran.set(true);
                Ok(())
            },
            |_| vec!["never".into()],
        )
        .unwrap_err();

        assert_eq!(err.message, "prepare failed");
        assert!(!mutation_ran.get());
        assert_eq!(calls(), vec![SyncCall::Prepare]);
    }

    #[test]
    fn sync_db_error_returns_without_publishing() {
        reset();
        let state = test_state();
        let handle = test_app_handle();
        let mutation_ran = std::cell::Cell::new(false);

        sync_backend::set_sync_db_result(Err(internal_error("sync failed")));

        let err = run_song_database_mutation(&state, &handle, "s1", || {
            mutation_ran.set(true);
            Ok(())
        })
        .unwrap_err();

        assert_eq!(err.message, "sync failed");
        assert!(mutation_ran.get());
        assert_eq!(calls(), vec![SyncCall::Prepare, SyncCall::SyncDb]);
    }
}
