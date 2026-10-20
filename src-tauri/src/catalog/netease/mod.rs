mod cache;
mod client;

pub use cache::NeteaseSongCache;
pub use client::{LiveNeteaseApi, NeteaseApi, NeteaseSidecar};

use super::streaming::{ResolveProgress, StreamingSource};
use super::types::{
    CatalogError, ImportRefusal, ImportRefusalReason, ResolvedStreamingFile, ResolvedStreamingUrl,
    StreamingCredentials, StreamingPlaylistDetail, StreamingPlaylistSummary,
    StreamingPreviewOutcome, StreamingResolveOutcome, StreamingSessionSnapshot, StreamingTrack,
    StreamingTrackListing,
};
use crate::catalog::credentials;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Ids per follow-up `song_detail` request (NetEase allows up to 1000).
const TRACK_PAGE_SIZE: usize = 500;
/// Tracks fetched before the liked list is first shown.
const LIKED_HEAD_TRACKS: usize = 100;

pub struct NeteaseStreamingSource<H> {
    pub(crate) http: H,
    app_data_dir: PathBuf,
    credentials: Mutex<Option<StreamingCredentials>>,
    display_name: Mutex<Option<String>>,
    cache: Arc<NeteaseSongCache>,
}

impl<H: NeteaseApi> NeteaseStreamingSource<H> {
    pub fn open(http: H, app_data_dir: impl Into<PathBuf>) -> Result<Self, CatalogError> {
        let app_data_dir = app_data_dir.into();
        let stored = credentials::load_credentials(&app_data_dir, "netease")
            .map_err(|error| CatalogError::Internal(error.to_string()))?;
        let snapshot = credentials::load_session_snapshot(&app_data_dir, "netease")
            .map_err(|error| CatalogError::Internal(error.to_string()))?;
        Ok(Self {
            http,
            app_data_dir,
            credentials: Mutex::new(stored),
            display_name: Mutex::new(snapshot.display_name),
            cache: Arc::default(),
        })
    }

    /// Shares one song cache across the per-command source instances.
    pub fn with_cache(mut self, cache: Arc<NeteaseSongCache>) -> Self {
        self.cache = cache;
        self
    }

    fn credentials(&self) -> Option<StreamingCredentials> {
        self.credentials.lock().ok().and_then(|guard| guard.clone())
    }

    fn persist(
        &self,
        credentials: StreamingCredentials,
        display_name: Option<String>,
    ) -> Result<(), CatalogError> {
        credentials::store_session(
            &self.app_data_dir,
            "netease",
            credentials.clone(),
            display_name.clone(),
        )
        .map_err(|error| CatalogError::Internal(error.to_string()))?;
        self.cache.clear();
        if let Ok(mut guard) = self.credentials.lock() {
            *guard = Some(credentials);
        }
        if let Ok(mut guard) = self.display_name.lock() {
            *guard = display_name;
        }
        Ok(())
    }

    fn clear(&self) -> Result<(), CatalogError> {
        self.cache.clear();
        credentials::clear_session(&self.app_data_dir, "netease")
            .map_err(|error| CatalogError::Internal(error.to_string()))?;
        if let Ok(mut guard) = self.credentials.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.display_name.lock() {
            *guard = None;
        }
        Ok(())
    }

    fn require_credentials(&self) -> Result<StreamingCredentials, CatalogError> {
        self.credentials()
            .ok_or_else(|| CatalogError::SessionExpired {
                source_id: "netease".to_owned(),
            })
    }

    pub fn complete_web_sign_in(
        &self,
        cookies: impl IntoIterator<Item = (String, String)>,
    ) -> Result<StreamingSessionSnapshot, CatalogError> {
        let mut music_u = None;
        let mut csrf = String::new();
        for (name, value) in cookies {
            match name.as_str() {
                "MUSIC_U" if !value.is_empty() => music_u = Some(value),
                "__csrf" => csrf = value,
                _ => {}
            }
        }
        if csrf.is_empty() {
            return Err(CatalogError::AuthFailed {
                source_id: "netease".to_owned(),
                detail: "NetEase sign-in did not provide a complete session. Try signing in again."
                    .to_owned(),
            });
        }
        let credentials = StreamingCredentials {
            music_u: music_u.ok_or_else(|| CatalogError::AuthFailed {
                source_id: "netease".to_owned(),
                detail: "NetEase sign-in did not provide a complete session. Try signing in again."
                    .to_owned(),
            })?,
            csrf,
        };
        let account = self.http.request("user_account", json!({}), &credentials)?;
        if account
            .pointer("/account/id")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
            .is_none()
        {
            return Err(CatalogError::SessionExpired {
                source_id: "netease".to_owned(),
            });
        }
        let name = account
            .pointer("/profile/nickname")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        self.persist(credentials, name.clone())?;
        Ok(StreamingSessionSnapshot {
            source_id: "netease".to_owned(),
            signed_in: true,
            display_name: name,
            expired: false,
        })
    }
}

impl<H: NeteaseApi> StreamingSource for NeteaseStreamingSource<H> {
    fn source_id(&self) -> &str {
        "netease"
    }

    fn session(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        let credentials = self.credentials();
        let expired = match credentials.as_ref() {
            Some(credentials) => match self.http.request("user_account", json!({}), credentials) {
                Ok(account) => account
                    .pointer("/account/id")
                    .and_then(Value::as_i64)
                    .is_none_or(|id| id <= 0),
                Err(CatalogError::SessionExpired { .. }) => true,
                Err(error) => return Err(error),
            },
            None => false,
        };
        let display_name = self
            .display_name
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        Ok(StreamingSessionSnapshot {
            source_id: "netease".to_owned(),
            signed_in: credentials.is_some() && !expired,
            display_name,
            expired,
        })
    }

