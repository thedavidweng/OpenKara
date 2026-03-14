pub mod bootstrap;
pub mod error;
pub mod import;
pub mod library_setup;
pub mod lyrics;
pub mod playback;
pub mod separation;

pub use bootstrap::get_model_bootstrap_status;
pub use error::{CommandError, CommandResult, ErrorCode, FallbackAction};
pub use import::{get_library, import_songs, search_library};
pub use lyrics::{fetch_lyrics, set_lyrics_offset};
pub use playback::{get_playback_state, load_stems, pause, play, seek, set_stem_volume, set_volume};
pub use separation::{get_separation_status, separate};
