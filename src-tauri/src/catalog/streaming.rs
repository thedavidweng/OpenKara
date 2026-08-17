use super::types::{
    CatalogError, ImportRefusal, ImportRefusalReason, ResolvedStreamingFile, StreamingCredentials,
    StreamingPasswordMethod, StreamingPlaylistDetail, StreamingPlaylistSummary,
    StreamingQrChallenge, StreamingQrPoll, StreamingQrStatus, StreamingResolveOutcome,
    StreamingSessionSnapshot, StreamingTrack,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub trait StreamingSource: Send + Sync {
    fn source_id(&self) -> &str;
    fn session(&self) -> Result<StreamingSessionSnapshot, CatalogError>;
    fn start_qr(&self) -> Result<StreamingQrChallenge, CatalogError>;
    fn poll_qr(&self, key: &str) -> Result<StreamingQrPoll, CatalogError>;
    fn sign_in_password(
        &self,
        method: StreamingPasswordMethod,
        identifier: &str,
        password: &str,
        country_code: Option<&str>,
    ) -> Result<StreamingSessionSnapshot, CatalogError>;
    fn sign_out(&self) -> Result<StreamingSessionSnapshot, CatalogError>;
    fn liked_tracks(&self) -> Result<Vec<StreamingTrack>, CatalogError>;
    fn playlists(&self) -> Result<Vec<StreamingPlaylistSummary>, CatalogError>;
    fn playlist(&self, remote_id: &str) -> Result<StreamingPlaylistDetail, CatalogError>;
    fn search(&self, query: &str) -> Result<Vec<StreamingTrack>, CatalogError>;
    fn resolve(&self, remote_track_id: &str) -> Result<StreamingResolveOutcome, CatalogError>;
}

pub struct GatedStreamingSource<S> {
    enabled: bool,
    inner: S,
}

impl<S: StreamingSource> GatedStreamingSource<S> {
    pub fn new(enabled: bool, inner: S) -> Self {
        Self { enabled, inner }
    }

    fn require_enabled(&self) -> Result<(), CatalogError> {
        if self.enabled {
            Ok(())
        } else {
            Err(CatalogError::SourceDisabled {
                source_id: self.inner.source_id().to_owned(),
            })
        }
    }
}

impl<S: StreamingSource> StreamingSource for GatedStreamingSource<S> {
    fn source_id(&self) -> &str {
        self.inner.source_id()
    }

    fn session(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        self.inner.session()
    }

    fn start_qr(&self) -> Result<StreamingQrChallenge, CatalogError> {
        self.require_enabled()?;
        self.inner.start_qr()
    }

    fn poll_qr(&self, key: &str) -> Result<StreamingQrPoll, CatalogError> {
        self.require_enabled()?;
        self.inner.poll_qr(key)
    }

    fn sign_in_password(
        &self,
        method: StreamingPasswordMethod,
        identifier: &str,
        password: &str,
        country_code: Option<&str>,
    ) -> Result<StreamingSessionSnapshot, CatalogError> {
        self.require_enabled()?;
        self.inner
            .sign_in_password(method, identifier, password, country_code)
    }

    fn sign_out(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        self.require_enabled()?;
        self.inner.sign_out()
    }

    fn liked_tracks(&self) -> Result<Vec<StreamingTrack>, CatalogError> {
        self.require_enabled()?;
        self.inner.liked_tracks()
    }

    fn playlists(&self) -> Result<Vec<StreamingPlaylistSummary>, CatalogError> {
        self.require_enabled()?;
        self.inner.playlists()
    }

    fn playlist(&self, remote_id: &str) -> Result<StreamingPlaylistDetail, CatalogError> {
        self.require_enabled()?;
        self.inner.playlist(remote_id)
    }

    fn search(&self, query: &str) -> Result<Vec<StreamingTrack>, CatalogError> {
        self.require_enabled()?;
        self.inner.search(query)
    }

    fn resolve(&self, remote_track_id: &str) -> Result<StreamingResolveOutcome, CatalogError> {
        self.require_enabled()?;
        self.inner.resolve(remote_track_id)
    }
}

#[derive(Clone)]
pub struct FakeTrackSpec {
    pub remote_track_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub resolve: FakeResolveSpec,
}

#[derive(Clone)]
pub enum FakeResolveSpec {
    File { path: PathBuf },
    Refusal(ImportRefusalReason),
}

struct FakeInner {
    credentials: Option<StreamingCredentials>,
    display_name: Option<String>,
    last_password: Option<String>,
    qr_key: Option<String>,
    qr_polls: u32,
    expired: bool,
    tracks: HashMap<String, FakeTrackSpec>,
    playlists: Vec<StreamingPlaylistSummary>,
    playlist_tracks: HashMap<String, Vec<String>>,
}

pub struct FakeStreamingSource {
    source_id: String,
    inner: Mutex<FakeInner>,
}

impl FakeStreamingSource {
    pub fn new(source_id: impl Into<String>) -> Self {
        Self {
            source_id: source_id.into(),
            inner: Mutex::new(FakeInner {
                credentials: None,
                display_name: None,
                last_password: None,
                qr_key: None,
                qr_polls: 0,
                expired: false,
                tracks: HashMap::new(),
                playlists: Vec::new(),
                playlist_tracks: HashMap::new(),
            }),
        }
    }

    pub fn insert_track(&self, spec: FakeTrackSpec) {
        let mut inner = self.inner.lock().expect("fake source lock");
        inner.tracks.insert(spec.remote_track_id.clone(), spec);
    }

    pub fn insert_playlist(&self, summary: StreamingPlaylistSummary, track_ids: Vec<String>) {
        let mut inner = self.inner.lock().expect("fake source lock");
        inner
            .playlist_tracks
            .insert(summary.remote_id.clone(), track_ids);
        inner.playlists.push(summary);
    }

    pub fn stored_credentials(&self) -> Option<StreamingCredentials> {
        self.inner
            .lock()
            .expect("fake source lock")
            .credentials
            .clone()
    }

    pub fn last_password(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("fake source lock")
            .last_password
            .clone()
    }

    pub fn mark_expired(&self) {
        self.inner.lock().expect("fake source lock").expired = true;
    }

    fn require_session(&self) -> Result<(), CatalogError> {
        let inner = self.inner.lock().expect("fake source lock");
        if inner.expired || inner.credentials.is_none() {
            return Err(CatalogError::SessionExpired {
                source_id: self.source_id.clone(),
            });
        }
        Ok(())
    }

    fn track_from_spec(&self, spec: &FakeTrackSpec) -> StreamingTrack {
        let refusal = match spec.resolve {
            FakeResolveSpec::Refusal(reason) => Some(ImportRefusal {
                reason,
                title: spec.title.clone(),
                artist: spec.artist.clone(),
            }),
            FakeResolveSpec::File { .. } => None,
        };
        StreamingTrack {
            source_id: self.source_id.clone(),
            remote_track_id: spec.remote_track_id.clone(),
            title: spec.title.clone(),
            artist: spec.artist.clone(),
            album: spec.album.clone(),
            duration_ms: spec.duration_ms,
            refusal,
        }
    }
}

impl StreamingSource for FakeStreamingSource {
    fn source_id(&self) -> &str {
        &self.source_id
    }

    fn session(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        let inner = self.inner.lock().expect("fake source lock");
        Ok(StreamingSessionSnapshot {
            source_id: self.source_id.clone(),
            signed_in: inner.credentials.is_some() && !inner.expired,
            display_name: inner.display_name.clone(),
            expired: inner.expired,
        })
    }

    fn start_qr(&self) -> Result<StreamingQrChallenge, CatalogError> {
        let mut inner = self.inner.lock().expect("fake source lock");
        inner.qr_key = Some("fake-qr-key".to_owned());
        inner.qr_polls = 0;
        Ok(StreamingQrChallenge {
            key: "fake-qr-key".to_owned(),
            login_url: "https://music.163.com/login?codekey=fake-qr-key".to_owned(),
            qr_svg: "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".to_owned(),
        })
    }

    fn poll_qr(&self, key: &str) -> Result<StreamingQrPoll, CatalogError> {
        let mut inner = self.inner.lock().expect("fake source lock");
        if inner.qr_key.as_deref() != Some(key) {
            return Ok(StreamingQrPoll {
                status: StreamingQrStatus::Expired,
                session: None,
            });
        }
        inner.qr_polls += 1;
        if inner.qr_polls < 2 {
            return Ok(StreamingQrPoll {
                status: StreamingQrStatus::Waiting,
                session: None,
            });
        }
        inner.credentials = Some(StreamingCredentials {
            music_u: "MUSIC_U_FAKE".to_owned(),
            csrf: "CSRF_FAKE".to_owned(),
        });
        inner.display_name = Some("QR User".to_owned());
        inner.expired = false;
        inner.last_password = None;
        Ok(StreamingQrPoll {
            status: StreamingQrStatus::Confirmed,
            session: Some(StreamingSessionSnapshot {
                source_id: self.source_id.clone(),
                signed_in: true,
                display_name: Some("QR User".to_owned()),
                expired: false,
            }),
        })
    }

    fn sign_in_password(
        &self,
        _method: StreamingPasswordMethod,
        identifier: &str,
        password: &str,
        _country_code: Option<&str>,
    ) -> Result<StreamingSessionSnapshot, CatalogError> {
        let mut inner = self.inner.lock().expect("fake source lock");
        inner.last_password = Some(password.to_owned());
        inner.credentials = Some(StreamingCredentials {
            music_u: format!("MUSIC_U_{identifier}"),
            csrf: "CSRF_FAKE".to_owned(),
        });
        if inner
            .credentials
            .as_ref()
            .is_some_and(|credentials| credentials.contains_password_material(password))
        {
            inner.credentials = Some(StreamingCredentials {
                music_u: "MUSIC_U_SANITIZED".to_owned(),
                csrf: "CSRF_FAKE".to_owned(),
            });
        }
        inner.display_name = Some(identifier.to_owned());
        inner.expired = false;
        inner.last_password = None;
        Ok(StreamingSessionSnapshot {
            source_id: self.source_id.clone(),
            signed_in: true,
            display_name: Some(identifier.to_owned()),
            expired: false,
        })
    }

    fn sign_out(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        let mut inner = self.inner.lock().expect("fake source lock");
        inner.credentials = None;
        inner.display_name = None;
        inner.expired = false;
        Ok(StreamingSessionSnapshot {
            source_id: self.source_id.clone(),
            signed_in: false,
            display_name: None,
            expired: false,
        })
    }

    fn liked_tracks(&self) -> Result<Vec<StreamingTrack>, CatalogError> {
        self.require_session()?;
        let inner = self.inner.lock().expect("fake source lock");
        Ok(inner
            .tracks
            .values()
            .map(|spec| self.track_from_spec(spec))
            .collect())
    }

    fn playlists(&self) -> Result<Vec<StreamingPlaylistSummary>, CatalogError> {
        self.require_session()?;
        Ok(self
            .inner
            .lock()
            .expect("fake source lock")
            .playlists
            .clone())
    }

    fn playlist(&self, remote_id: &str) -> Result<StreamingPlaylistDetail, CatalogError> {
        self.require_session()?;
        let inner = self.inner.lock().expect("fake source lock");
        let summary = inner
            .playlists
            .iter()
            .find(|playlist| playlist.remote_id == remote_id)
            .cloned()
            .ok_or_else(|| CatalogError::Internal(format!("unknown playlist {remote_id}")))?;
        let tracks = inner
            .playlist_tracks
            .get(remote_id)
            .into_iter()
            .flatten()
            .filter_map(|id| inner.tracks.get(id).map(|spec| self.track_from_spec(spec)))
            .collect();
        Ok(StreamingPlaylistDetail {
            remote_id: summary.remote_id,
            name: summary.name,
            tracks,
        })
    }

    fn search(&self, query: &str) -> Result<Vec<StreamingTrack>, CatalogError> {
        self.require_session()?;
        let query = query.to_lowercase();
        let inner = self.inner.lock().expect("fake source lock");
        Ok(inner
            .tracks
            .values()
            .filter(|spec| {
                spec.title.to_lowercase().contains(&query)
                    || spec.artist.to_lowercase().contains(&query)
            })
            .map(|spec| self.track_from_spec(spec))
            .collect())
    }

    fn resolve(&self, remote_track_id: &str) -> Result<StreamingResolveOutcome, CatalogError> {
        self.require_session()?;
        let inner = self.inner.lock().expect("fake source lock");
        let spec = inner
            .tracks
            .get(remote_track_id)
            .ok_or_else(|| CatalogError::Internal(format!("unknown track {remote_track_id}")))?;
        match &spec.resolve {
            FakeResolveSpec::File { path } => {
                let ext = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("bin");
                let dest = std::env::temp_dir().join(format!(
                    "openkara-fake-{}-{}.{}",
                    spec.remote_track_id,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_nanos())
                        .unwrap_or(0),
                    ext
                ));
                if let Some(parent) = dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::copy(path, &dest).map_err(|error| {
                    CatalogError::Internal(format!("failed to stage fake file: {error}"))
                })?;
                Ok(StreamingResolveOutcome::File(ResolvedStreamingFile {
                    path: dest,
                    title: Some(spec.title.clone()),
                    artist: Some(spec.artist.clone()),
                    album: spec.album.clone(),
                }))
            }
            FakeResolveSpec::Refusal(reason) => {
                Ok(StreamingResolveOutcome::Refusal(ImportRefusal {
                    reason: *reason,
                    title: spec.title.clone(),
                    artist: spec.artist.clone(),
                }))
            }
        }
    }
}