    fn sign_out(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        self.clear()?;
        Ok(StreamingSessionSnapshot {
            source_id: "netease".to_owned(),
            signed_in: false,
            display_name: None,
            expired: false,
        })
    }

    fn liked_tracks(&self) -> Result<Vec<StreamingTrack>, CatalogError> {
        let credentials = self.require_credentials()?;
        let ids = self.liked_ids(&credentials)?;
        self.tracks_by_ids(&credentials, &ids)
    }

    fn liked_head(&self) -> Result<StreamingTrackListing, CatalogError> {
        let started = Instant::now();
        let credentials = self.require_credentials()?;
        let ids = self.liked_ids(&credentials)?;
        let head = &ids[..ids.len().min(LIKED_HEAD_TRACKS)];
        let tracks = self.tracks_by_ids(&credentials, head)?;
        tracing::debug!(
            ids = ids.len(),
            head = tracks.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "netease liked list head ready"
        );
        Ok(StreamingTrackListing {
            partial: ids.len() > head.len(),
            tracks,
            track_ids: ids,
        })
    }

    fn tracks(&self, ids: &[String]) -> Result<Vec<StreamingTrack>, CatalogError> {
        let credentials = self.require_credentials()?;
        self.tracks_by_ids(&credentials, ids)
    }

    fn playlists(&self) -> Result<Vec<StreamingPlaylistSummary>, CatalogError> {
        let credentials = self.require_credentials()?;
        let account = self.http.request("user_account", json!({}), &credentials)?;
        let uid = account
            .pointer("/account/id")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
            .ok_or_else(|| CatalogError::SessionExpired {
                source_id: "netease".to_owned(),
            })?;
        let response = self.http.request(
            "user_playlist",
            json!({ "uid": uid, "limit": 200, "offset": 0 }),
            &credentials,
        )?;
        let playlists = response
            .get("playlist")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(playlists
            .into_iter()
            .filter_map(|playlist| {
                Some(StreamingPlaylistSummary {
                    remote_id: playlist.get("id")?.as_i64()?.to_string(),
                    name: playlist
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Streaming Playlist")
                        .to_owned(),
                    track_count: playlist
                        .get("trackCount")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32,
                    cover_url: playlist
                        .get("coverImgUrl")
                        .and_then(Value::as_str)
                        .and_then(https_url),
                })
            })
            .collect())
    }

    fn playlist(&self, remote_id: &str) -> Result<StreamingPlaylistDetail, CatalogError> {
        let mut detail = self.playlist_head(remote_id)?;
        if detail.partial {
            let credentials = self.require_credentials()?;
            detail.tracks = self.tracks_by_ids(&credentials, &detail.track_ids)?;
            detail.partial = false;
        }
        Ok(detail)
    }

