pub mod audio;
pub mod cache;
pub mod commands;
pub mod library;
pub mod lyrics;
pub mod metadata;
pub mod perf;
pub mod separator;
use crate::audio::playback::{
    monotonic_now_ms, PlaybackController, PLAYBACK_POSITION_POLL_INTERVAL_MS,
};
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
    pub model_bootstrap_status: Arc<Mutex<commands::bootstrap::ModelBootstrapStatusSnapshot>>,
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
            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            fs::create_dir_all(&app_data_dir)
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let playback = Arc::new(Mutex::new(PlaybackController::default()));
            let audio_output_started = Arc::new(AtomicBool::new(false));
            let audio_output_start_lock = Arc::new(Mutex::new(()));
            let managed_model_path = separator::bootstrap::managed_model_path(&app_data_dir);
            let development_model_path = separator::model::default_model_path();
            let resolved_model = separator::bootstrap::resolve_existing_model_path(
                &managed_model_path,
                &development_model_path,
                separator::bootstrap::MODEL_SHA256,
            )
            .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let model_path = resolved_model
                .as_ref()
                .map(|resolved| resolved.path.clone())
                .unwrap_or_else(|| managed_model_path.clone());
            let model_bootstrap_status = Arc::new(Mutex::new(match resolved_model {
                Some(resolved) => {
                    commands::bootstrap::ready_status(resolved.path.display().to_string())
                }
                None => {
                    commands::bootstrap::pending_status(managed_model_path.display().to_string())
                }
            }));
            let separation_statuses = Arc::new(Mutex::new(HashMap::new()));

            // Commands open short-lived SQLite connections on demand. This avoids
            // sharing a long-lived connection across Tauri threads before we need
            // more advanced pooling behavior.
            app.manage(AppState {
                database_path,
                cache_dir,
                model_path: model_path.clone(),
                playback: Arc::clone(&playback),
                audio_output_started,
                audio_output_start_lock,
                model_bootstrap_status: Arc::clone(&model_bootstrap_status),
                separation_statuses,
            });
            spawn_playback_position_emitter(app.handle().clone(), playback);
            if model_path == managed_model_path {
                spawn_model_bootstrap_worker(
                    app.handle().clone(),
                    managed_model_path,
                    model_bootstrap_status,
                );
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap::get_model_bootstrap_status,
            commands::import::import_songs,
            commands::import::get_library,
            commands::import::search_library,
            commands::lyrics::fetch_lyrics,
            commands::lyrics::set_lyrics_offset,
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
            thread::sleep(Duration::from_millis(PLAYBACK_POSITION_POLL_INTERVAL_MS));

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

fn spawn_model_bootstrap_worker(
    app_handle: tauri::AppHandle,
    model_path: PathBuf,
    status: Arc<Mutex<commands::bootstrap::ModelBootstrapStatusSnapshot>>,
) {
    let progress_path = model_path.display().to_string();
    tauri::async_runtime::spawn(async move {
        let blocking_status = Arc::clone(&status);
        let blocking_app_handle = app_handle.clone();
        let blocking_model_path = model_path.clone();
        let progress_path = progress_path.clone();

        let result = tauri::async_runtime::spawn_blocking(move || {
            separator::bootstrap::download_and_install_model(
                &blocking_model_path,
                separator::bootstrap::MODEL_DOWNLOAD_URL,
                separator::bootstrap::MODEL_SHA256,
                |downloaded_bytes, total_bytes| {
                    let snapshot = commands::bootstrap::downloading_status(
                        progress_path.clone(),
                        downloaded_bytes,
                        total_bytes,
                    );
                    if let Ok(mut current) = blocking_status.lock() {
                        *current = snapshot.clone();
                    }
                    let _ = blocking_app_handle.emit(
                        commands::bootstrap::MODEL_BOOTSTRAP_PROGRESS_EVENT,
                        snapshot,
                    );
                },
            )
        })
        .await;

        match result {
            Ok(Ok(())) => {
                let snapshot = commands::bootstrap::ready_status(model_path.display().to_string());
                if let Ok(mut current) = status.lock() {
                    *current = snapshot.clone();
                }
                let _ = app_handle.emit(commands::bootstrap::MODEL_BOOTSTRAP_READY_EVENT, snapshot);
            }
            Ok(Err(error)) => {
                let command_error = commands::error::model_bootstrap_error(error.to_string());
                let snapshot = commands::bootstrap::failed_status(
                    model_path.display().to_string(),
                    command_error,
                );
                if let Ok(mut current) = status.lock() {
                    *current = snapshot.clone();
                }
                let _ = app_handle.emit(commands::bootstrap::MODEL_BOOTSTRAP_ERROR_EVENT, snapshot);
            }
            Err(error) => {
                let command_error = commands::error::model_bootstrap_error(error.to_string());
                let snapshot = commands::bootstrap::failed_status(
                    model_path.display().to_string(),
                    command_error,
                );
                if let Ok(mut current) = status.lock() {
                    *current = snapshot.clone();
                }
                let _ = app_handle.emit(commands::bootstrap::MODEL_BOOTSTRAP_ERROR_EVENT, snapshot);
            }
        }
    });
}
