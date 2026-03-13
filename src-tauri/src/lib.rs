pub mod audio;
pub mod cache;
pub mod commands;
pub mod library;
pub mod lyrics;
pub mod metadata;
pub mod separator;
use crate::audio::playback::{monotonic_now_ms, PlaybackController};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::atomic::AtomicBool,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tauri::{Emitter, Manager};

pub struct AppState {
    pub database_path: PathBuf,
    pub cache_dir: PathBuf,
    pub model_path: PathBuf,
    pub playback: Arc<Mutex<PlaybackController>>,
    pub audio_output_started: Arc<AtomicBool>,
    pub audio_output_start_lock: Arc<Mutex<()>>,
    pub separation_statuses:
        Arc<Mutex<HashMap<String, commands::separation::SeparationStatusSnapshot>>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let database_path = cache::initialize_database(app.handle())
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let cache_dir = app
                .path()
                .app_cache_dir()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            fs::create_dir_all(&cache_dir)
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let playback = Arc::new(Mutex::new(PlaybackController::default()));
            let audio_output_started = Arc::new(AtomicBool::new(false));
            let audio_output_start_lock = Arc::new(Mutex::new(()));
            let separation_statuses = Arc::new(Mutex::new(HashMap::new()));

            // Commands open short-lived SQLite connections on demand. This avoids
            // sharing a long-lived connection across Tauri threads before we need
            // more advanced pooling behavior.
            app.manage(AppState {
                database_path,
                cache_dir,
                model_path: separator::model::default_model_path(),
                playback: Arc::clone(&playback),
                audio_output_started,
                audio_output_start_lock,
                separation_statuses,
            });
            spawn_playback_position_emitter(app.handle().clone(), playback);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::import::import_songs,
            commands::import::get_library,
            commands::import::search_library,
            commands::playback::play,
            commands::playback::pause,
            commands::playback::seek,
            commands::playback::set_volume,
            commands::playback::set_playback_mode,
            commands::playback::get_playback_state,
            commands::separation::separate,
            commands::separation::get_separation_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn spawn_playback_position_emitter(
    app_handle: tauri::AppHandle,
    playback: Arc<Mutex<PlaybackController>>,
) {
    thread::spawn(move || {
        let mut last_emitted_position = None;

        loop {
            thread::sleep(Duration::from_millis(16));

            let snapshot = match playback.lock() {
                Ok(mut controller) => controller.snapshot(monotonic_now_ms()),
                Err(_) => break,
            };

            if snapshot.song_id.is_none() {
                last_emitted_position = None;
                continue;
            }

            let should_emit = last_emitted_position != Some(snapshot.position_ms);
            if should_emit {
                let _ = app_handle.emit(
                    audio::playback::PLAYBACK_POSITION_EVENT,
                    audio::playback::PlaybackPositionEvent {
                        ms: snapshot.position_ms,
                    },
                );
                last_emitted_position = Some(snapshot.position_ms);
            }
        }
    });
}