    /// `playlist_detail` already carries the first batch of tracks and their
    /// privileges, so the head needs no `song_detail` round trip.
    fn playlist_head(&self, remote_id: &str) -> Result<StreamingPlaylistDetail, CatalogError> {
        let started = Instant::now();
        let credentials = self.require_credentials()?;
        let response = self.http.request(
            "playlist_detail",
            json!({ "id": remote_id, "n": 1000, "s": 0 }),
            &credentials,
        )?;
        let name = response
            .pointer("/playlist/name")
            .and_then(Value::as_str)
            .unwrap_or("Streaming Playlist")
            .to_owned();
        let ids: Vec<String> = response
            .pointer("/playlist/trackIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|track| track.get("id").and_then(Value::as_u64))
            .map(|id| id.to_string())
            .collect();
        let text = |pointer: &str| {
            response
                .pointer(pointer)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        };
        let track_count = response
            .pointer("/playlist/trackCount")
            .and_then(Value::as_u64)
            .unwrap_or(ids.len() as u64) as u32;
        let songs = json_array(response.pointer("/playlist/tracks"));
        let privileges = json_array(response.get("privileges"));
        let tracks = tracks_from_songs(&songs, &privileges);
        self.cache.insert_all(&tracks);
        tracing::debug!(
            ids = ids.len(),
            head = tracks.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "netease playlist head ready"
        );
        Ok(StreamingPlaylistDetail {
            remote_id: remote_id.to_owned(),
            name,
            cover_url: response
                .pointer("/playlist/coverImgUrl")
                .and_then(Value::as_str)
                .and_then(https_url),
            creator: text("/playlist/creator/nickname"),
            description: text("/playlist/description"),
            track_count,
            updated_at_ms: response
                .pointer("/playlist/updateTime")
                .and_then(Value::as_u64)
                .filter(|value| *value > 0),
            partial: tracks.len() < ids.len(),
            track_ids: ids,
            tracks,
        })
    }

    fn search(&self, query: &str) -> Result<Vec<StreamingTrack>, CatalogError> {
        let credentials = self.require_credentials()?;
        let response = self.http.request(
            "cloudsearch",
            json!({
                "keywords": query,
                "type": 1,
                "limit": 30,
                "offset": 0
            }),
            &credentials,
        )?;
        let songs = response
            .pointer("/result/songs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(songs
            .into_iter()
            .filter_map(|song| track_from_json(&song, None))
            .collect())
    }

    fn resolve(
        &self,
        remote_track_id: &str,
        progress: &dyn ResolveProgress,
    ) -> Result<StreamingResolveOutcome, CatalogError> {
        let (track, url, extension) = match self.playable_url(remote_track_id)? {
            PlayableUrl::Refusal(refusal) => return Ok(StreamingResolveOutcome::Refusal(refusal)),
            PlayableUrl::Url {
                track,
                url,
                extension,
            } => (*track, url, extension),
        };
        let dest = temp_download_path(&extension);
        if progress.is_cancelled() {
            return Err(CatalogError::Cancelled);
        }
        progress.downloading(None);
        self.http.download(&url, &dest, progress)?;
        Ok(StreamingResolveOutcome::File(ResolvedStreamingFile {
            path: dest,
            title: Some(track.title),
            artist: Some(track.artist),
            album: track.album,
        }))
    }

    fn preview(&self, remote_track_id: &str) -> Result<StreamingPreviewOutcome, CatalogError> {
        Ok(match self.playable_url(remote_track_id)? {
            PlayableUrl::Refusal(refusal) => StreamingPreviewOutcome::Refusal(refusal),
            PlayableUrl::Url { url, extension, .. } => {
                StreamingPreviewOutcome::Stream(ResolvedStreamingUrl { url, extension })
            }
        })
    }
}

/// What `song_url` says about one track.
enum PlayableUrl {
    Refusal(ImportRefusal),
    Url {
        track: Box<StreamingTrack>,
        url: String,
        extension: String,
    },
}

impl<H: NeteaseApi> NeteaseStreamingSource<H> {
    /// The track's refusal from its listing data or from `song_url` (trial
    /// clip, empty URL), else the full-length audio URL and its format.
    fn playable_url(&self, remote_track_id: &str) -> Result<PlayableUrl, CatalogError> {
        if remote_track_id.parse::<u64>().is_err() {
            return Err(CatalogError::Internal(
                "Invalid NetEase track ID".to_owned(),
            ));
        }
        let credentials = self.require_credentials()?;
        let track = self
            .tracks_by_ids(&credentials, &[remote_track_id.to_owned()])?
            .into_iter()
            .next()
            .unwrap_or(StreamingTrack {
                source_id: "netease".to_owned(),
                remote_track_id: remote_track_id.to_owned(),
                title: remote_track_id.to_owned(),
                artist: String::new(),
                album: None,
                duration_ms: None,
                refusal: None,
                library_song_id: None,
                cover_url: None,
            });
        if let Some(refusal) = track.refusal.clone() {
            return Ok(PlayableUrl::Refusal(refusal));
        }

        let url_response = self.http.request(
            "song_url",
            json!({
                "id": remote_track_id,
                "br": 320000
            }),
            &credentials,
        )?;
        let data = url_response
            .pointer("/data/0")
            .cloned()
            .unwrap_or(Value::Null);
        if data
            .get("freeTrialInfo")
            .is_some_and(|value| !value.is_null())
        {
            return Ok(PlayableUrl::Refusal(ImportRefusal {
                reason: ImportRefusalReason::TrialClip,
                title: track.title,
                artist: track.artist,
            }));
        }
        let url = data.get("url").and_then(Value::as_str).unwrap_or("");
        if url.is_empty() {
            return Ok(PlayableUrl::Refusal(ImportRefusal {
                reason: ImportRefusalReason::EmptyUrl,
                title: track.title,
                artist: track.artist,
            }));
        }
        let format = data
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase);
        let extension = match format.as_deref() {
            Some(extension @ ("mp3" | "flac" | "wav" | "ogg" | "aac" | "m4a")) => extension,
            _ => {
                return Err(CatalogError::Internal(
                    "NetEase returned an unsupported audio format".to_owned(),
                ))
            }
        };
        Ok(PlayableUrl::Url {
            url: url.to_owned(),
            extension: extension.to_owned(),
            track: Box::new(track),
        })
    }
}

impl<H: NeteaseApi> NeteaseStreamingSource<H> {
    fn liked_ids(&self, credentials: &StreamingCredentials) -> Result<Vec<String>, CatalogError> {
        let account = self.http.request("user_account", json!({}), credentials)?;
        let uid = account
            .pointer("/account/id")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
            .ok_or_else(|| CatalogError::SessionExpired {
                source_id: "netease".to_owned(),
            })?;
        let liked = self
            .http
            .request("likelist", json!({ "uid": uid }), credentials)?;
        Ok(json_array(liked.get("ids"))
            .iter()
            .filter_map(|value| {
                value
                    .as_i64()
                    .map(|id| id.to_string())
                    .or_else(|| value.as_str().map(ToOwned::to_owned))
            })
            .collect())
    }

    /// Tracks for `ids` in order. Cached ids are served locally; the rest are
    /// fetched in pages of [`TRACK_PAGE_SIZE`] and remembered.
    fn tracks_by_ids(
        &self,
        credentials: &StreamingCredentials,
        ids: &[String],
    ) -> Result<Vec<StreamingTrack>, CatalogError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if ids.iter().any(|id| id.parse::<u64>().is_err()) {
            return Err(CatalogError::Internal(
                "Invalid NetEase track ID".to_owned(),
            ));
        }
        let mut found: HashMap<&str, StreamingTrack> = HashMap::new();
        let mut missing: Vec<&String> = Vec::new();
        for id in ids {
            match self.cache.get(id) {
                Some(track) => {
                    found.insert(id, track);
                }
                None => missing.push(id),
            }
        }
        for page in missing.chunks(TRACK_PAGE_SIZE) {
            let started = Instant::now();
            let joined = page
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let detail = self
                .http
                .request("song_detail", json!({ "ids": joined }), credentials)?;
            let songs = json_array(detail.get("songs"));
            let privileges = json_array(detail.get("privileges"));
            log_privilege_summary(songs.len(), &privileges);
            let tracks = tracks_from_songs(&songs, &privileges);
            tracing::debug!(
                ids = page.len(),
                tracks = tracks.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "netease song_detail page"
            );
            self.cache.insert_all(&tracks);
            for track in tracks {
                if let Some(id) = page.iter().find(|id| **id == &track.remote_track_id) {
                    found.insert(id.as_str(), track);
                }
            }
        }
        Ok(ids
            .iter()
            .filter_map(|id| found.get(id.as_str()).cloned())
            .collect())
    }
}

