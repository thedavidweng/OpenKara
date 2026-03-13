#[derive(Debug, Clone, PartialEq, Eq)]
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
