pub mod airplay;
pub mod batch_separation;
pub mod bootstrap;
pub mod cdg;
pub mod error;
pub mod import;
pub mod integrity;
pub mod library_setup;
pub mod lyrics;
pub mod maintenance;
pub mod playback;
pub mod playlist;
pub mod remote_library;
pub mod runtime_bootstrap;
pub mod separation;
pub mod settings;
pub mod window_shell;

pub use bootstrap::get_model_bootstrap_status;
pub use cdg::{get_cdg_frame, get_cdg_status};
pub use error::{current_unix_timestamp, CommandError, CommandResult, ErrorCode, FallbackAction};
pub use import::{delete_songs, get_library, import_songs, search_library};
pub use lyrics::{fetch_lyrics, set_lyrics_offset};
pub use playback::{
    get_audio_peaks, get_playback_state, load_stems, pause, play, seek, set_preload_candidate,
    set_stem_volume, set_volume,
};
pub use playlist::{
    add_songs_to_playlist, advance_rotation, create_playlist, delete_playlist, get_playlist_songs,
    get_rotation_state, list_playlists, remove_songs_from_playlist, rename_playlist,
    set_queue_entry_singer, set_rotation_state,
};
pub use separation::{get_separation_status, separate, upgrade_to_four_stem};
pub use settings::{get_settings, set_stem_mode};
