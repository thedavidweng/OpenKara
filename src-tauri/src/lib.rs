#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod airplay_stream;
mod app_menu;
mod app_runtime;
pub mod audio;
pub mod cache;
pub mod cdg;
pub mod commands;
pub mod config;
pub mod hash;
pub mod library;
pub mod library_root;
pub mod lyrics;
pub mod media_g;
pub mod metadata;
pub mod perf;
pub mod remote;
pub mod separator;
pub mod services;
pub mod smoke;
pub mod state;
pub mod system_credentials;
mod window_shell;

pub use state::AppState;

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupModelBootstrapPlan {
    pub model_path: PathBuf,
    pub managed_model_path: PathBuf,
    pub status: commands::bootstrap::ModelBootstrapStatusSnapshot,
    pub should_spawn_bootstrap_worker: bool,
}

pub fn derive_startup_model_bootstrap(
    app_data_dir: &Path,
    development_model_path: &Path,
    active_variant: config::ModelVariant,
    expected_sha256: &str,
) -> anyhow::Result<StartupModelBootstrapPlan> {
    // Startup readiness must mean "the active variant has a verified model",
    // not merely "some file exists at the managed path". That distinction is
    // what prevents re-downloading already-installed models on every launch.
    let descriptor = separator::bootstrap::descriptor_for(active_variant);
    let managed_model_path = separator::bootstrap::managed_model_path_for(app_data_dir, descriptor);
    let resolution = separator::bootstrap::resolve_model_installation(
        &managed_model_path,
        development_model_path,
        expected_sha256,
    )?;
    let (model_path, status, should_spawn_bootstrap_worker) = match resolution {
        separator::bootstrap::ModelInstallationResolution::Ready(resolved) => (
            resolved.path.clone(),
            commands::bootstrap::ready_status(resolved.path.display().to_string()),
            false,
        ),
        separator::bootstrap::ModelInstallationResolution::LegacyManaged(path) => (
            path.clone(),
            commands::bootstrap::outdated_status(path.display().to_string()),
            false,
        ),
        separator::bootstrap::ModelInstallationResolution::Absent => (
            managed_model_path.clone(),
            commands::bootstrap::pending_status(managed_model_path.display().to_string()),
            true,
        ),
    };

    Ok(StartupModelBootstrapPlan {
        model_path,
        managed_model_path,
        status,
        should_spawn_bootstrap_worker,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| app_runtime::setup_app(app))
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap::get_model_bootstrap_status,
            commands::import::import_songs,
            commands::import::get_import_candidate_details,
            commands::import::expand_import_paths,
            commands::import::pick_import_paths,
            commands::import::get_library,
            commands::import::search_library,
            commands::import::get_cover_art,
            commands::import::delete_songs,
            commands::import::extract_embedded_cover_art,
            commands::import::update_song_metadata,
            commands::import::set_songs_instrumental,
            commands::import::set_songs_language,
            commands::import::get_song_properties,
            commands::library_setup::create_library,
            commands::library_setup::open_library,
            commands::library_setup::switch_library,
            commands::library_setup::get_library_path,
            commands::library_setup::get_library_registry,
            commands::library_setup::get_active_library,
            commands::library_setup::remove_library,
            commands::library_setup::rename_library,
            commands::library_setup::delete_library,
            commands::remote_library::begin_remote_auth,
            commands::remote_library::poll_remote_auth,
            commands::remote_library::cancel_remote_auth,
            commands::remote_library::open_external_url,
            commands::remote_library::list_remote_library_roots,
            commands::remote_library::create_remote_library,
            commands::remote_library::resolve_remote_library_candidate,
            commands::remote_library::register_remote_library,
            commands::remote_library::reauthorize_remote_library,
            commands::remote_library::mirror_local_library_to_remote,
            commands::remote_library::sync_active_remote_library,
            commands::remote_library::publish_song_to_remote,
            commands::remote_library::publish_songs_to_remote,
            commands::remote_library::get_all_upload_statuses,
            commands::lyrics::fetch_lyrics,
            commands::lyrics::set_lyrics_offset,
            commands::lyrics::save_manual_lyrics,
            commands::lyrics::import_lyrics_files,
            commands::lyrics::extract_embedded_lyrics,
            commands::lyrics::fetch_lyrics_online,
            commands::maintenance::delete_all_stems,
            commands::maintenance::estimate_stems_size,
            commands::maintenance::delete_all_cached_lyrics,
            commands::maintenance::downgrade_all_to_two_stem,
            commands::maintenance::estimate_downgrade_savings,
            commands::playback::play,
            commands::playback::resume,
            commands::playback::pause,
            commands::playback::seek,
            commands::playback::set_volume,
            commands::playback::set_stem_volume,
            commands::playback::load_stems,
            commands::playback::get_playback_state,
            commands::playback::get_audio_peaks,
            commands::playback::set_preload_candidate,
            commands::cdg::get_cdg_frame,
            commands::airplay::sync_airplay_route_picker,
            commands::airplay::sync_airplay_audience_state,
            commands::airplay::step_airplay_plain_text_page,
            commands::batch_separation::batch_separate,
            commands::batch_separation::cancel_batch_separation,
            commands::separation::separate,
            commands::separation::get_separation_status,
            commands::separation::get_all_separation_statuses,
            commands::separation::upgrade_to_four_stem,
            commands::separation::re_separate,
            commands::separation::downgrade_single_to_two_stem,
            commands::playlist::list_playlists,
            commands::playlist::create_playlist,
            commands::playlist::rename_playlist,
            commands::playlist::delete_playlist,
            commands::playlist::add_songs_to_playlist,
            commands::playlist::remove_songs_from_playlist,
            commands::playlist::get_playlist_songs,
            commands::playlist::set_rotation_state,
            commands::playlist::get_rotation_state,
            commands::playlist::advance_rotation,
            commands::playlist::set_queue_entry_singer,
            commands::settings::get_settings,
            commands::settings::set_stem_mode,
            commands::settings::set_model_variant,
            commands::settings::set_language,
            commands::settings::set_hide_batch_separate,
            commands::settings::set_cover_art_backdrop,
            commands::settings::set_lyrics_font_step,
            commands::settings::set_execution_provider,
            commands::settings::set_eq_enabled,
            commands::settings::set_eq_gains,
            commands::settings::set_library_sort_mode,
            commands::settings::restart_app,
            commands::window_shell::get_window_shell_state,
            commands::window_shell::set_native_sidebar_visibility,
            commands::window_shell::window_ready,
            commands::bootstrap::download_model,
            commands::bootstrap::delete_model,
            commands::bootstrap::get_model_status,
            commands::runtime_bootstrap::get_runtime_bootstrap_status,
            commands::runtime_bootstrap::download_runtime,
            commands::runtime_bootstrap::delete_runtime
        ]);

    #[cfg(target_os = "macos")]
    let builder = builder
        .menu(app_menu::build_app_menu)
        .on_menu_event(app_menu::handle_menu_event);

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