fn json_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

/// Builds tracks from a `songs` array, matching each privilege by song id (or
/// by position when the provider omitted ids).
fn tracks_from_songs(songs: &[Value], privileges: &[Value]) -> Vec<StreamingTrack> {
    let by_id = privileges_by_id(privileges);
    songs
        .iter()
        .enumerate()
        .filter_map(|(index, song)| {
            let privilege = match &by_id {
                Some(by_id) => song
                    .get("id")
                    .and_then(Value::as_i64)
                    .and_then(|id| by_id.get(&id).copied()),
                None => privileges.get(index),
            };
            track_from_json(song, privilege)
        })
        .collect()
}

fn track_from_json(song: &Value, privilege: Option<&Value>) -> Option<StreamingTrack> {
    let remote_track_id = song
        .get("id")
        .and_then(Value::as_i64)
        .map(|id| id.to_string())?;
    let title = song
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled")
        .to_owned();
    let artist = song
        .get("ar")
        .and_then(Value::as_array)
        .and_then(|artists| artists.first())
        .and_then(|artist| artist.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let album = song
        .pointer("/al/name")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let duration_ms = song.get("dt").and_then(Value::as_u64);
    let cover_url = song
        .pointer("/al/picUrl")
        .and_then(Value::as_str)
        .and_then(https_url);
    let refusal =
        refusal_from_privilege(song, privilege.or_else(|| song.get("privilege"))).map(|reason| {
            ImportRefusal {
                reason,
                title: title.clone(),
                artist: artist.clone(),
            }
        });
    Some(StreamingTrack {
        source_id: "netease".to_owned(),
        remote_track_id,
        title,
        artist,
        album,
        duration_ms,
        refusal,
        library_song_id: None,
        cover_url,
    })
}

/// Normalizes a provider image URL to HTTPS; empty values become `None`.
fn https_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    Some(match url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_owned(),
    })
}

/// Indexes privileges by song id. Returns `None` when the provider omitted ids
/// (then the caller falls back to array position).
fn privileges_by_id(privileges: &[Value]) -> Option<std::collections::HashMap<i64, &Value>> {
    let map: std::collections::HashMap<i64, &Value> = privileges
        .iter()
        .filter_map(|privilege| Some((privilege.get("id")?.as_i64()?, privilege)))
        .collect();
    (!map.is_empty()).then_some(map)
}

/// Debug-level summary of provider rights flags for one `song_detail` batch,
/// to tell genuine refusals from a mismatch. No identity data is logged.
fn log_privilege_summary(songs: usize, privileges: &[Value]) {
    let count = |predicate: &dyn Fn(&Value) -> bool| {
        privileges.iter().filter(|value| predicate(value)).count()
    };
    let st_negative = count(&|value| value.get("st").and_then(Value::as_i64).unwrap_or(0) < 0);
    let pl_zero = count(&|value| value.get("pl").and_then(Value::as_i64) == Some(0));
    let fee_free = count(&|value| value.get("fee").and_then(Value::as_i64) == Some(0));
    let fee_vip = count(&|value| value.get("fee").and_then(Value::as_i64) == Some(1));
    let fee_other = privileges.len().saturating_sub(fee_free + fee_vip);
    tracing::debug!(
        songs,
        privileges = privileges.len(),
        st_negative,
        pl_zero,
        fee_free,
        fee_vip,
        fee_other,
        "netease song_detail rights summary"
    );
}

/// Listing-time playability, following YesPlayMusic's `isTrackPlayable`.
/// `fee 1` is VIP-only, which the provider serves as a 30-second clip.
fn refusal_from_privilege(song: &Value, privilege: Option<&Value>) -> Option<ImportRefusalReason> {
    let number = |value: Option<&Value>, key: &str| {
        value
            .and_then(|value| value.get(key))
            .and_then(Value::as_i64)
    };
    let truthy = |value: Option<&Value>| match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
        _ => false,
    };
    if number(privilege, "pl").unwrap_or(0) > 0 {
        return None;
    }
    // Cloud-storage tracks play for the signed-in owner.
    if truthy(privilege.and_then(|privilege| privilege.get("cs"))) {
        return None;
    }
    let fee = |value: i64| {
        number(Some(song), "fee") == Some(value) || number(privilege, "fee") == Some(value)
    };
    if fee(1) {
        return Some(ImportRefusalReason::TrialClip);
    }
    if fee(4) {
        // Paid album: nothing to import without buying it.
        return Some(ImportRefusalReason::NoPlayRights);
    }
    if song
        .get("noCopyrightRcmd")
        .is_some_and(|value| !value.is_null())
    {
        return Some(ImportRefusalReason::NoPlayRights);
    }
    if number(privilege, "st").unwrap_or(0) < 0 {
        return Some(ImportRefusalReason::NoPlayRights);
    }
    None
}

