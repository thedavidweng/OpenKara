mod client;

pub use client::{LiveNeteaseApi, NeteaseApi, NeteaseSidecar};

use super::streaming::StreamingSource;
use super::types::{
    CatalogError, ImportRefusal, ImportRefusalReason, ResolvedStreamingFile, StreamingCredentials,
    StreamingPlaylistDetail, StreamingPlaylistSummary, StreamingResolveOutcome,
    StreamingSessionSnapshot, StreamingTrack,
};
use crate::catalog::credentials;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct NeteaseStreamingSource<H> {
    pub(crate) http: H,
    app_data_dir: PathBuf,
    credentials: Mutex<Option<StreamingCredentials>>,
    display_name: Mutex<Option<String>>,
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
        let account = self.http.request("user_account", json!({}), &credentials)?;
        let uid = account
            .pointer("/account/id")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
            .ok_or_else(|| CatalogError::SessionExpired {
                source_id: "netease".to_owned(),
            })?;
        let liked = self
            .http
            .request("likelist", json!({ "uid": uid }), &credentials)?;
        let ids = liked
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
                })
            })
            .collect())
    }

    fn playlist(&self, remote_id: &str) -> Result<StreamingPlaylistDetail, CatalogError> {
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
        let tracks = response
            .pointer("/playlist/tracks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let privileges = response
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

    fn resolve(&self, remote_track_id: &str) -> Result<StreamingResolveOutcome, CatalogError> {
        if remote_track_id.parse::<u64>().is_err() {
            return Err(CatalogError::Internal(
                "Invalid NetEase track ID".to_owned(),
            ));
        }
        let credentials = self.require_credentials()?;
        let detail = self.http.request(
            "song_detail",
            json!({ "ids": remote_track_id }),
            &credentials,
        )?;
        let song = detail.pointer("/songs/0").cloned().unwrap_or(Value::Null);
        let privilege = detail.pointer("/privileges/0");
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

        let extension = match data.get("type").and_then(Value::as_str) {
            Some(extension @ ("mp3" | "flac" | "wav" | "ogg" | "aac" | "m4a")) => extension,
            _ => {
                return Err(CatalogError::Internal(
                    "NetEase returned an unsupported audio format".to_owned(),
                ))
            }
        };
        let dest = temp_download_path(extension);
        self.http.download(url, &dest)?;
        Ok(StreamingResolveOutcome::File(ResolvedStreamingFile {
            path: dest,
            title: Some(track.title),
            artist: Some(track.artist),
            album: track.album,
        }))
    }
}

impl<H: NeteaseApi> NeteaseStreamingSource<H> {
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
        let detail =
            self.http
                .request("song_detail", json!({ "ids": ids.join(",") }), credentials)?;
        let songs = detail
            .get("songs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let privileges = detail
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
            _: Value,
            _: &StreamingCredentials,
        ) -> Result<Value, CatalogError> {
            let body = match operation {
                "user_account" => self.account.clone(),
                "song_detail" => {
                    json!({"code":200,"songs":[{"id":1,"name":"Song","ar":[{"name":"Artist"}]}],"privileges":[{"fee":1,"st":0}]})
                }
                "song_url" => json!({"code":200,"data":[self.url.clone()]}),
                _ => panic!("unexpected operation"),
            };
            client::provider_response(body)
        }
        fn download(&self, _: &str, dest: &Path) -> Result<(), CatalogError> {
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
        let source = NeteaseStreamingSource::open(
            api(
                account(),
                json!({"url":"https://audio.example/full.flac","type":"flac","freeTrialInfo":null}),
            ),
            dir.path(),
        )
        .unwrap();
        source.complete_web_sign_in(cookies()).unwrap();
        match source.resolve("1").unwrap() {
            StreamingResolveOutcome::File(file) => {
                assert_eq!(
                    file.path.extension().and_then(|value| value.to_str()),
                    Some("flac")
                );
                assert_eq!(std::fs::read(&file.path).unwrap(), b"complete audio");
                std::fs::remove_file(file.path).unwrap();
            }
            _ => panic!("full song refused"),
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
                matches!(source.resolve("1").unwrap(), StreamingResolveOutcome::Refusal(refusal) if refusal.reason == reason)
            );
        }
    }
}
