pub mod audio;
pub mod cache;
pub mod commands;
pub mod library;
pub mod metadata;
use std::path::PathBuf;
use tauri::Manager;

pub struct AppState {
    pub database_path: PathBuf,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let database_path = cache::initialize_database(app.handle())
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;

            // Commands open short-lived SQLite connections on demand. This avoids
            // sharing a long-lived connection across Tauri threads before we need
            // more advanced pooling behavior.
            app.manage(AppState { database_path });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::import::import_songs,
            commands::import::get_library,
            commands::import::search_library
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
