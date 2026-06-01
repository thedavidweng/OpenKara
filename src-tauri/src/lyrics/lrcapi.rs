use crate::lyrics::error::LyricsError;
use crate::lyrics::lrclib::LyricsLookupQuery;
use serde::Deserialize;

const DEFAULT_BASE_URL: &str = "https://api.lrc.cx";
const USER_AGENT: &str = concat!("OpenKara/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LrcApiLyrics {
    pub id: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub score: f64,
    pub lrc: String,
    pub lrc_ttml: Option<String>,
    pub lyric_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LrcApiMiss {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
enum LrcApiResponse {
    Hits(Vec<LrcApiLyrics>),
    Miss(LrcApiMiss),
}

#[derive(Debug, Clone)]
pub struct LrcApiClient {
    base_url: String,
    http: reqwest::blocking::Client,
}

impl LrcApiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            http: reqwest::blocking::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .expect("reqwest blocking client should build"),
        }
    }

    pub fn new_default() -> Self {
        Self::new(DEFAULT_BASE_URL)
    }

    pub fn fetch_by_track(
        &self,
        query: &LyricsLookupQuery,
    ) -> Result<Option<LrcApiLyrics>, LyricsError> {
        let url = format!("{}/jsonapi", self.base_url);
        let mut request = self.http.get(url).query(&[
            ("title", query.track_name.as_str()),
            ("artist", query.artist_name.as_str()),
        ]);

        if let Some(album_name) = query.album_name.as_deref() {
            request = request.query(&[("album", album_name)]);
        }

        let response = request.send().map_err(|e| {
            LyricsError::NetworkUnavailable(format!("failed to request lyrics from LrcAPI: {e}"))
        })?;
        let response = response.error_for_status().map_err(|e| {
            LyricsError::NetworkUnavailable(format!("LrcAPI returned a non-success response: {e}"))
        })?;
        let response = response.json::<LrcApiResponse>().map_err(|e| {
            LyricsError::NetworkUnavailable(format!(
                "failed to deserialize LrcAPI lyrics response: {e}"
            ))
        })?;

        Ok(match response {
            LrcApiResponse::Hits(entries) => entries
                .into_iter()
                .filter(|entry| !entry.lrc.trim().is_empty())
                .max_by(|left, right| left.score.total_cmp(&right.score)),
            LrcApiResponse::Miss(miss) => {
                if miss.message.trim() == "未找到歌词" {
                    None
                } else {
                    return Err(LyricsError::NetworkUnavailable(format!(
                        "LrcAPI returned an unexpected response: {}",
                        miss.message
                    )));
                }
            }
        })
    }
}