impl StreamingSource for &FakeStreamingSource {
    fn source_id(&self) -> &str {
        (*self).source_id()
    }
    fn session(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        (*self).session()
    }
    fn start_qr(&self) -> Result<StreamingQrChallenge, CatalogError> {
        (*self).start_qr()
    }
    fn poll_qr(&self, key: &str) -> Result<StreamingQrPoll, CatalogError> {
        (*self).poll_qr(key)
    }
    fn sign_in_password(
        &self,
        method: StreamingPasswordMethod,
        identifier: &str,
        password: &str,
        country_code: Option<&str>,
    ) -> Result<StreamingSessionSnapshot, CatalogError> {
        (*self).sign_in_password(method, identifier, password, country_code)
    }
    fn sign_out(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        (*self).sign_out()
    }
    fn liked_tracks(&self) -> Result<Vec<StreamingTrack>, CatalogError> {
        (*self).liked_tracks()
    }
    fn playlists(&self) -> Result<Vec<StreamingPlaylistSummary>, CatalogError> {
        (*self).playlists()
    }
    fn playlist(&self, remote_id: &str) -> Result<StreamingPlaylistDetail, CatalogError> {
        (*self).playlist(remote_id)
    }
    fn search(&self, query: &str) -> Result<Vec<StreamingTrack>, CatalogError> {
        (*self).search(query)
    }
    fn resolve(&self, remote_track_id: &str) -> Result<StreamingResolveOutcome, CatalogError> {
        (*self).resolve(remote_track_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_and_password_store_credentials_not_password() {
        let source = FakeStreamingSource::new("netease");
        let challenge = source.start_qr().expect("qr");
        let first = source.poll_qr(&challenge.key).expect("poll 1");
        assert_eq!(first.status, StreamingQrStatus::Waiting);
        let confirmed = source.poll_qr(&challenge.key).expect("poll 2");
        assert_eq!(confirmed.status, StreamingQrStatus::Confirmed);
        let credentials = source.stored_credentials().expect("stored");
        assert_eq!(credentials.music_u, "MUSIC_U_FAKE");
        assert_eq!(credentials.csrf, "CSRF_FAKE");
        assert!(source.last_password().is_none());

        source
            .sign_in_password(
                StreamingPasswordMethod::Phone,
                "13800000000",
                "super-secret",
                Some("86"),
            )
            .expect("password");
        let credentials = source.stored_credentials().expect("stored");
        assert!(!credentials.contains_password_material("super-secret"));
        assert!(source.last_password().is_none());
        assert_eq!(
            source.session().expect("session").display_name.as_deref(),
            Some("13800000000")
        );
    }

    #[test]
    fn sign_out_clears_credentials_disable_does_not() {
        let source = FakeStreamingSource::new("netease");
        source
            .sign_in_password(StreamingPasswordMethod::Email, "a@b.c", "pw", None)
            .expect("sign in");
        assert!(source.stored_credentials().is_some());
        let gated = GatedStreamingSource::new(false, &source);
        assert!(matches!(
            gated.liked_tracks(),
            Err(CatalogError::SourceDisabled { .. })
        ));
        assert!(source.stored_credentials().is_some());
        source.sign_out().expect("sign out");
        assert!(source.stored_credentials().is_none());
    }

    #[test]
    fn resolve_file_and_refusals() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("track.mp3");
        std::fs::write(&path, b"audio").expect("write");
        let source = FakeStreamingSource::new("netease");
        source
            .sign_in_password(StreamingPasswordMethod::Email, "a@b.c", "pw", None)
            .expect("sign in");
        source.insert_track(FakeTrackSpec {
            remote_track_id: "playable".to_owned(),
            title: "Playable".to_owned(),
            artist: "Artist".to_owned(),
            album: None,
            duration_ms: Some(1000),
            resolve: FakeResolveSpec::File { path: path.clone() },
        });
        source.insert_track(FakeTrackSpec {
            remote_track_id: "trial".to_owned(),
            title: "Trial".to_owned(),
            artist: "Artist".to_owned(),
            album: None,
            duration_ms: None,
            resolve: FakeResolveSpec::Refusal(ImportRefusalReason::TrialClip),
        });
        source.insert_track(FakeTrackSpec {
            remote_track_id: "empty".to_owned(),
            title: "Empty".to_owned(),
            artist: "Artist".to_owned(),
            album: None,
            duration_ms: None,
            resolve: FakeResolveSpec::Refusal(ImportRefusalReason::EmptyUrl),
        });

        match source.resolve("playable").expect("resolve") {
            StreamingResolveOutcome::File(file) => {
                assert_ne!(file.path, path);
                assert_eq!(
                    std::fs::read(&file.path).expect("staged"),
                    std::fs::read(&path).expect("source")
                );
            }
            StreamingResolveOutcome::Refusal(_) => panic!("expected file"),
        }
        match source.resolve("trial").expect("resolve") {
            StreamingResolveOutcome::Refusal(refusal) => {
                assert_eq!(refusal.reason, ImportRefusalReason::TrialClip);
                assert_eq!(refusal.title, "Trial");
            }
            StreamingResolveOutcome::File(_) => panic!("expected refusal"),
        }
        match source.resolve("empty").expect("resolve") {
            StreamingResolveOutcome::Refusal(refusal) => {
                assert_eq!(refusal.reason, ImportRefusalReason::EmptyUrl);
            }
            StreamingResolveOutcome::File(_) => panic!("expected refusal"),
        }
    }
}
