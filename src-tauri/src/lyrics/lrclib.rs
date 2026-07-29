use crate::lyrics::error::LyricsError;
use serde::Deserialize;

use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://lrclib.net";
const USER_AGENT: &str = concat!("OpenKara/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricsLookupQuery {
    pub track_name: String,
    pub artist_name: String,
    pub album_name: Option<String>,
    pub duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LrcLibLyrics {
    pub id: i64,
    pub name: Option<String>,
    pub track_name: String,
    pub artist_name: String,
    pub album_name: Option<String>,
    #[serde(rename = "duration")]
    pub duration_seconds: f64,
    pub instrumental: bool,
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LrcLibClient {
    base_url: String,
    http: reqwest::blocking::Client,
    http_async: reqwest::Client,
}

impl LrcLibClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            http: reqwest::blocking::Client::builder()
                .user_agent(USER_AGENT)
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(6))
                .build()
                .expect("reqwest blocking client should build"),
            http_async: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(6))
                .build()
                .expect("reqwest async client should build"),
        }
    }

    pub fn new_default() -> Self {
        Self::new(DEFAULT_BASE_URL)
    }

    pub fn fetch_by_track(
        &self,
        query: &LyricsLookupQuery,
    ) -> Result<Option<LrcLibLyrics>, LyricsError> {
        let url = format!("{}/api/get", self.base_url);
        let mut request = self.http.get(url).query(&[
            ("track_name", query.track_name.as_str()),
            ("artist_name", query.artist_name.as_str()),
        ]);

        if let Some(album_name) = query.album_name.as_deref() {
            request = request.query(&[("album_name", album_name)]);
        }

        if let Some(duration_seconds) = query.duration_seconds {
            request = request.query(&[("duration", duration_seconds)]);
        }

        let response = request.send().map_err(|e| {
            LyricsError::NetworkUnavailable(format!("failed to request lyrics from LRCLIB: {e}"))
        })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let response = response.error_for_status().map_err(|e| {
            LyricsError::NetworkUnavailable(format!("LRCLIB returned a non-success response: {e}"))
        })?;
        let lyrics = response.json::<LrcLibLyrics>().map_err(|e| {
            LyricsError::NetworkUnavailable(format!(
                "failed to deserialize LRCLIB lyrics response: {e}"
            ))
        })?;

        Ok(Some(lyrics))
    }

    pub async fn fetch_by_track_async(
        &self,
        query: &LyricsLookupQuery,
    ) -> Result<Option<LrcLibLyrics>, LyricsError> {
        let url = format!("{}/api/get", self.base_url);
        let mut request = self.http_async.get(url).query(&[
            ("track_name", query.track_name.as_str()),
            ("artist_name", query.artist_name.as_str()),
        ]);

        if let Some(album_name) = query.album_name.as_deref() {
            request = request.query(&[("album_name", album_name)]);
        }

        if let Some(duration_seconds) = query.duration_seconds {
            request = request.query(&[("duration", duration_seconds)]);
        }

        let response = request.send().await.map_err(|e| {
            LyricsError::NetworkUnavailable(format!("failed to request lyrics from LRCLIB: {e}"))
        })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let response = response.error_for_status().map_err(|e| {
            LyricsError::NetworkUnavailable(format!("LRCLIB returned a non-success response: {e}"))
        })?;
        let lyrics = response.json::<LrcLibLyrics>().await.map_err(|e| {
            LyricsError::NetworkUnavailable(format!(
                "failed to deserialize LRCLIB lyrics response: {e}"
            ))
        })?;

        Ok(Some(lyrics))
    }
}