fn temp_download_path(extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "openkara-netease-{}.{extension}",
        uuid::Uuid::new_v4()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    struct FixtureApi {
        account: Value,
        url: Value,
    }
    impl NeteaseApi for FixtureApi {
        fn request(
            &self,
            operation: &str,
            params: Value,
            _: &StreamingCredentials,
        ) -> Result<Value, CatalogError> {
            let body = match operation {
                "user_account" => self.account.clone(),
                "playlist_detail" => json!({
                    "code": 200,
                    "playlist": {
                        "name": "Large playlist",
                        "tracks": [{"id": 1, "name": "Song"}],
                        "trackIds": (1..=1001).map(|id| json!({"id": id})).collect::<Vec<_>>()
                    }
                }),
                "likelist" => json!({"code": 200, "ids": (1..=1001).collect::<Vec<_>>()}),
                "song_detail" => {
                    let ids: Vec<u64> = params["ids"]
                        .as_str()
                        .unwrap()
                        .split(',')
                        .map(|id| id.parse().unwrap())
                        .collect();
                    assert!(ids.len() <= TRACK_PAGE_SIZE);
                    json!({
                        "code": 200,
                        "songs": ids.iter().map(|id| json!({"id": id, "name": "Song", "ar": [{"name": "Artist"}]})).collect::<Vec<_>>(),
                        "privileges": ids.iter().map(|id| json!({"fee": 0, "pl": if *id == 1001 { 0 } else { 320000 }, "st": if *id == 1001 { -100 } else { 0 }})).collect::<Vec<_>>()
                    })
                }
                "song_url" => json!({"code":200,"data":[self.url.clone()]}),
                _ => panic!("unexpected operation"),
            };
            client::provider_response(body)
        }
        fn download(
            &self,
            _: &str,
            dest: &Path,
            progress: &dyn ResolveProgress,
        ) -> Result<(), CatalogError> {
            progress.downloading(Some(100));
            std::fs::write(dest, b"complete audio").unwrap();
            Ok(())
        }
    }
    fn api(account: Value, url: Value) -> FixtureApi {
        FixtureApi { account, url }
    }
    fn account() -> Value {
        json!({"code":200,"account":{"id":42},"profile":{"nickname":"Ada"}})
    }
    fn cookies() -> Vec<(String, String)> {
        vec![
            ("MUSIC_U".into(), "fixture-session".into()),
            ("__csrf".into(), "fixture-csrf".into()),
            ("ignored".into(), "not-stored".into()),
        ]
    }
    #[test]
    fn playlists_and_liked_tracks_load_all_ids_in_page_sized_batches() {
        let dir = tempfile::tempdir().unwrap();
        let source = NeteaseStreamingSource::open(api(account(), Value::Null), dir.path()).unwrap();
        source.complete_web_sign_in(cookies()).unwrap();
        let playlist = source.playlist("42").unwrap();
        assert_eq!(playlist.name, "Large playlist");
        for tracks in [playlist.tracks, source.liked_tracks().unwrap()] {
            assert_eq!(tracks.len(), 1001);
            for (index, track) in tracks.iter().enumerate() {
                assert_eq!(track.remote_track_id, (index + 1).to_string());
            }
            assert_eq!(
                tracks[1000].refusal.as_ref().unwrap().reason,
                ImportRefusalReason::NoPlayRights
            );
        }
        assert!(matches!(
            source
                .resolve("1001", &crate::catalog::NoResolveProgress)
                .unwrap(),
            StreamingResolveOutcome::Refusal(ImportRefusal {
                reason: ImportRefusalReason::NoPlayRights,
                ..
            })
        ));
        assert!(source
            .tracks_by_ids(&source.require_credentials().unwrap(), &[])
            .unwrap()
            .is_empty());
        assert!(source
            .tracks_by_ids(
                &source.require_credentials().unwrap(),
                &["invalid".to_owned()]
            )
            .is_err());
    }
    struct StaticApi(std::collections::HashMap<&'static str, Value>);
    impl NeteaseApi for StaticApi {
        fn request(
            &self,
            operation: &str,
            _: Value,
            _: &StreamingCredentials,
        ) -> Result<Value, CatalogError> {
            client::provider_response(self.0.get(operation).cloned().expect("operation"))
        }
        fn download(&self, _: &str, _: &Path, _: &dyn ResolveProgress) -> Result<(), CatalogError> {
            unreachable!("not used")
        }
    }
    fn signed_in_static(
        operations: Vec<(&'static str, Value)>,
    ) -> (tempfile::TempDir, NeteaseStreamingSource<StaticApi>) {
        let dir = tempfile::tempdir().unwrap();
        let mut map = std::collections::HashMap::from([("user_account", account())]);
        map.extend(operations);
        let source = NeteaseStreamingSource::open(StaticApi(map), dir.path()).unwrap();
        source.complete_web_sign_in(cookies()).unwrap();
        (dir, source)
    }
    #[test]
    fn privileges_match_songs_by_id_not_by_position() {
        let (_dir, source) = signed_in_static(vec![
            ("likelist", json!({"code": 200, "ids": [1, 2, 3]})),
            (
                "song_detail",
                json!({
                    "code": 200,
                    "songs": [
                        {"id": 1, "name": "One"},
                        {"id": 2, "name": "Two"},
                        {"id": 3, "name": "Three"}
                    ],
                    "privileges": [
                        {"id": 3, "st": 0},
                        {"id": 1, "st": -200},
                        {"id": 2, "st": 0}
                    ]
                }),
            ),
        ]);
        let tracks = source.liked_tracks().unwrap();
        let refused: Vec<&str> = tracks
            .iter()
            .filter(|track| track.refusal.is_some())
            .map(|track| track.remote_track_id.as_str())
            .collect();
        assert_eq!(refused, vec!["1"]);
    }
    #[test]
    fn a_song_without_a_matching_privilege_is_not_refused() {
        let (_dir, source) = signed_in_static(vec![
            ("likelist", json!({"code": 200, "ids": [1, 2]})),
            (
                "song_detail",
                json!({
                    "code": 200,
                    "songs": [{"id": 1, "name": "One"}, {"id": 2, "name": "Two"}],
                    "privileges": [{"id": 2, "st": -1}]
                }),
            ),
        ]);
        let tracks = source.liked_tracks().unwrap();
        assert!(tracks[0].refusal.is_none());
        assert!(tracks[1].refusal.is_some());
    }
    #[test]
    fn covers_are_https_and_playlist_detail_carries_its_metadata() {
        let (_dir, source) = signed_in_static(vec![
            (
                "user_playlist",
                json!({"code": 200, "playlist": [
                    {"id": 7, "name": "Night", "trackCount": 2,
                     "coverImgUrl": "http://p1.music.126.net/a.jpg"},
                    {"id": 8, "name": "Bare", "trackCount": 0, "coverImgUrl": ""}
                ]}),
            ),
            (
                "playlist_detail",
                json!({"code": 200, "playlist": {
                    "name": "Night", "trackCount": 2,
                    "coverImgUrl": "http://p2.music.126.net/b.jpg",
                    "creator": {"nickname": "Ada"},
                    "description": "  late set ",
                    "updateTime": 1700000000000_u64,
                    "trackIds": [{"id": 1}]
                }}),
            ),
            (
                "song_detail",
                json!({
                    "code": 200,
                    "songs": [{"id": 1, "name": "One",
                               "al": {"name": "Alb", "picUrl": "http://p3.music.126.net/c.jpg"}}],
                    "privileges": [{"id": 1, "st": 0}]
                }),
            ),
        ]);
        let playlists = source.playlists().unwrap();
        assert_eq!(
            playlists[0].cover_url.as_deref(),
            Some("https://p1.music.126.net/a.jpg")
        );
        assert_eq!(playlists[1].cover_url, None);
        let detail = source.playlist("7").unwrap();
        assert_eq!(
            detail.cover_url.as_deref(),
            Some("https://p2.music.126.net/b.jpg")
        );
        assert_eq!(detail.creator.as_deref(), Some("Ada"));
        assert_eq!(detail.description.as_deref(), Some("late set"));
        assert_eq!(detail.track_count, 2);
        assert_eq!(detail.updated_at_ms, Some(1_700_000_000_000));
        assert_eq!(
            detail.tracks[0].cover_url.as_deref(),
            Some("https://p3.music.126.net/c.jpg")
        );
    }
    #[test]
    fn web_sign_in_persists_verified_session_and_sign_out_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let source = NeteaseStreamingSource::open(api(account(), Value::Null), dir.path()).unwrap();
        assert!(source.complete_web_sign_in(Vec::new()).is_err());
        assert!(source
            .complete_web_sign_in(vec![("MUSIC_U".into(), "fixture-session".into())])
            .is_err());
        let session = source.complete_web_sign_in(cookies()).unwrap();
        assert_eq!(session.display_name.as_deref(), Some("Ada"));
        let reopened =
            NeteaseStreamingSource::open(api(account(), Value::Null), dir.path()).unwrap();
        assert!(reopened.session().unwrap().signed_in);
        reopened.sign_out().unwrap();
        assert!(
            !NeteaseStreamingSource::open(api(account(), Value::Null), dir.path())
                .unwrap()
                .session()
                .unwrap()
                .signed_in
        );
    }
    #[test]
    fn rejected_and_expired_sessions_are_not_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let source =
            NeteaseStreamingSource::open(api(json!({"code":8860}), Value::Null), dir.path())
                .unwrap();
        assert!(
            matches!(source.complete_web_sign_in(cookies()), Err(CatalogError::AuthFailed { detail, .. }) if detail.contains("8860"))
        );
        assert!(!source.session().unwrap().signed_in);
        let valid = NeteaseStreamingSource::open(api(account(), Value::Null), dir.path()).unwrap();
        valid.complete_web_sign_in(cookies()).unwrap();
        let expired =
            NeteaseStreamingSource::open(api(json!({"code":301}), Value::Null), dir.path())
                .unwrap();
        let session = expired.session().unwrap();
        assert!(!session.signed_in);
        assert!(session.expired);
    }
    #[test]
    fn full_audio_import_is_allowed_but_trial_and_empty_urls_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        for (format, extension) in [("flac", "flac"), ("FLAC", "flac"), ("MP3", "mp3")] {
            let source = NeteaseStreamingSource::open(
                api(
                    account(),
                    json!({"url":"https://audio.example/full","type":format,"freeTrialInfo":null}),
                ),
                dir.path(),
            )
            .unwrap();
            source.complete_web_sign_in(cookies()).unwrap();
            match source
                .resolve("1", &crate::catalog::NoResolveProgress)
                .unwrap()
            {
                StreamingResolveOutcome::File(file) => {
                    assert_eq!(
                        file.path.extension().and_then(|value| value.to_str()),
                        Some(extension)
                    );
                    assert_eq!(std::fs::read(&file.path).unwrap(), b"complete audio");
                    std::fs::remove_file(file.path).unwrap();
                }
                _ => panic!("full song refused"),
            }
        }
        for (url, reason) in [
            (
                json!({"url":"https://audio.example/trial.mp3","freeTrialInfo":{"end":30}}),
                ImportRefusalReason::TrialClip,
            ),
            (json!({"url":null}), ImportRefusalReason::EmptyUrl),
        ] {
            let source = NeteaseStreamingSource::open(api(account(), url), dir.path()).unwrap();
            assert!(
                matches!(source.resolve("1", &crate::catalog::NoResolveProgress).unwrap(), StreamingResolveOutcome::Refusal(refusal) if refusal.reason == reason)
            );
        }
    }

    #[test]
    fn preview_streams_full_audio_and_refuses_trial_clips_without_downloading() {
        let dir = tempfile::tempdir().unwrap();
        let open = |url: Value| {
            let source = NeteaseStreamingSource::open(api(account(), url), dir.path()).unwrap();
            source.complete_web_sign_in(cookies()).unwrap();
            source
        };
        match open(json!({"url":"https://audio.example/full","type":"MP3","freeTrialInfo":null}))
            .preview("1")
            .unwrap()
        {
            StreamingPreviewOutcome::Stream(stream) => {
                assert_eq!(stream.url, "https://audio.example/full");
                assert_eq!(stream.extension, "mp3");
            }
            StreamingPreviewOutcome::Refusal(_) => panic!("full song refused"),
        }
        for (url, reason) in [
            (
                json!({"url":"https://audio.example/trial.mp3","freeTrialInfo":{"end":30}}),
                ImportRefusalReason::TrialClip,
            ),
            (json!({"url":null}), ImportRefusalReason::EmptyUrl),
        ] {
            assert!(matches!(
                open(url).preview("1").unwrap(),
                StreamingPreviewOutcome::Refusal(refusal) if refusal.reason == reason
            ));
        }
        // Listing-time refusals never reach song_url.
        assert!(matches!(
            open(Value::Null).preview("1001").unwrap(),
            StreamingPreviewOutcome::Refusal(refusal)
                if refusal.reason == ImportRefusalReason::NoPlayRights
        ));
        assert!(open(Value::Null).preview("abc").is_err());
    }

    #[test]
    fn privileges_follow_the_youtube_music_playability_rules() {
        let song = |extra: Value| {
            let mut song = json!({"id": 1, "name": "S", "ar": [{"name": "A"}]});
            song.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            song
        };
        let reason =
            |song: Value, privilege: Value| refusal_from_privilege(&song, Some(&privilege));
        // Playable rights win over everything else.
        assert_eq!(
            reason(song(json!({})), json!({"pl": 128000, "fee": 1})),
            None
        );
        // Cloud-storage tracks play for the owner.
        assert_eq!(
            reason(song(json!({})), json!({"pl": 0, "cs": true, "fee": 1})),
            None
        );
        // Self Control: VIP only, served as a 30-second clip.
        assert_eq!(
            reason(song(json!({})), json!({"fee": 1, "pl": 0, "st": 0})),
            Some(ImportRefusalReason::TrialClip)
        );
        assert_eq!(
            reason(song(json!({"fee": 1})), json!({"pl": 0})),
            Some(ImportRefusalReason::TrialClip)
        );
        // Paid album, no copyright and taken-down tracks have no play rights.
        assert_eq!(
            reason(song(json!({})), json!({"fee": 4, "pl": 0})),
            Some(ImportRefusalReason::NoPlayRights)
        );
        assert_eq!(
            reason(
                song(json!({"noCopyrightRcmd": {"type": 1}})),
                json!({"pl": 0})
            ),
            Some(ImportRefusalReason::NoPlayRights)
        );
        assert_eq!(
            reason(song(json!({"noCopyrightRcmd": null})), json!({"pl": 0})),
            None
        );
        assert_eq!(
            reason(song(json!({})), json!({"fee": 0, "pl": 0, "st": -200})),
            Some(ImportRefusalReason::NoPlayRights)
        );
        assert_eq!(refusal_from_privilege(&song(json!({})), None), None);
        // Search results carry the privilege on the song itself.
        let searched =
            track_from_json(&song(json!({"privilege": {"fee": 1, "pl": 0}})), None).unwrap();
        assert_eq!(
            searched.refusal.unwrap().reason,
            ImportRefusalReason::TrialClip
        );
    }

    #[test]
    fn the_playlist_head_needs_no_song_detail_and_pages_fill_the_rest() {
        let (_dir, source) = signed_in_static(vec![
            (
                "playlist_detail",
                json!({"code": 200, "playlist": {
                    "name": "Big", "trackCount": 3,
                    "tracks": [{"id": 1, "name": "One", "ar": [{"name": "A"}]}],
                    "trackIds": [{"id": 1}, {"id": 2}, {"id": 3}]
                }, "privileges": [{"id": 1, "fee": 0, "pl": 320000}]}),
            ),
            (
                "song_detail",
                json!({"code": 200,
                    "songs": [{"id": 2, "name": "Two"}, {"id": 3, "name": "Three"}],
                    "privileges": [{"id": 2, "fee": 1, "pl": 0}, {"id": 3, "pl": 1}]}),
            ),
        ]);
        let head = source.playlist_head("9").unwrap();
        assert!(head.partial);
        assert_eq!(head.track_ids, ["1", "2", "3"]);
        assert_eq!(head.tracks.len(), 1);
        assert_eq!(head.track_count, 3);
        let page = source.tracks(&["2".to_owned(), "3".to_owned()]).unwrap();
        assert_eq!(
            page[0].refusal.as_ref().unwrap().reason,
            ImportRefusalReason::TrialClip
        );
        assert_eq!(page[0].refusal.as_ref().unwrap().title, "Two");
        assert!(page[1].refusal.is_none());
        let full = source.playlist("9").unwrap();
        assert!(!full.partial);
        assert_eq!(full.tracks.len(), 3);
    }

    struct CountingApi {
        song_detail_calls: std::sync::atomic::AtomicUsize,
    }
    impl NeteaseApi for CountingApi {
        fn request(
            &self,
            operation: &str,
            params: Value,
            _: &StreamingCredentials,
        ) -> Result<Value, CatalogError> {
            let body = match operation {
                "user_account" => account(),
                "song_detail" => {
                    self.song_detail_calls
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let ids: Vec<u64> = params["ids"]
                        .as_str()
                        .unwrap()
                        .split(',')
                        .map(|id| id.parse().unwrap())
                        .collect();
                    json!({"code": 200,
                        "songs": ids.iter().map(|id| json!({"id": id, "name": "S"})).collect::<Vec<_>>(),
                        "privileges": []})
                }
                _ => panic!("unexpected operation"),
            };
            client::provider_response(body)
        }
        fn download(&self, _: &str, _: &Path, _: &dyn ResolveProgress) -> Result<(), CatalogError> {
            unreachable!("not used")
        }
    }

    #[test]
    fn the_song_cache_avoids_refetching_and_is_shared_between_sources() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(NeteaseSongCache::default());
        let open = || {
            let source = NeteaseStreamingSource::open(
                CountingApi {
                    song_detail_calls: Default::default(),
                },
                dir.path(),
            )
            .unwrap()
            .with_cache(cache.clone());
            source
        };
        let first = open();
        first.complete_web_sign_in(cookies()).unwrap();
        let ids: Vec<String> = (1..=1200).map(|id| id.to_string()).collect();
        assert_eq!(first.tracks(&ids).unwrap().len(), 1200);
        // 1200 ids are fetched as three pages of at most 500.
        assert_eq!(
            first
                .http
                .song_detail_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            3
        );
        let second = open();
        let again = second
            .tracks_by_ids(
                &StreamingCredentials {
                    music_u: "x".into(),
                    csrf: "y".into(),
                },
                &ids,
            )
            .unwrap();
        assert_eq!(again.len(), 1200);
        assert_eq!(
            second
                .http
                .song_detail_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "a second source reads the shared cache"
        );
        // Signing out forgets rights-dependent data.
        first.sign_out().unwrap();
        assert!(cache.get("1").is_none());
    }

    /// Latency model scaled 1:10 from a measured 3097-track liked playlist
    /// (playlist_detail 3.7 s; song_detail ~0.2 s + 5 ms per id).
    struct LatencyApi;
    impl NeteaseApi for LatencyApi {
        fn request(
            &self,
            operation: &str,
            params: Value,
            _: &StreamingCredentials,
        ) -> Result<Value, CatalogError> {
            let sleep = |ms: u64| std::thread::sleep(std::time::Duration::from_millis(ms));
            let body = match operation {
                "user_account" => account(),
                "playlist_detail" => {
                    sleep(370);
                    json!({"code": 200, "playlist": {
                        "name": "Liked", "trackCount": 3097,
                        "tracks": (1..=1000).map(|id| json!({"id": id, "name": "S"})).collect::<Vec<_>>(),
                        "trackIds": (1..=3097).map(|id| json!({"id": id})).collect::<Vec<_>>()
                    }, "privileges": (1..=1000).map(|id| json!({"id": id, "pl": 1})).collect::<Vec<_>>()})
                }
                "song_detail" => {
                    let ids: Vec<u64> = params["ids"]
                        .as_str()
                        .unwrap()
                        .split(',')
                        .map(|id| id.parse().unwrap())
                        .collect();
                    sleep(20 + ids.len() as u64 / 2);
                    json!({"code": 200,
                        "songs": ids.iter().map(|id| json!({"id": id, "name": "S"})).collect::<Vec<_>>(),
                        "privileges": ids.iter().map(|id| json!({"id": id, "pl": 1})).collect::<Vec<_>>()})
                }
                _ => panic!("unexpected operation"),
            };
            client::provider_response(body)
        }
        fn download(&self, _: &str, _: &Path, _: &dyn ResolveProgress) -> Result<(), CatalogError> {
            unreachable!("not used")
        }
    }

    #[test]
    fn paged_loading_shows_the_list_far_sooner_than_loading_everything() {
        use std::time::Instant;
        let open = || {
            let dir = tempfile::tempdir().unwrap();
            let source = NeteaseStreamingSource::open(LatencyApi, dir.path()).unwrap();
            source.complete_web_sign_in(cookies()).unwrap();
            (dir, source)
        };
        // Before: wait for playlist_detail and four serial song_detail(1000).
        let (_dir, old) = open();
        let started = Instant::now();
        let credentials = old.require_credentials().unwrap();
        old.http
            .request("playlist_detail", json!({}), &credentials)
            .unwrap();
        let ids: Vec<String> = (1..=3097).map(|id| id.to_string()).collect();
        for chunk in ids.chunks(1000) {
            old.http
                .request("song_detail", json!({"ids": chunk.join(",")}), &credentials)
                .unwrap();
        }
        let before = started.elapsed();
        // After: the head is ready after playlist_detail alone; the remaining
        // ids load in 500-id pages, three in flight.
        let (_dir, new) = open();
        let started = Instant::now();
        let head = new.playlist_head("1").unwrap();
        let head_ready = started.elapsed();
        assert_eq!(head.tracks.len(), 1000);
        let rest: Vec<String> = head.track_ids[1000..].to_vec();
        let pages: Vec<&[String]> = rest.chunks(TRACK_PAGE_SIZE).collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..3 {
                scope.spawn(|| loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some(page) = pages.get(index) else { break };
                    assert_eq!(new.tracks(page).unwrap().len(), page.len());
                });
            }
        });
        let all_loaded = started.elapsed();
        eprintln!(
            "timing (scaled 1:10, x10 = live ms): before first-list={}ms all={}ms; after first-list={}ms all={}ms",
            before.as_millis() * 10,
            before.as_millis() * 10,
            head_ready.as_millis() * 10,
            all_loaded.as_millis() * 10
        );
        assert!(
            head_ready * 4 < before,
            "the head must not wait on song_detail"
        );
        assert!(
            all_loaded < before,
            "concurrent pages finish sooner overall"
        );
        // Reopening is free: everything is cached.
        let reopened = Instant::now();
        new.tracks(&head.track_ids).unwrap();
        assert!(reopened.elapsed() < std::time::Duration::from_millis(50));
    }
}
