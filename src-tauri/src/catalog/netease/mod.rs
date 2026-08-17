mod client;
mod crypto;

pub use client::{china_client_address, hashed_password, LiveNeteaseHttp, NeteaseHttp};

use super::streaming::StreamingSource;
use super::types::{
    CatalogError, ImportRefusal, ImportRefusalReason, ResolvedStreamingFile, StreamingCredentials,
    StreamingPasswordMethod, StreamingPlaylistDetail, StreamingPlaylistSummary,
    StreamingQrChallenge, StreamingQrPoll, StreamingQrStatus, StreamingResolveOutcome,
    StreamingSessionSnapshot, StreamingTrack,
};
use crate::catalog::credentials;
use qrcode::render::svg;
use qrcode::QrCode;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct NeteaseStreamingSource<H> {
    pub(crate) http: H,
    app_data_dir: PathBuf,
    credentials: Mutex<Option<StreamingCredentials>>,
    display_name: Mutex<Option<String>>,
}

impl<H: NeteaseHttp> NeteaseStreamingSource<H> {
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
        })
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
        if let Ok(mut guard) = self.credentials.lock() {
            *guard = Some(credentials);
        }
        if let Ok(mut guard) = self.display_name.lock() {
            *guard = display_name;
        }
        Ok(())
    }

    fn clear(&self) -> Result<(), CatalogError> {
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

    fn credentials_from_cookies(
        &self,
        cookies: &std::collections::HashMap<String, String>,
        fallback: Option<StreamingCredentials>,
    ) -> Option<StreamingCredentials> {
        let music_u = cookies
            .get("MUSIC_U")
            .cloned()
            .or_else(|| fallback.as_ref().map(|value| value.music_u.clone()))?;
        let csrf = cookies
            .get("__csrf")
            .cloned()
            .or_else(|| fallback.as_ref().map(|value| value.csrf.clone()))
            .unwrap_or_default();
        Some(StreamingCredentials { music_u, csrf })
    }

    fn merge_login_cookies(
        cookies: &std::collections::HashMap<String, String>,
        json: &Value,
    ) -> std::collections::HashMap<String, String> {
        let mut merged = cookies.clone();
        if let Some(cookie) = json.get("cookie").and_then(Value::as_str) {
            for part in cookie.split(";;") {
                let pair = part.split(';').next().unwrap_or(part);
                if let Some((name, value)) = pair.split_once('=') {
                    let name = name.trim();
                    if name == "MUSIC_U" || name == "__csrf" {
                        merged.insert(name.to_owned(), value.trim().to_owned());
                    }
                }
            }
        }
        merged
    }

    fn load_account_name(
        &self,
        credentials: &StreamingCredentials,
    ) -> Result<Option<String>, CatalogError> {
        let response =
            self.http
                .post_weapi("/weapi/w/nuser/account/get", json!({}), Some(credentials))?;
        let name = response
            .json
            .pointer("/profile/nickname")
            .and_then(Value::as_str)
            .or_else(|| {
                response
                    .json
                    .pointer("/account/userName")
                    .and_then(Value::as_str)
            })
            .map(ToOwned::to_owned);
        Ok(name)
    }

    fn require_credentials(&self) -> Result<StreamingCredentials, CatalogError> {
        self.credentials()
            .ok_or_else(|| CatalogError::SessionExpired {
                source_id: "netease".to_owned(),
            })
    }
}

impl<H: NeteaseHttp> StreamingSource for NeteaseStreamingSource<H> {
    fn source_id(&self) -> &str {
        "netease"
    }

    fn session(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        let credentials = self.credentials();
        let display_name = self
            .display_name
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        Ok(StreamingSessionSnapshot {
            source_id: "netease".to_owned(),
            signed_in: credentials.is_some(),
            display_name,
            expired: false,
        })
    }

