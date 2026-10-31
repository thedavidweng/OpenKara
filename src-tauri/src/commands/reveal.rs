use crate::cache;
use crate::catalog;
use crate::commands::error::{database_error, CommandResult};
use crate::AppState;
use tauri::State;

#[tauri::command]
pub fn get_reveal_targets(
    state: State<'_, AppState>,
    song_id: String,
) -> CommandResult<catalog::RevealTargets> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;
    catalog::get_reveal_targets(&connection, &library, &song_id).map_err(Into::into)
}

#[tauri::command]
pub fn reveal_in_folder(path: String) -> CommandResult<()> {
    catalog::reveal_path(&path).map_err(Into::into)
}
