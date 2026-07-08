//! Deep library module: song domain model + local write path.
//!
//! - Domain types: [`Song`], import/delete result shapes
//! - Write path: [`import`], [`delete`], [`songs`], [`playlist`]
//! - Storage adapter: `crate::cache` (SQL only)
//!
//! IPC commands and remote Pre-Mutation Refresh / Publish Song wrappers live
//! outside this module and call into these APIs with `Connection` + `LibraryRoot`.

pub mod delete;
pub mod error;
pub mod import;
pub mod playlist;
pub mod songs;

use crate::commands::error::CommandError;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Song {
    pub hash: String,
    pub file_path: Option<String>,
    pub cdg_path: Option<String>,
    pub media_g_container: Option<String>,
    pub instrumental: bool,
    pub language: Option<String>,
    pub audio_source_kind: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: i64,
    pub cover_art: Option<Vec<u8>>,
    pub has_cover_art: bool,
    pub imported_at: i64,
    pub original_ext: Option<String>,
}

impl Song {
    pub fn is_media_g(&self) -> bool {
        self.media_g_container.is_some() || self.cdg_path.is_some()
    }

    pub fn is_instrumental(&self) -> bool {
        self.instrumental
    }

    pub fn is_separable(&self) -> bool {
        !self.is_media_g() && !self.is_instrumental()
    }

    pub fn is_media_g_zip(&self) -> bool {
        self.media_g_container.as_deref() == Some("zip")
    }

    pub fn is_remote(&self) -> bool {
        self.audio_source_kind != "original"
    }

    pub fn is_remote_stems(&self) -> bool {
        self.audio_source_kind == "stems_remote"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportFailure {
    pub path: String,
    pub error: CommandError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportSongsResult {
    pub imported: Vec<Song>,
    pub failed: Vec<ImportFailure>,
}

// Re-export the main write-path entry points for locality at the module root.
pub use delete::{
    delete_song_files_from_working_copy, delete_song_from_library,
    delete_song_rows_from_database, delete_stem_files_from_working_copy,
};
pub use import::{
    extract_embedded_cover_art_from_connection, get_library_from_connection,
    import_songs_from_paths, import_songs_from_paths_with_options, ImportSongsOptions,
};
pub use songs::{
    delete_songs, get_song_properties, set_songs_instrumental, set_songs_language,
    update_song_metadata,
};

#[cfg(test)]
mod tests {
    use super::Song;

    fn sample_song() -> Song {
        Song {
            hash: "song-1".to_owned(),
            file_path: Some("media/song-1.mp3".to_owned()),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: Some("Song".to_owned()),
            artist: None,
            album: None,
            duration_ms: 1_000,
            cover_art: None,
            has_cover_art: false,
            imported_at: 1,
            original_ext: Some("mp3".to_owned()),
        }
    }

    #[test]
    fn separable_songs_must_be_plain_audio_and_not_instrumental() {
        let plain_audio = sample_song();
        assert!(plain_audio.is_separable());

        let mut instrumental = sample_song();
        instrumental.instrumental = true;
        assert!(!instrumental.is_separable());

        let mut media_g = sample_song();
        media_g.cdg_path = Some("media-g/song-1.cdg".to_owned());
        assert!(!media_g.is_separable());
    }

    #[test]
    fn is_media_g_with_cdg_path() {
        let mut song = sample_song();
        song.cdg_path = Some("media-g/song.cdg".to_owned());
        assert!(song.is_media_g());
    }

    #[test]
    fn is_media_g_with_container() {
        let mut song = sample_song();
        song.media_g_container = Some("zip".to_owned());
        assert!(song.is_media_g());
    }

    #[test]
    fn is_media_g_zip_only_when_zip() {
        let mut song = sample_song();
        assert!(!song.is_media_g_zip());

        song.media_g_container = Some("zip".to_owned());
        assert!(song.is_media_g_zip());

        song.media_g_container = Some("folder".to_owned());
        assert!(!song.is_media_g_zip());
    }

    #[test]
    fn is_remote_when_not_original() {
        let mut song = sample_song();
        assert!(!song.is_remote());

        song.audio_source_kind = "stems_remote".to_owned();
        assert!(song.is_remote());

        song.audio_source_kind = "original".to_owned();
        assert!(!song.is_remote());
    }

    #[test]
    fn is_remote_stems_specifically() {
        let mut song = sample_song();
        assert!(!song.is_remote_stems());

        song.audio_source_kind = "stems_remote".to_owned();
        assert!(song.is_remote_stems());

        song.audio_source_kind = "other_kind".to_owned();
        assert!(!song.is_remote_stems());
    }

    #[test]
    fn is_instrumental_reflects_flag() {
        let mut song = sample_song();
        assert!(!song.is_instrumental());

        song.instrumental = true;
        assert!(song.is_instrumental());
    }
}