    fn start_qr(&self) -> Result<StreamingQrChallenge, CatalogError> {
        let response =
            self.http
                .post_weapi("/weapi/login/qrcode/unikey", json!({ "type": 1 }), None)?;
        let key = response
            .json
            .get("unikey")
            .and_then(Value::as_str)
            .ok_or_else(|| CatalogError::Internal("NetEase QR key was missing".to_owned()))?
            .to_owned();
        let login_url = format!("https://music.163.com/login?codekey={key}");
        let qr_svg = QrCode::new(login_url.as_bytes())
            .map(|code| {
                code.render::<svg::Color<'_>>()
                    .min_dimensions(160, 160)
                    .build()
            })
            .unwrap_or_else(|_| "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".to_owned());
        Ok(StreamingQrChallenge {
            key,
            login_url,
            qr_svg,
        })
    }

    fn poll_qr(&self, key: &str) -> Result<StreamingQrPoll, CatalogError> {
        let response = self.http.post_weapi(
            "/weapi/login/qrcode/client/login",
            json!({ "key": key, "type": 1 }),
            None,
        )?;
        let code = response
            .json
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        match code {
            801 => Ok(StreamingQrPoll {
                status: StreamingQrStatus::Waiting,
                session: None,
            }),
            802 => Ok(StreamingQrPoll {
                status: StreamingQrStatus::Scanned,
                session: None,
            }),
            803 => {
                let cookies = Self::merge_login_cookies(&response.cookies, &response.json);
                let credentials =
                    self.credentials_from_cookies(&cookies, None)
                        .ok_or_else(|| {
                            CatalogError::Internal(
                                "QR login did not return Streaming Credentials".to_owned(),
                            )
                        })?;
                let display_name = self.load_account_name(&credentials)?;
                self.persist(credentials, display_name.clone())?;
                Ok(StreamingQrPoll {
                    status: StreamingQrStatus::Confirmed,
                    session: Some(StreamingSessionSnapshot {
                        source_id: "netease".to_owned(),
                        signed_in: true,
                        display_name,
                        expired: false,
                    }),
                })
            }
            _ => Ok(StreamingQrPoll {
                status: StreamingQrStatus::Expired,
                session: None,
            }),
        }
    }

    fn sign_in_password(
        &self,
        method: StreamingPasswordMethod,
        identifier: &str,
        password: &str,
        country_code: Option<&str>,
    ) -> Result<StreamingSessionSnapshot, CatalogError> {
        let hashed = hashed_password(password);
        let response = match method {
            StreamingPasswordMethod::Phone => self.http.post_weapi(
                "/weapi/login/cellphone",
                json!({
                    "phone": identifier,
                    "countrycode": country_code.unwrap_or("86"),
                    "password": hashed,
                    "rememberLogin": "true"
                }),
                None,
            )?,
            StreamingPasswordMethod::Email => self.http.post_weapi(
                "/weapi/login",
                json!({
                    "username": identifier,
                    "password": hashed,
                    "rememberLogin": "true"
                }),
                None,
            )?,
        };
        let _ = password;
        let cookies = Self::merge_login_cookies(&response.cookies, &response.json);
        let credentials = self
            .credentials_from_cookies(&cookies, None)
            .ok_or_else(|| {
                CatalogError::Internal(
                    "password sign-in did not return Streaming Credentials".to_owned(),
                )
            })?;
        if credentials.contains_password_material(password)
            || credentials.contains_password_material(&hashed)
        {
            return Err(CatalogError::Internal(
                "refusing to store a password as Streaming Credentials".to_owned(),
            ));
        }
        let display_name = self
            .load_account_name(&credentials)?
            .or_else(|| Some(identifier.to_owned()));
        self.persist(credentials, display_name.clone())?;
        Ok(StreamingSessionSnapshot {
            source_id: "netease".to_owned(),
            signed_in: true,
            display_name,
            expired: false,
        })
    }

    fn sign_out(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        if let Some(credentials) = self.credentials() {
            let _ = self
                .http
                .post_weapi("/weapi/logout", json!({}), Some(&credentials));
        }
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
        let account =
            self.http
                .post_weapi("/weapi/w/nuser/account/get", json!({}), Some(&credentials))?;
        let uid = account
            .json
            .pointer("/account/id")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let liked = self.http.post_weapi(
            "/weapi/song/like/get",
            json!({ "uid": uid }),
            Some(&credentials),
        )?;
        let ids = liked
            .json
            .get("ids")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let id_list: Vec<String> = ids
            .iter()
            .filter_map(|value| {
                value
                    .as_i64()
                    .map(|id| id.to_string())
                    .or_else(|| value.as_str().map(ToOwned::to_owned))
            })
            .collect();
        self.tracks_by_ids(&credentials, &id_list)
    }

