use crate::lyrics::amll_match::{self, AmllMatchCandidate};
use crate::lyrics::error::LyricsError;
use crate::lyrics::lrclib::LyricsLookupQuery;
use crate::lyrics::parser::has_word_tokens;
use crate::lyrics::ttml_parser;
use serde::Deserialize;

use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://api.amll.dev";
const USER_AGENT: &str = concat!("OpenKara/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AmllApiResponse<T> {
    Success {
        status: u16,
        data: T,
    },
    Error {
        status: u16,
        error: String,
        message: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AmllSearchData {
    items: Vec<AmllSongItem>,
    pagination: AmllPagination,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AmllSongItem {
    id: i64,
    filename: String,
    music_names: Vec<String>,
    artist_names: Vec<String>,
    #[allow(dead_code)]
    album_names: Vec<String>,
    lyrics: Option<String>,
    #[allow(dead_code)]
    format: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AmllPagination {
    page: u32,
    page_size: u32,
    total: u32,
    total_pages: u32,
    has_more: bool,
}

#[derive(Debug, Clone)]
pub struct AmllClient {
    base_url: String,
    http: reqwest::blocking::Client,
    http_async: reqwest::Client,
}

impl AmllClient {
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

    pub fn fetch_by_track(&self, query: &LyricsLookupQuery) -> Result<Option<String>, LyricsError> {
        self.fetch_by_track_with(&self.http, query)
    }

    pub async fn fetch_by_track_async(
        &self,
        query: &LyricsLookupQuery,
    ) -> Result<Option<String>, LyricsError> {
        self.fetch_by_track_with_async(&self.http_async, query)
            .await
    }

    fn fetch_by_track_with(
        &self,
        http: &reqwest::blocking::Client,
        query: &LyricsLookupQuery,
    ) -> Result<Option<String>, LyricsError> {
        let Some(item) = self.search_confident_item(http, query)? else {
            return Ok(None);
        };
        self.get_word_timed_lyrics(http, item.id)
    }

    async fn fetch_by_track_with_async(
        &self,
        http: &reqwest::Client,
        query: &LyricsLookupQuery,
    ) -> Result<Option<String>, LyricsError> {
        let Some(item) = self.search_confident_item_async(http, query).await? else {
            return Ok(None);
        };
        self.get_word_timed_lyrics_async(http, item.id).await
    }

    fn search_confident_item(
        &self,
        http: &reqwest::blocking::Client,
        query: &LyricsLookupQuery,
    ) -> Result<Option<AmllSongItem>, LyricsError> {
        let album_sent = query
            .album_name
            .as_deref()
            .is_some_and(|album| !album.trim().is_empty());
        let first = self.search_page(http, query, album_sent)?;
        let items = match first {
            None => return Ok(None),
            Some(items) if items.is_empty() && album_sent => {
                match self.search_page(http, query, false)? {
                    None => return Ok(None),
                    Some(items) => items,
                }
            }
            Some(items) => items,
        };
        Ok(select_confident_item(query, &items))
    }

    async fn search_confident_item_async(
        &self,
        http: &reqwest::Client,
        query: &LyricsLookupQuery,
    ) -> Result<Option<AmllSongItem>, LyricsError> {
        let album_sent = query
            .album_name
            .as_deref()
            .is_some_and(|album| !album.trim().is_empty());
        let first = self.search_page_async(http, query, album_sent).await?;
        let items = match first {
            None => return Ok(None),
            Some(items) if items.is_empty() && album_sent => {
                match self.search_page_async(http, query, false).await? {
                    None => return Ok(None),
                    Some(items) => items,
                }
            }
            Some(items) => items,
        };
        Ok(select_confident_item(query, &items))
    }

    fn search_page(
        &self,
        http: &reqwest::blocking::Client,
        query: &LyricsLookupQuery,
        include_album: bool,
    ) -> Result<Option<Vec<AmllSongItem>>, LyricsError> {
        let url = format!("{}/v1/lyrics/search", self.base_url);
        let mut request = http.get(&url).query(&[
            ("musicName", query.track_name.as_str()),
            ("artistName", query.artist_name.as_str()),
            ("page", "1"),
            ("pageSize", "5"),
        ]);
        if include_album {
            if let Some(album_name) = query.album_name.as_deref().map(str::trim) {
                if !album_name.is_empty() {
                    request = request.query(&[("albumName", album_name)]);
                }
            }
        }

        tracing::debug!(
            has_title = !query.track_name.is_empty(),
            has_artist = !query.artist_name.is_empty(),
            has_album = include_album,
            "amll search"
        );

        let response = request.send().map_err(|error| {
            LyricsError::NetworkUnavailable(format!("failed to request lyrics from AMLL: {error}"))
        })?;
        let http_status = response.status();
        if let Some(miss_or_error) = classify_http_status(http_status.as_u16(), "search") {
            return miss_or_error;
        }

        let parsed = response
            .json::<AmllApiResponse<AmllSearchData>>()
            .map_err(|error| {
                LyricsError::NetworkUnavailable(format!(
                    "failed to deserialize AMLL search response: {error}"
                ))
            })?;
        match interpret_search_body(parsed) {
            Ok(Some(data)) => {
                tracing::debug!(
                    items = data.items.len(),
                    has_more = data.pagination.has_more,
                    page = data.pagination.page,
                    page_size = data.pagination.page_size,
                    total = data.pagination.total,
                    total_pages = data.pagination.total_pages,
                    "amll search page"
                );
                Ok(Some(data.items))
            }
            other => other.map(|data| data.map(|data| data.items)),
        }
    }

    async fn search_page_async(
        &self,
        http: &reqwest::Client,
        query: &LyricsLookupQuery,
        include_album: bool,
    ) -> Result<Option<Vec<AmllSongItem>>, LyricsError> {
        let url = format!("{}/v1/lyrics/search", self.base_url);
        let mut request = http.get(&url).query(&[
            ("musicName", query.track_name.as_str()),
            ("artistName", query.artist_name.as_str()),
            ("page", "1"),
            ("pageSize", "5"),
        ]);
        if include_album {
            if let Some(album_name) = query.album_name.as_deref().map(str::trim) {
                if !album_name.is_empty() {
                    request = request.query(&[("albumName", album_name)]);
                }
            }
        }

        let response = request.send().await.map_err(|error| {
            LyricsError::NetworkUnavailable(format!("failed to request lyrics from AMLL: {error}"))
        })?;
        let http_status = response.status();
        if let Some(miss_or_error) = classify_http_status(http_status.as_u16(), "search") {
            return miss_or_error;
        }

        let parsed = response
            .json::<AmllApiResponse<AmllSearchData>>()
            .await
            .map_err(|error| {
                LyricsError::NetworkUnavailable(format!(
                    "failed to deserialize AMLL search response: {error}"
                ))
            })?;
        interpret_search_body(parsed).map(|data| data.map(|data| data.items))
    }

    fn get_word_timed_lyrics(
        &self,
        http: &reqwest::blocking::Client,
        id: i64,
    ) -> Result<Option<String>, LyricsError> {
        let url = format!("{}/v1/lyrics/get", self.base_url);
        let response = http.get(url).query(&[("id", id)]).send().map_err(|error| {
            LyricsError::NetworkUnavailable(format!("failed to request AMLL lyrics get: {error}"))
        })?;
        let http_status = response.status();
        if let Some(miss_or_error) = classify_http_status(http_status.as_u16(), "get") {
            return miss_or_error;
        }
        let parsed = response
            .json::<AmllApiResponse<AmllSongItem>>()
            .map_err(|error| {
                LyricsError::NetworkUnavailable(format!(
                    "failed to deserialize AMLL get response: {error}"
                ))
            })?;
        interpret_get_body(parsed)
    }

    async fn get_word_timed_lyrics_async(
        &self,
        http: &reqwest::Client,
        id: i64,
    ) -> Result<Option<String>, LyricsError> {
        let url = format!("{}/v1/lyrics/get", self.base_url);
        let response = http
            .get(url)
            .query(&[("id", id)])
            .send()
            .await
            .map_err(|error| {
                LyricsError::NetworkUnavailable(format!(
                    "failed to request AMLL lyrics get: {error}"
                ))
            })?;
        let http_status = response.status();
        if let Some(miss_or_error) = classify_http_status(http_status.as_u16(), "get") {
            return miss_or_error;
        }
        let parsed = response
            .json::<AmllApiResponse<AmllSongItem>>()
            .await
            .map_err(|error| {
                LyricsError::NetworkUnavailable(format!(
                    "failed to deserialize AMLL get response: {error}"
                ))
            })?;
        interpret_get_body(parsed)
    }
}

fn select_confident_item(
    query: &LyricsLookupQuery,
    items: &[AmllSongItem],
) -> Option<AmllSongItem> {
    let candidates: Vec<AmllMatchCandidate<'_>> = items
        .iter()
        .map(|item| AmllMatchCandidate {
            music_names: &item.music_names,
            artist_names: &item.artist_names,
        })
        .collect();
    let filtered_len = candidates
        .iter()
        .filter(|item| {
            item.music_names
                .iter()
                .any(|name| amll_match::title_similar(&query.track_name, name))
                && amll_match::artists_overlap(&query.artist_name, item.artist_names)
        })
        .count();
    let decision = match amll_match::select_confident_index(
        &query.track_name,
        &query.artist_name,
        &candidates,
    ) {
        Some(index) if filtered_len == 1 => {
            tracing::debug!(
                items = items.len(),
                filtered = filtered_len,
                id = items[index].id,
                filename = %items[index].filename,
                decision = "confident_unique",
                "amll match"
            );
            Some(items[index].clone())
        }
        Some(index) => {
            tracing::debug!(
                items = items.len(),
                filtered = filtered_len,
                id = items[index].id,
                filename = %items[index].filename,
                decision = "confident_exact",
                "amll match"
            );
            Some(items[index].clone())
        }
        None if filtered_len == 0 => {
            tracing::debug!(
                items = items.len(),
                filtered = 0,
                decision = "empty",
                "amll match"
            );
            None
        }
        None => {
            tracing::debug!(
                items = items.len(),
                filtered = filtered_len,
                decision = "ambiguous",
                "amll match"
            );
            None
        }
    };
    decision
}

fn classify_http_status<T>(status: u16, operation: &str) -> Option<Result<Option<T>, LyricsError>> {
    match status {
        200 => None,
        404 => Some(Ok(None)),
        429 | 500..=599 => {
            tracing::warn!(status, operation, "amll unavailable");
            Some(Err(LyricsError::NetworkUnavailable(format!(
                "AMLL {operation} returned HTTP {status}"
            ))))
        }
        other => {
            tracing::warn!(status = other, operation, "amll unavailable");
            Some(Err(LyricsError::NetworkUnavailable(format!(
                "AMLL {operation} returned HTTP {other}"
            ))))
        }
    }
}

fn classify_body_status(status: u16, operation: &str) -> Result<Option<()>, LyricsError> {
    match status {
        200 => Ok(Some(())),
        404 => Ok(None),
        429 | 500..=599 => {
            tracing::warn!(status, operation, "amll unavailable");
            Err(LyricsError::NetworkUnavailable(format!(
                "AMLL {operation} returned status {status}"
            )))
        }
        other => {
            tracing::warn!(status = other, operation, "amll unavailable");
            Err(LyricsError::NetworkUnavailable(format!(
                "AMLL {operation} returned status {other}"
            )))
        }
    }
}

fn interpret_search_body(
    parsed: AmllApiResponse<AmllSearchData>,
) -> Result<Option<AmllSearchData>, LyricsError> {
    match parsed {
        AmllApiResponse::Success { status, data } => {
            match classify_body_status(status, "search")? {
                Some(()) => Ok(Some(data)),
                None => Ok(None),
            }
        }
        AmllApiResponse::Error {
            status,
            error,
            message,
        } => match classify_body_status(status, "search")? {
            Some(()) => Err(LyricsError::NetworkUnavailable(format!(
                "AMLL search returned an error body: {error}: {message}"
            ))),
            None => Ok(None),
        },
    }
}

fn interpret_get_body(
    parsed: AmllApiResponse<AmllSongItem>,
) -> Result<Option<String>, LyricsError> {
    let item = match parsed {
        AmllApiResponse::Success { status, data } => match classify_body_status(status, "get")? {
            Some(()) => data,
            None => return Ok(None),
        },
        AmllApiResponse::Error {
            status,
            error,
            message,
        } => match classify_body_status(status, "get")? {
            Some(()) => {
                return Err(LyricsError::NetworkUnavailable(format!(
                    "AMLL get returned an error body: {error}: {message}"
                )));
            }
            None => return Ok(None),
        },
    };

    let Some(raw) = item
        .lyrics
        .as_deref()
        .map(str::trim)
        .filter(|lyrics| !lyrics.is_empty())
        .map(ToOwned::to_owned)
    else {
        tracing::debug!(id = item.id, word_tokens = false, "amll get empty lyrics");
        return Ok(None);
    };

    let word_tokens = ttml_parser::parse_ttml(&raw)
        .map(|lines| has_word_tokens(&lines))
        .unwrap_or(false);
    tracing::debug!(id = item.id, word_tokens, "amll get");
    if word_tokens {
        Ok(Some(raw))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> LyricsLookupQuery {
        LyricsLookupQuery {
            track_name: "Yellow".to_owned(),
            artist_name: "Coldplay".to_owned(),
            album_name: Some("Parachutes".to_owned()),
            duration_seconds: Some(267),
        }
    }

    fn word_timed_ttml() -> &'static str {
        r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000"><span begin="00:01.000" end="00:02.000">Hello</span></p></div></body></tt>"#
    }

    fn line_timed_ttml() -> &'static str {
        r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:itunes="http://music.apple.com/lyric-ttml-internal"><body><div itunes:timing="Line"><p begin="00:01.000" end="00:02.000"><span begin="00:01.000" end="00:01.500">Hel</span><span begin="00:01.500" end="00:02.000">lo</span></p></div></body></tt>"#
    }

    fn search_hit_body(id: i64) -> String {
        format!(
            r#"{{
                "status": 200,
                "data": {{
                    "items": [
                        {{
                            "id": {id},
                            "filename": "yellow.ttml",
                            "musicNames": ["Yellow"],
                            "artistNames": ["Coldplay"],
                            "albumNames": ["Parachutes"],
                            "ncmMusicIds": [99],
                            "matchContext": {{"score": 1}}
                        }}
                    ],
                    "pagination": {{
                        "page": 1,
                        "pageSize": 5,
                        "total": 1,
                        "totalPages": 1,
                        "hasMore": false
                    }}
                }}
            }}"#
        )
    }

    fn search_empty_body() -> &'static str {
        r#"{
            "status": 200,
            "data": {
                "items": [],
                "pagination": {
                    "page": 1,
                    "pageSize": 5,
                    "total": 0,
                    "totalPages": 0,
                    "hasMore": false
                }
            }
        }"#
    }

    fn get_body(id: i64, lyrics: &str) -> String {
        let escaped = lyrics.replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            r#"{{
                "status": 200,
                "data": {{
                    "id": {id},
                    "filename": "yellow.ttml",
                    "musicNames": ["Yellow"],
                    "artistNames": ["Coldplay"],
                    "albumNames": ["Parachutes"],
                    "lyrics": "{escaped}",
                    "format": "ttml"
                }}
            }}"#
        )
    }

    fn search_query_matcher() -> mockito::Matcher {
        mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("musicName".into(), "Yellow".into()),
            mockito::Matcher::UrlEncoded("artistName".into(), "Coldplay".into()),
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
            mockito::Matcher::UrlEncoded("pageSize".into(), "5".into()),
            mockito::Matcher::Regex(
                r"^(?:(?:musicName|artistName|albumName|page|pageSize)=[^&]*&?)+$".into(),
            ),
        ])
    }

    #[test]
    fn search_query_has_native_fields_and_no_q_or_duration() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(search_query_matcher())
            .match_header(
                "user-agent",
                format!("OpenKara/{}", env!("CARGO_PKG_VERSION")).as_str(),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(search_empty_body())
            .create();

        let client = AmllClient::new(server.url());
        let mut lookup = query();
        lookup.album_name = None;
        let result = client
            .fetch_by_track(&lookup)
            .expect("search should succeed");
        assert!(result.is_none());
        mock.assert();
    }

    #[test]
    fn search_sends_album_and_retries_without_album_on_empty_items() {
        let mut server = mockito::Server::new();
        let with_album = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("musicName".into(), "Yellow".into()),
                mockito::Matcher::UrlEncoded("artistName".into(), "Coldplay".into()),
                mockito::Matcher::UrlEncoded("albumName".into(), "Parachutes".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(search_empty_body())
            .create();
        let without_album = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("musicName".into(), "Yellow".into()),
                mockito::Matcher::UrlEncoded("artistName".into(), "Coldplay".into()),
                mockito::Matcher::Regex(
                    r"^(?:(?:musicName|artistName|page|pageSize)=[^&]*&?)+$".into(),
                ),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(search_hit_body(42))
            .create();
        let get = server
            .mock("GET", "/v1/lyrics/get")
            .match_query(mockito::Matcher::UrlEncoded("id".into(), "42".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(get_body(42, word_timed_ttml()))
            .create();

        let client = AmllClient::new(server.url());
        let result = client
            .fetch_by_track(&query())
            .expect("album retry should succeed")
            .expect("word-timed lyrics");
        assert!(result.contains("Hello"));
        with_album.assert();
        without_album.assert();
        get.assert();
    }

    #[test]
    fn http_404_is_miss() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .with_status(404)
            .create();

        let client = AmllClient::new(server.url());
        let result = client.fetch_by_track(&query()).expect("404 is a miss");
        assert!(result.is_none());
        mock.assert();
    }

    #[test]
    fn http_429_is_unavailable() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .with_status(429)
            .create();

        let client = AmllClient::new(server.url());
        let error = client
            .fetch_by_track(&query())
            .expect_err("429 is unavailable");
        assert!(matches!(error, LyricsError::NetworkUnavailable(_)));
        mock.assert();
    }

    #[test]
    fn http_500_is_unavailable() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .with_status(500)
            .create();

        let client = AmllClient::new(server.url());
        let error = client
            .fetch_by_track(&query())
            .expect_err("500 is unavailable");
        assert!(matches!(error, LyricsError::NetworkUnavailable(_)));
        mock.assert();
    }

    #[test]
    fn empty_items_without_album_is_miss() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .expect(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(search_empty_body())
            .create();

        let client = AmllClient::new(server.url());
        let mut no_album = query();
        no_album.album_name = None;
        let result = client
            .fetch_by_track(&no_album)
            .expect("empty page is a miss");
        assert!(result.is_none());
        mock.assert();
    }

    #[test]
    fn confident_id_then_get_returns_ttml() {
        let mut server = mockito::Server::new();
        let search = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(search_hit_body(7))
            .create();
        let get = server
            .mock("GET", "/v1/lyrics/get")
            .match_query(mockito::Matcher::UrlEncoded("id".into(), "7".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(get_body(7, word_timed_ttml()))
            .create();

        let client = AmllClient::new(server.url());
        let result = client
            .fetch_by_track(&query())
            .expect("get should succeed")
            .expect("word-timed ttml");
        assert!(result.contains("<span"));
        search.assert();
        get.assert();
    }

    #[test]
    fn get_line_timed_ttml_is_miss() {
        let mut server = mockito::Server::new();
        let _search = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(search_hit_body(8))
            .create();
        let get = server
            .mock("GET", "/v1/lyrics/get")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(get_body(8, line_timed_ttml()))
            .create();

        let client = AmllClient::new(server.url());
        let result = client
            .fetch_by_track(&query())
            .expect("line-timed is a miss");
        assert!(result.is_none());
        get.assert();
    }

    #[test]
    fn unknown_json_fields_still_deserialize() {
        let parsed: AmllApiResponse<AmllSearchData> =
            serde_json::from_str(&search_hit_body(1)).expect("extra fields are ignored");
        match parsed {
            AmllApiResponse::Success { data, .. } => {
                assert_eq!(data.items[0].id, 1);
                assert_eq!(data.items[0].music_names, vec!["Yellow".to_owned()]);
            }
            AmllApiResponse::Error { .. } => panic!("should parse as success"),
        }
    }
}
