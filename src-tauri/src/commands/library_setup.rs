use crate::{
    cache,
    commands::error::{internal_error, state_lock_error, CommandError, CommandResult},
    config::{self, AppConfig, RegisteredLibrary},
    library::error::LibraryError,
    library_root::LibraryRoot,
    AppState,
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::State;

#[derive(Debug, Clone, Serialize)]
pub struct LibraryRegistrySnapshot {
    pub active_library_id: Option<String>,
    pub libraries: Vec<RegisteredLibrary>,
}

fn canonical_path_string(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn load_app_config(app_data_dir: &Path) -> CommandResult<AppConfig> {
    Ok(config::load_config(app_data_dir)
        .map_err(internal_error)?
        .unwrap_or_default())
}

fn persist_app_config(app_data_dir: &Path, config: &AppConfig) -> CommandResult<()> {
    config::save_config(app_data_dir, config).map_err(internal_error)
}

fn update_library_display_name(
    app_data_dir: &Path,
    library_id: &str,
    display_name: &str,
) -> CommandResult<LibraryRegistrySnapshot> {
    let mut config = load_app_config(app_data_dir)?;
    let Some(library) = config
        .libraries
        .iter_mut()
        .find(|entry| entry.id() == library_id)
    else {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "library {library_id} was not found"
        ))));
    };

    match library {
        RegisteredLibrary::Local {
            display_name: name, ..
        }
        | RegisteredLibrary::Remote {
            display_name: name, ..
        } => {
            *name = display_name.to_owned();
        }
    }

    persist_app_config(app_data_dir, &config)?;

    Ok(LibraryRegistrySnapshot {
        active_library_id: config.active_library_id.clone(),
        libraries: config.libraries.clone(),
    })
}

fn delete_library_data(app_data_dir: &Path, library: &RegisteredLibrary) -> CommandResult<()> {
    match library {
        RegisteredLibrary::Local { root_path, .. } => {
            if Path::new(root_path).exists() {
                fs::remove_dir_all(root_path).map_err(|error| {
                    CommandError::from(LibraryError::Internal(format!(
                        "failed to delete {root_path}: {error}"
                    )))
                })?;
            }
        }
        RegisteredLibrary::Remote { .. } => {
            crate::remote::delete_remote_library_root(app_data_dir, library)?;
            if let Some(working_copy_root) = library.working_copy_root() {
                if working_copy_root.exists() {
                    fs::remove_dir_all(&working_copy_root).map_err(|error| {
                        CommandError::from(LibraryError::Internal(format!(
                            "failed to delete {}: {error}",
                            working_copy_root.display()
                        )))
                    })?;
                }
            }
        }
    }

    Ok(())
}

fn upsert_library(config: &mut AppConfig, library: RegisteredLibrary) {
    if let Some(existing) = config
        .libraries
        .iter_mut()
        .find(|entry| entry.id() == library.id())
    {
        *existing = library;
    } else {
        config.libraries.push(library);
    }
}

fn set_active_library(config: &mut AppConfig, library_id: String) {
    config.active_library_id = Some(library_id);
    config.library_path = None;
}

fn store_active_library(
    state: &State<'_, AppState>,
    config: &mut AppConfig,
    library: LibraryRoot,
) -> CommandResult<()> {
    let mut guard = state
        .shell
        .library
        .lock()
        .map_err(|_| state_lock_error("library lock was poisoned"))?;
    *guard = Some(library);
    config.library_path = None;
    Ok(())
}

fn clear_library_scoped_runtime_state(state: &State<'_, AppState>) -> CommandResult<()> {
    {
        let mut playback = state
            .playback
            .playback
            .lock()
            .map_err(|_| state_lock_error("playback controller lock was poisoned"))?;
        playback.clear_track();
    }
    {
        let mut cdg_state = state
            .playback
            .cdg_state
            .lock()
            .map_err(|_| state_lock_error("CDG state lock was poisoned"))?;
        *cdg_state = None;
    }
    {
        let mut upload_statuses = state
            .remote
            .remote_upload_statuses
            .lock()
            .map_err(|_| state_lock_error("remote upload status lock was poisoned"))?;
        upload_statuses.clear();
    }
    Ok(())
}