    fn playlists(&self) -> Result<Vec<StreamingPlaylistSummary>, CatalogError> {
        let credentials = self.require_credentials()?;
        let account =
            self.http
                .post_weapi("/weapi/w/nuser/account/get", json!({}), Some(&credentials))?;
        let uid = account
            .json
            .pointer("/account/id")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let response = self.http.post_weapi(
            "/weapi/user/playlist",
            json!({ "uid": uid, "limit": 200, "offset": 0 }),
            Some(&credentials),
        )?;
        let playlists = response
            .json
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
                })
            })
            .collect())
    }

    fn playlist(&self, remote_id: &str) -> Result<StreamingPlaylistDetail, CatalogError> {
        let credentials = self.require_credentials()?;
        let response = self.http.post_weapi(
            "/weapi/v6/playlist/detail",
            json!({ "id": remote_id, "n": 1000, "s": 0 }),
            Some(&credentials),
        )?;
        let name = response
            .json
            .pointer("/playlist/name")
            .and_then(Value::as_str)
            .unwrap_or("Streaming Playlist")
            .to_owned();
        let tracks = response
            .json
            .pointer("/playlist/tracks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let privileges = response
            .json
            .get("privileges")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(StreamingPlaylistDetail {
            remote_id: remote_id.to_owned(),
            name,
            tracks: tracks
                .into_iter()
                .enumerate()
                .filter_map(|(index, track)| track_from_json(&track, privileges.get(index)))
                .collect(),
        })
    }

    fn search(&self, query: &str) -> Result<Vec<StreamingTrack>, CatalogError> {
        let credentials = self.require_credentials()?;
        let response = self.http.post_weapi(
            "/weapi/cloudsearch/get/web",
            json!({
                "s": query,
                "type": 1,
                "limit": 30,
                "offset": 0
            }),
            Some(&credentials),
        )?;
        let songs = response
            .json
            .pointer("/result/songs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(songs
            .into_iter()
            .filter_map(|song| track_from_json(&song, None))
            .collect())
    }

    fn resolve(&self, remote_track_id: &str) -> Result<StreamingResolveOutcome, CatalogError> {
        let credentials = self.require_credentials()?;
        let detail = self.http.post_weapi(
            "/weapi/v3/song/detail",
            json!({ "c": format!("[{{\"id\":{remote_track_id}}}]") }),
            Some(&credentials),
        )?;
        let song = detail
            .json
            .pointer("/songs/0")
            .cloned()
            .unwrap_or(Value::Null);
        let privilege = detail.json.pointer("/privileges/0");
        let track = track_from_json(&song, privilege).unwrap_or(StreamingTrack {
            source_id: "netease".to_owned(),
            remote_track_id: remote_track_id.to_owned(),
            title: remote_track_id.to_owned(),
            artist: String::new(),
            album: None,
            duration_ms: None,
            refusal: None,
        });
        if let Some(refusal) = track.refusal.clone() {
            return Ok(StreamingResolveOutcome::Refusal(refusal));
        }

        let url_response = self.http.post_weapi(
            "/weapi/song/enhance/player/url/v1",
            json!({
                "ids": [remote_track_id],
                "level": "exhigh",
                "encodeType": "mp3"
            }),
            Some(&credentials),
        )?;
        let data = url_response
            .json
            .pointer("/data/0")
            .cloned()
            .unwrap_or(Value::Null);
        if data.get("freeTrialInfo").is_some() && !data.get("freeTrialInfo").unwrap().is_null() {
            return Ok(StreamingResolveOutcome::Refusal(ImportRefusal {
                reason: ImportRefusalReason::TrialClip,
                title: track.title,
                artist: track.artist,
            }));
        }
        let url = data.get("url").and_then(Value::as_str).unwrap_or("");
        if url.is_empty() {
            return Ok(StreamingResolveOutcome::Refusal(ImportRefusal {
                reason: ImportRefusalReason::EmptyUrl,
                title: track.title,
                artist: track.artist,
            }));
        }

        let dest = temp_download_path(remote_track_id);
        self.http.download(url, &dest, Some(&credentials))?;
        Ok(StreamingResolveOutcome::File(ResolvedStreamingFile {
            path: dest,
            title: Some(track.title),
            artist: Some(track.artist),
            album: track.album,
        }))
    }
}

