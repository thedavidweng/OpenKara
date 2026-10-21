use super::types::{
    CatalogError, ImportRefusal, ImportRefusalReason, ResolvedStreamingFile, StreamingCredentials,
    StreamingPlaylistDetail, StreamingPlaylistSummary, StreamingPreviewOutcome,
    StreamingResolveOutcome, StreamingSessionSnapshot, StreamingTrack,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// Receives download progress while a source resolves a track, and lets the
/// caller stop a download in flight.
pub trait ResolveProgress: Send + Sync {
    /// `percent` is 0-100 from the response length, or `None` when unknown.
    fn downloading(&self, percent: Option<u8>);
    fn is_cancelled(&self) -> bool;
}

pub struct NoResolveProgress;

impl ResolveProgress for NoResolveProgress {
    fn downloading(&self, _percent: Option<u8>) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

pub trait StreamingSource: Send + Sync {
    fn source_id(&self) -> &str;
    fn session(&self) -> Result<StreamingSessionSnapshot, CatalogError>;
    fn sign_out(&self) -> Result<StreamingSessionSnapshot, CatalogError>;
    fn liked_tracks(&self) -> Result<Vec<StreamingTrack>, CatalogError>;
    fn playlists(&self) -> Result<Vec<StreamingPlaylistSummary>, CatalogError>;
    /// The complete playlist (every track), used by import.
    fn playlist(&self, remote_id: &str) -> Result<StreamingPlaylistDetail, CatalogError>;
    /// Tracks for `ids`, in order; unknown ids are dropped.
    fn tracks(&self, ids: &[String]) -> Result<Vec<StreamingTrack>, CatalogError>;
    fn search(&self, query: &str) -> Result<Vec<StreamingTrack>, CatalogError>;
    fn resolve(
        &self,
        remote_track_id: &str,
        progress: &dyn ResolveProgress,
    ) -> Result<StreamingResolveOutcome, CatalogError>;
    /// A URL the track can be streamed from without importing it, or the
    /// reason it cannot be played in full.
    fn preview(&self, _remote_track_id: &str) -> Result<StreamingPreviewOutcome, CatalogError> {
        Err(CatalogError::Internal(
            "this source cannot stream previews".to_owned(),
        ))
    }
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

    fn tracks(&self, ids: &[String]) -> Result<Vec<StreamingTrack>, CatalogError> {
        self.require_enabled()?;
        self.inner.tracks(ids)
    }

    fn search(&self, query: &str) -> Result<Vec<StreamingTrack>, CatalogError> {
        self.require_enabled()?;
        self.inner.search(query)
    }

    fn resolve(
        &self,
        remote_track_id: &str,
        progress: &dyn ResolveProgress,
    ) -> Result<StreamingResolveOutcome, CatalogError> {
        self.require_enabled()?;
        self.inner.resolve(remote_track_id, progress)
    }

    fn preview(&self, remote_track_id: &str) -> Result<StreamingPreviewOutcome, CatalogError> {
        self.require_enabled()?;
        self.inner.preview(remote_track_id)
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

    pub fn authenticate(&self) {
        let mut inner = self.inner.lock().expect("fake source lock");
        inner.credentials = Some(StreamingCredentials::from([(
            "fixture_session".into(),
            "fixture-session".into(),
        )]));
        inner.display_name = Some("Ada".to_owned());
        inner.expired = false;
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
            library_song_id: None,
            cover_url: None,
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
        let tracks: Vec<StreamingTrack> = inner
            .playlist_tracks
            .get(remote_id)
            .into_iter()
            .flatten()
            .filter_map(|id| inner.tracks.get(id).map(|spec| self.track_from_spec(spec)))
            .collect();
        Ok(StreamingPlaylistDetail {
            source_id: self.source_id.clone(),
            remote_id: summary.remote_id,
            name: summary.name,
            track_count: Some(tracks.len() as u32),
            tracks,
            cover_url: summary.cover_url,
            creator: None,
            description: None,
            updated_at_ms: None,
            next_cursor: None,
        })
    }

    fn tracks(&self, ids: &[String]) -> Result<Vec<StreamingTrack>, CatalogError> {
        self.require_session()?;
        let inner = self.inner.lock().expect("fake source lock");
        Ok(ids
            .iter()
            .filter_map(|id| inner.tracks.get(id).map(|spec| self.track_from_spec(spec)))
            .collect())
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

    fn resolve(
        &self,
        remote_track_id: &str,
        progress: &dyn ResolveProgress,
    ) -> Result<StreamingResolveOutcome, CatalogError> {
        self.require_session()?;
        if progress.is_cancelled() {
            return Err(CatalogError::Cancelled);
        }
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
                progress.downloading(Some(0));
                std::fs::copy(path, &dest).map_err(|error| {
                    CatalogError::Internal(format!("failed to stage fake file: {error}"))
                })?;
                progress.downloading(Some(100));
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
    fn tracks(&self, ids: &[String]) -> Result<Vec<StreamingTrack>, CatalogError> {
        (*self).tracks(ids)
    }
    fn search(&self, query: &str) -> Result<Vec<StreamingTrack>, CatalogError> {
        (*self).search(query)
    }
    fn resolve(
        &self,
        remote_track_id: &str,
        progress: &dyn ResolveProgress,
    ) -> Result<StreamingResolveOutcome, CatalogError> {
        (*self).resolve(remote_track_id, progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_out_clears_credentials_disable_does_not() {
        let source = FakeStreamingSource::new("netease");
        source.authenticate();
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
        source.authenticate();
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

        match source
            .resolve("playable", &NoResolveProgress)
            .expect("resolve")
        {
            StreamingResolveOutcome::File(file) => {
                assert_ne!(file.path, path);
                assert_eq!(
                    std::fs::read(&file.path).expect("staged"),
                    std::fs::read(&path).expect("source")
                );
            }
            StreamingResolveOutcome::Refusal(_) => panic!("expected file"),
        }
        match source
            .resolve("trial", &NoResolveProgress)
            .expect("resolve")
        {
            StreamingResolveOutcome::Refusal(refusal) => {
                assert_eq!(refusal.reason, ImportRefusalReason::TrialClip);
                assert_eq!(refusal.title, "Trial");
            }
            StreamingResolveOutcome::File(_) => panic!("expected refusal"),
        }
        match source
            .resolve("empty", &NoResolveProgress)
            .expect("resolve")
        {
            StreamingResolveOutcome::Refusal(refusal) => {
                assert_eq!(refusal.reason, ImportRefusalReason::EmptyUrl);
            }
            StreamingResolveOutcome::File(_) => panic!("expected refusal"),
        }
    }
}