fn register_library(
    state: &State<'_, AppState>,
    app_data_dir: &Path,
    library: RegisteredLibrary,
    root: LibraryRoot,
) -> CommandResult<LibraryRegistrySnapshot> {
    let db_path = root.database_path();
    cache::initialize_library_database(&db_path)
        .map_err(|e| CommandError::from(LibraryError::DatabaseUnavailable(e.to_string())))?;

    let mut config = load_app_config(app_data_dir)?;
    upsert_library(&mut config, library.clone());
    set_active_library(&mut config, library.id().to_owned());
    persist_app_config(app_data_dir, &config)?;

    store_active_library(state, &mut config, root)?;
    {
        let mut upload_statuses = state
            .remote
            .remote_upload_statuses
            .lock()
            .map_err(|_| state_lock_error("remote upload status lock was poisoned"))?;
        upload_statuses.clear();
    }

    Ok(LibraryRegistrySnapshot {
        active_library_id: config.active_library_id.clone(),
        libraries: config.libraries.clone(),
    })
}

fn activate_library(
    state: &State<'_, AppState>,
    app_data_dir: &Path,
    library_id: &str,
) -> CommandResult<LibraryRegistrySnapshot> {
    let mut config = load_app_config(app_data_dir)?;
    let library = config
        .libraries
        .iter()
        .find(|entry| entry.id() == library_id)
        .cloned()
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "library {library_id} was not found"
            )))
        })?;

    let root_path = library.working_copy_root().ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "remote repository is missing a cached working copy".to_string(),
        ))
    })?;
    let lib = LibraryRoot::open(&root_path).map_err(internal_error)?;
    let db_path = lib.database_path();
    cache::initialize_library_database(&db_path)
        .map_err(|e| CommandError::from(LibraryError::DatabaseUnavailable(e.to_string())))?;

    clear_library_scoped_runtime_state(state)?;

    set_active_library(&mut config, library.id().to_owned());
    persist_app_config(app_data_dir, &config)?;
    store_active_library(state, &mut config, lib)?;

    Ok(LibraryRegistrySnapshot {
        active_library_id: config.active_library_id.clone(),
        libraries: config.libraries.clone(),
    })
}