impl<H: NeteaseHttp> NeteaseStreamingSource<H> {
    fn tracks_by_ids(
        &self,
        credentials: &StreamingCredentials,
        ids: &[String],
    ) -> Result<Vec<StreamingTrack>, CatalogError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let c = ids
            .iter()
            .map(|id| format!("{{\"id\":{id}}}"))
            .collect::<Vec<_>>()
            .join(",");
        let detail = self.http.post_weapi(
            "/weapi/v3/song/detail",
            json!({ "c": format!("[{c}]") }),
            Some(credentials),
        )?;
        let songs = detail
            .json
            .get("songs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let privileges = detail
            .json
            .get("privileges")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(songs
            .into_iter()
            .enumerate()
            .filter_map(|(index, song)| track_from_json(&song, privileges.get(index)))
            .collect())
    }
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
    let refusal = privilege
        .and_then(refusal_from_privilege)
        .map(|reason| ImportRefusal {
            reason,
            title: title.clone(),
            artist: artist.clone(),
        });
    Some(StreamingTrack {
        source_id: "netease".to_owned(),
        remote_track_id,
        title,
        artist,
        album,
        duration_ms,
        refusal,
    })
}

fn refusal_from_privilege(privilege: &Value) -> Option<ImportRefusalReason> {
    let st = privilege.get("st").and_then(Value::as_i64).unwrap_or(0);
    if st < 0 {
        return Some(ImportRefusalReason::NoPlayRights);
    }
    let fee = privilege.get("fee").and_then(Value::as_i64).unwrap_or(0);
    match fee {
        1 => Some(ImportRefusalReason::TrialClip),
        4 => Some(ImportRefusalReason::NoPlayRights),
        _ => None,
    }
}

fn temp_download_path(remote_track_id: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("openkara-netease-{remote_track_id}-{nanos}.mp3"))
}

#[cfg(test)]
pub fn recording_http_with_china_ip() -> RecordingNeteaseHttp {
    RecordingNeteaseHttp {
        last_address: Mutex::new(None),
        last_path: Mutex::new(None),
    }
}

#[cfg(test)]
pub struct RecordingNeteaseHttp {
    last_address: Mutex<Option<String>>,
    last_path: Mutex<Option<String>>,
}

#[cfg(test)]
impl NeteaseHttp for RecordingNeteaseHttp {
    fn post_weapi(
        &self,
        path: &str,
        _payload: Value,
        _credentials: Option<&StreamingCredentials>,
    ) -> Result<crate::catalog::netease::client::NeteaseHttpResponse, CatalogError> {
        let address = china_client_address();
        if let Ok(mut last) = self.last_address.lock() {
            *last = Some(address);
        }
        if let Ok(mut last) = self.last_path.lock() {
            *last = Some(path.to_owned());
        }
        Ok(crate::catalog::netease::client::NeteaseHttpResponse {
            json: json!({ "code": 200, "unikey": "recorded" }),
            cookies: Default::default(),
        })
    }

    fn download(
        &self,
        _url: &str,
        _dest: &Path,
        _credentials: Option<&StreamingCredentials>,
    ) -> Result<(), CatalogError> {
        Ok(())
    }

    fn last_china_address(&self) -> Option<String> {
        self.last_address
            .lock()
            .ok()
            .and_then(|value| value.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_sends_china_client_address() {
        let http = recording_http_with_china_ip();
        let dir = tempfile::tempdir().expect("tmp");
        std::env::set_var("OPENKARA_TEST_CREDENTIAL_STORE_DIR", dir.path());
        let source = NeteaseStreamingSource::open(http, dir.path()).expect("open");
        let _ = source.start_qr();
        assert!(source.http.last_china_address().is_some());
        std::env::remove_var("OPENKARA_TEST_CREDENTIAL_STORE_DIR");
    }
}
