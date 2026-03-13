use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Song {
    pub hash: String,
    pub file_path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: i64,
    pub cover_art: Option<Vec<u8>>,
    pub imported_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportFailure {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportSongsResult {
    pub imported: Vec<Song>,
    pub failed: Vec<ImportFailure>,
}