#[tauri::command]
pub fn create_library(
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<LibraryRegistrySnapshot> {
    let lib_path = PathBuf::from(&path);

    let lib = LibraryRoot::create(&lib_path).map_err(internal_error)?;
    let canonical_root = canonical_path_string(lib.root());
    let library = RegisteredLibrary::local(
        canonical_root.clone(),
        config::library_display_name(&canonical_root),
    );

    register_library(&state, &state.shell.app_data_dir, library, lib)
}

#[tauri::command]
pub fn open_library(
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<LibraryRegistrySnapshot> {
    let lib_path = PathBuf::from(&path);

    let lib = LibraryRoot::open(&lib_path).map_err(internal_error)?;
    let canonical_root = canonical_path_string(lib.root());
    let library = RegisteredLibrary::local(
        canonical_root.clone(),
        config::library_display_name(&canonical_root),
    );

    register_library(&state, &state.shell.app_data_dir, library, lib)
}

#[tauri::command]
pub fn switch_library(
    state: State<'_, AppState>,
    library_id: String,
) -> CommandResult<LibraryRegistrySnapshot> {
    activate_library(&state, &state.shell.app_data_dir, &library_id)
}

#[tauri::command]
pub fn get_library_path(state: State<'_, AppState>) -> CommandResult<Option<String>> {
    let guard = state
        .shell
        .library
        .lock()
        .map_err(|_| state_lock_error("library lock was poisoned"))?;

    Ok(guard
        .as_ref()
        .map(|lib: &LibraryRoot| canonical_path_string(lib.root())))
}

#[tauri::command]
pub fn get_library_registry(state: State<'_, AppState>) -> CommandResult<LibraryRegistrySnapshot> {
    // Use the resolved shell app-data path (respects OPENKARA_APP_DATA_DIR under
    // automation-smoke). Tauri's path().app_data_dir() ignores that override and
    // makes the frontend think no library is registered.
    let config = load_app_config(&state.shell.app_data_dir)?;

    Ok(LibraryRegistrySnapshot {
        active_library_id: config.active_library_id.clone(),
        libraries: config.libraries.clone(),
    })
}

#[tauri::command]
pub fn get_active_library(state: State<'_, AppState>) -> CommandResult<Option<RegisteredLibrary>> {
    let config = load_app_config(&state.shell.app_data_dir)?;

    Ok(config.active_library().cloned())
}

#[tauri::command]
pub fn remove_library(
    state: State<'_, AppState>,
    library_id: String,
) -> CommandResult<LibraryRegistrySnapshot> {
    let app_data_dir = state.shell.app_data_dir.clone();
    let mut config = load_app_config(&app_data_dir)?;
    let removed_active = config.active_library_id.as_deref() == Some(library_id.as_str());
    let removed_libraries: Vec<_> = config
        .libraries
        .iter()
        .filter(|library| library.id() == library_id)
        .cloned()
        .collect();
    config
        .libraries
        .retain(|library| library.id() != library_id);

    for library in &removed_libraries {
        crate::remote::remove_remote_library_credentials(&app_data_dir, library)?;
    }

    if removed_active {
        config.active_library_id = config
            .libraries
            .first()
            .map(|library| library.id().to_owned());
    }

    persist_app_config(&app_data_dir, &config)?;

    if config.active_library_id.is_none() {
        let mut guard = state
            .shell
            .library
            .lock()
            .map_err(|_| state_lock_error("library lock was poisoned"))?;
        *guard = None;
        clear_library_scoped_runtime_state(&state)?;
    } else if removed_active {
        activate_library(
            &state,
            &app_data_dir,
            config.active_library_id.as_deref().unwrap_or_default(),
        )?;
    }

    Ok(LibraryRegistrySnapshot {
        active_library_id: config.active_library_id.clone(),
        libraries: config.libraries.clone(),
    })
}

#[tauri::command]
pub fn rename_library(
    state: State<'_, AppState>,
    library_id: String,
    display_name: String,
) -> CommandResult<LibraryRegistrySnapshot> {
    update_library_display_name(&state.shell.app_data_dir, &library_id, &display_name)
}

#[tauri::command]
pub fn delete_library(
    state: State<'_, AppState>,
    library_id: String,
) -> CommandResult<LibraryRegistrySnapshot> {
    let app_data_dir = state.shell.app_data_dir.clone();
    let config = load_app_config(&app_data_dir)?;
    let Some(library) = config
        .libraries
        .iter()
        .find(|entry| entry.id() == library_id)
        .cloned()
    else {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "library {library_id} was not found"
        ))));
    };

    delete_library_data(&app_data_dir, &library)?;
    remove_library(state, library_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_local_library_sets_active_library() {
        let mut config = AppConfig::default();
        let library = RegisteredLibrary::local("/tmp/library".to_owned(), "library".to_owned());
        upsert_library(&mut config, library.clone());
        set_active_library(&mut config, library.id().to_owned());
        assert_eq!(config.active_library_id.as_deref(), Some(library.id()));
        assert_eq!(config.libraries.len(), 1);
    }

    #[test]
    fn register_library_replaces_existing_entry_with_same_id() {
        let mut config = AppConfig::default();
        let first = RegisteredLibrary::local("/tmp/library".to_owned(), "one".to_owned());
        let second = RegisteredLibrary::local("/tmp/library".to_owned(), "two".to_owned());
        upsert_library(&mut config, first);
        upsert_library(&mut config, second.clone());
        assert_eq!(config.libraries.len(), 1);
        assert_eq!(config.libraries[0].display_name(), second.display_name());
    }
}
