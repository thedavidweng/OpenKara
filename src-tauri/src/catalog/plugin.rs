use super::credentials;
use super::streaming::{ResolveProgress, StreamingSource};
use super::types::*;
use crate::source_host::{Cancellation, HostError, ResourceDescriptor, SourceHost};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const METADATA_CAPACITY: usize = 20_000;
#[derive(Default)]
pub struct TrackMetadataCache {
    inner: Mutex<(
        HashMap<(String, String), StreamingTrack>,
        VecDeque<(String, String)>,
    )>,
}
impl TrackMetadataCache {
    pub fn get(&self, source_id: &str, remote_id: &str) -> Option<StreamingTrack> {
        self.inner
            .lock()
            .ok()?
            .0
            .get(&(source_id.to_owned(), remote_id.to_owned()))
            .cloned()
    }
    pub fn insert_all(&self, tracks: &[StreamingTrack]) {
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        for track in tracks {
            let key = (track.source_id.clone(), track.remote_track_id.clone());
            if !guard.0.contains_key(&key) {
                guard.1.push_back(key.clone());
            }
            guard.0.insert(key, track.clone());
        }
        while guard.0.len() > METADATA_CAPACITY {
            if let Some(key) = guard.1.pop_front() {
                guard.0.remove(&key);
            }
        }
    }
}

pub fn host_error(error: HostError) -> CatalogError {
    if error.code == "cancelled" {
        return CatalogError::Cancelled;
    }
    CatalogError::SourceFailure {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
}
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, CatalogError> {
    serde_json::from_value(value)
        .map_err(|_| CatalogError::Internal("source returned invalid domain data".into()))
}
#[derive(Deserialize)]
struct ProtocolSession {
    state: String,
    account: Option<Account>,
}
#[derive(Deserialize)]
struct Account {
    id: String,
    display_name: String,
}
#[derive(Deserialize)]
struct Track {
    id: String,
    title: String,
    artists: Vec<String>,
    album: Option<String>,
    duration_ms: Option<u64>,
    artwork: Option<ResourceDescriptor>,
    refusal: Option<Refusal>,
}
#[derive(Deserialize)]
struct Refusal {
    reason: String,
}
#[derive(Deserialize)]
struct Playlist {
    id: String,
    name: String,
    description: Option<String>,
    artwork: Option<ResourceDescriptor>,
    track_count: Option<u32>,
}
#[derive(Deserialize)]
struct Page<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
}
#[derive(Deserialize)]
struct PlaylistPage {
    playlist: Playlist,
    items: Vec<Track>,
    next_cursor: Option<String>,
}
#[derive(Deserialize)]
struct Details {
    items: Vec<Track>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Resolution {
    Media { media: Media },
    Refusal { refusal: Refusal },
}
#[derive(Deserialize)]
struct Media {
    url: String,
    headers: std::collections::BTreeMap<String, String>,
    expires_at: Option<String>,
    file_extension: String,
}

#[derive(Clone)]
pub struct PluginStreamingSource {
    host: SourceHost,
    source_id: String,
    data_root: PathBuf,
    cache: Arc<TrackMetadataCache>,
    cancellation: Cancellation,
}
impl PluginStreamingSource {
    pub fn open(
        host: SourceHost,
        source_id: String,
        data_root: PathBuf,
    ) -> Result<Self, CatalogError> {
        host.description(&source_id).map_err(host_error)?;
        Ok(Self {
            host,
            source_id,
            data_root,
            cache: Arc::new(TrackMetadataCache::default()),
            cancellation: Cancellation::new(),
        })
    }
    pub fn with_cache(mut self, cache: Arc<TrackMetadataCache>) -> Self {
        self.cache = cache;
        self
    }
    pub fn with_cancellation(mut self, cancellation: Cancellation) -> Self {
        self.cancellation = cancellation;
        self
    }
    pub fn request(&self, method: &str, params: Value) -> Result<Value, CatalogError> {
        let declaration = self.host.description(&self.source_id).map_err(host_error)?;
        if declaration
            .manifest
            .requires_authentication
            .iter()
            .any(|required| required == method)
        {
            let description = self.request_raw("describe", json!({}))?;
            match description
                .pointer("/session/state")
                .and_then(Value::as_str)
            {
                Some("signed_out") => {
                    if let Some(credentials) = credentials::load_credentials(
                        &self.data_root,
                        &self.source_id,
                        &declaration.owner_binding,
                    )
                    .map_err(|_| CatalogError::Internal("could not load source session".into()))?
                    {
                        self.request_raw("session.restore", json!({"credentials":credentials}))?;
                    }
                }
                Some("expired") => {
                    return Err(CatalogError::SessionExpired {
                        source_id: self.source_id.clone(),
                    })
                }
                Some("signed_in") => {}
                _ => {
                    return Err(CatalogError::Internal(
                        "source session state missing".into(),
                    ))
                }
            }
        }
        self.request_raw(method, params)
    }
    fn request_raw(&self, method: &str, params: Value) -> Result<Value, CatalogError> {
        tauri::async_runtime::block_on(async {
            let request = self
                .host
                .start_request(&self.source_id, method, params)
                .await
                .map_err(host_error)?;
            let cancel_handle = request.cancellation();
            let result = request.result();
            tokio::pin!(result);
            tokio::select! {
                result = &mut result => result.map_err(host_error),
                _ = self.cancellation.cancelled() => { cancel_handle.cancel(); let _ = result.await; Err(CatalogError::Cancelled) }
            }
        })
    }
    pub fn session_from_value(
        &self,
        value: Value,
    ) -> Result<StreamingSessionSnapshot, CatalogError> {
        let session: ProtocolSession = decode(value)?;
        if !matches!(
            session.state.as_str(),
            "signed_out" | "signed_in" | "expired"
        ) {
            return Err(CatalogError::Internal(
                "source returned invalid session state".into(),
            ));
        }
        if session.state == "signed_in"
            && session
                .account
                .as_ref()
                .is_none_or(|account| account.id.is_empty() || account.display_name.is_empty())
        {
            return Err(CatalogError::Internal(
                "source verified session account missing".into(),
            ));
        }
        Ok(StreamingSessionSnapshot {
            source_id: self.source_id.clone(),
            signed_in: session.state == "signed_in",
            display_name: session.account.map(|a| a.display_name),
            expired: session.state == "expired",
        })
    }
    pub fn save_verified_session(
        &self,
        value: &Value,
    ) -> Result<StreamingSessionSnapshot, CatalogError> {
        if self.cancellation.is_cancelled() {
            return Err(CatalogError::Cancelled);
        }
        let session = self.session_from_value(
            value
                .get("session")
                .cloned()
                .ok_or_else(|| CatalogError::Internal("verified session missing".into()))?,
        )?;
        if !session.signed_in {
            return Err(CatalogError::AuthFailed {
                source_id: self.source_id.clone(),
                detail: "source did not verify sign-in".into(),
            });
        }
        let credentials: StreamingCredentials = decode(
            value
                .get("credentials")
                .cloned()
                .ok_or_else(|| CatalogError::Internal("verified credentials missing".into()))?,
        )?;
        let owner = self
            .host
            .description(&self.source_id)
            .map_err(host_error)?
            .owner_binding;
        credentials::store_session(&self.data_root, &self.source_id, &owner, credentials)
            .map_err(|_| CatalogError::Internal("could not save source session".into()))?;
        Ok(session)
    }
    fn artwork(
        &self,
        descriptor: Option<ResourceDescriptor>,
    ) -> Result<Option<String>, CatalogError> {
        let Some(descriptor) = descriptor else {
            return Ok(None);
        };
        let bytes = match tauri::async_runtime::block_on(self.host.load_resource(
            &self.source_id,
            descriptor,
            self.cancellation.clone(),
        )) {
            Ok(bytes) => bytes,
            Err(error) if error.code == "cancelled" => return Err(CatalogError::Cancelled),
            Err(_) => return Ok(None),
        };
        let path = self
            .data_root
            .join("source-artwork")
            .join(&self.source_id)
            .join(format!(
                "{}.img",
                crate::hash::hex_lower(Sha256::digest(&bytes))
            ));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| CatalogError::Internal("could not cache source artwork".into()))?;
        }
        std::fs::write(&path, bytes)
            .map_err(|_| CatalogError::Internal("could not cache source artwork".into()))?;
        Ok(Some(path.to_string_lossy().into_owned()))
    }
    fn track(&self, track: Track) -> Result<StreamingTrack, CatalogError> {
        if track.id.is_empty() || track.title.is_empty() {
            return Err(CatalogError::Internal(
                "source returned empty track identity".into(),
            ));
        }
        let artist = track.artists.join(", ");
        let refusal = track
            .refusal
            .map(|r| self.refusal(r, &track.title, &artist))
            .transpose()?;
        Ok(StreamingTrack {
            source_id: self.source_id.clone(),
            remote_track_id: track.id,
            title: track.title,
            artist,
            album: track.album,
            duration_ms: track.duration_ms,
            refusal,
            library_song_id: None,
            cover_url: self.artwork(track.artwork)?,
        })
    }
    fn refusal(
        &self,
        refusal: Refusal,
        title: &str,
        artist: &str,
    ) -> Result<ImportRefusal, CatalogError> {
        let reason = match refusal.reason.as_str() {
            "no_play_rights" => ImportRefusalReason::NoPlayRights,
            "trial_clip" => ImportRefusalReason::TrialClip,
            "no_media" => ImportRefusalReason::EmptyUrl,
            "unavailable" => ImportRefusalReason::Unavailable,
            _ => return Err(CatalogError::Internal("unknown source refusal".into())),
        };
        Ok(ImportRefusal {
            reason,
            title: title.into(),
            artist: artist.into(),
        })
    }
    fn tracks_from(&self, tracks: Vec<Track>) -> Result<Vec<StreamingTrack>, CatalogError> {
        let tracks = tracks
            .into_iter()
            .map(|track| self.track(track))
            .collect::<Result<Vec<_>, _>>()?;
        self.cache.insert_all(&tracks);
        Ok(tracks)
    }
    pub fn track_page(
        &self,
        method: &str,
        params: Value,
    ) -> Result<StreamingTrackListing, CatalogError> {
        let page: Page<Track> = decode(self.request(method, params)?)?;
        Ok(StreamingTrackListing {
            tracks: self.tracks_from(page.items)?,
            next_cursor: page.next_cursor,
        })
    }
    pub fn playlist_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<StreamingPlaylistListing, CatalogError> {
        let page: Page<Playlist> = decode(self.request("playlists", json!({"cursor":cursor}))?)?;
        let items = page
            .items
            .into_iter()
            .map(|p| {
                Ok(StreamingPlaylistSummary {
                    source_id: self.source_id.clone(),
                    remote_id: p.id,
                    name: p.name,
                    track_count: p.track_count,
                    cover_url: self.artwork(p.artwork)?,
                })
            })
            .collect::<Result<Vec<_>, CatalogError>>()?;
        Ok(StreamingPlaylistListing {
            items,
            next_cursor: page.next_cursor,
        })
    }
    pub fn playlist_tracks_page(
        &self,
        id: &str,
        cursor: Option<&str>,
    ) -> Result<StreamingPlaylistDetail, CatalogError> {
        let page: PlaylistPage =
            decode(self.request("playlist.tracks", json!({"playlist_id":id,"cursor":cursor}))?)?;
        if page.playlist.id != id {
            return Err(CatalogError::Internal(
                "source returned another playlist".into(),
            ));
        }
        Ok(StreamingPlaylistDetail {
            source_id: self.source_id.clone(),
            remote_id: page.playlist.id,
            name: page.playlist.name,
            tracks: self.tracks_from(page.items)?,
            cover_url: self.artwork(page.playlist.artwork)?,
            creator: None,
            description: page.playlist.description,
            track_count: page.playlist.track_count,
            updated_at_ms: None,
            next_cursor: page.next_cursor,
        })
    }
    pub fn import_tracks(&self, ids: &[String]) -> Result<Vec<StreamingTrack>, CatalogError> {
        let missing = ids
            .iter()
            .filter(|id| self.cache.get(&self.source_id, id).is_none())
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            let declaration = self.host.description(&self.source_id).map_err(host_error)?;
            if !declaration
                .manifest
                .capabilities
                .iter()
                .any(|capability| capability == "track_details")
            {
                return Err(CatalogError::Internal(
                    "reload the source listing before importing this track".into(),
                ));
            }
            self.tracks(&missing)?;
        }
        ids.iter()
            .map(|id| {
                self.cache
                    .get(&self.source_id, id)
                    .ok_or_else(|| CatalogError::SourceFailure {
                        code: "not_found".into(),
                        message:
                            "A requested track is no longer available. Reload the source listing."
                                .into(),
                        retryable: false,
                    })
            })
            .collect()
    }
    fn resolution(&self, id: &str, purpose: &str) -> Result<Resolution, CatalogError> {
        decode(self.request("media.resolve", json!({"track_id":id,"purpose":purpose}))?)
    }
    fn media_descriptor(media: Media) -> Result<(ResourceDescriptor, String), CatalogError> {
        if media.file_extension.is_empty()
            || !media
                .file_extension
                .bytes()
                .all(|b| b.is_ascii_alphanumeric())
        {
            return Err(CatalogError::Internal(
                "invalid source media extension".into(),
            ));
        }
        Ok((
            ResourceDescriptor {
                url: media.url,
                headers: media.headers,
                expires_at: media.expires_at,
            },
            media.file_extension,
        ))
    }
}
impl StreamingSource for PluginStreamingSource {
    fn source_id(&self) -> &str {
        &self.source_id
    }
    fn session(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        let owner = self
            .host
            .description(&self.source_id)
            .map_err(host_error)?
            .owner_binding;
        let current = self
            .request("describe", json!({}))?
            .get("session")
            .cloned()
            .ok_or_else(|| CatalogError::Internal("source session missing".into()))?;
        let snapshot = self.session_from_value(current)?;
        if snapshot.signed_in || snapshot.expired {
            return Ok(snapshot);
        }
        match credentials::load_credentials(&self.data_root, &self.source_id, &owner)
            .map_err(|_| CatalogError::Internal("could not load source session".into()))?
        {
            Some(credentials) => {
                let restored =
                    self.request("session.restore", json!({"credentials":credentials}))?;
                self.session_from_value(restored)
            }
            None => Ok(snapshot),
        }
    }
    fn sign_out(&self) -> Result<StreamingSessionSnapshot, CatalogError> {
        credentials::clear_session(&self.data_root, &self.source_id)
            .map_err(|_| CatalogError::Internal("could not remove source session".into()))?;
        let _ = self.request("auth.sign_out", json!({}));
        Ok(StreamingSessionSnapshot {
            source_id: self.source_id.clone(),
            signed_in: false,
            display_name: None,
            expired: false,
        })
    }
    fn liked_tracks(&self) -> Result<Vec<StreamingTrack>, CatalogError> {
        let mut tracks = Vec::new();
        let mut cursor = None;
        loop {
            let page = self.track_page("liked_tracks", json!({"cursor":cursor}))?;
            tracks.extend(page.tracks);
            cursor = page.next_cursor;
            if cursor.is_none() {
                return Ok(tracks);
            }
        }
    }
    fn playlists(&self) -> Result<Vec<StreamingPlaylistSummary>, CatalogError> {
        let mut items = Vec::new();
        let mut cursor = None;
        loop {
            let page = self.playlist_page(cursor.as_deref())?;
            items.extend(page.items);
            cursor = page.next_cursor;
            if cursor.is_none() {
                return Ok(items);
            }
        }
    }
    fn playlist(&self, id: &str) -> Result<StreamingPlaylistDetail, CatalogError> {
        let mut detail = self.playlist_tracks_page(id, None)?;
        while let Some(cursor) = detail.next_cursor.take() {
            let page = self.playlist_tracks_page(id, Some(&cursor))?;
            detail.tracks.extend(page.tracks);
            detail.next_cursor = page.next_cursor;
        }
        Ok(detail)
    }
    fn tracks(&self, ids: &[String]) -> Result<Vec<StreamingTrack>, CatalogError> {
        let mut tracks = Vec::with_capacity(ids.len());
        for batch in ids.chunks(100) {
            let page: Details = decode(self.request("tracks.get", json!({"track_ids":batch}))?)?;
            let fetched = self.tracks_from(page.items)?;
            if batch
                .iter()
                .any(|id| !fetched.iter().any(|track| &track.remote_track_id == id))
            {
                return Err(CatalogError::SourceFailure {
                    code: "not_found".into(),
                    message: "A requested track is no longer available. Reload the source listing."
                        .into(),
                    retryable: false,
                });
            }
            tracks.extend(fetched);
        }
        Ok(tracks)
    }
    fn search(&self, query: &str) -> Result<Vec<StreamingTrack>, CatalogError> {
        Ok(self
            .track_page("search", json!({"query":query,"cursor":null}))?
            .tracks)
    }
    fn resolve(
        &self,
        id: &str,
        progress: &dyn ResolveProgress,
    ) -> Result<StreamingResolveOutcome, CatalogError> {
        if progress.is_cancelled() {
            return Err(CatalogError::Cancelled);
        }
        let track = self.cache.get(&self.source_id, id);
        match self.resolution(id, "import")? {
            Resolution::Refusal { refusal } => Ok(StreamingResolveOutcome::Refusal(self.refusal(
                refusal,
                track.as_ref().map(|t| t.title.as_str()).unwrap_or(""),
                track.as_ref().map(|t| t.artist.as_str()).unwrap_or(""),
            )?)),
            Resolution::Media { media } => {
                let (descriptor, extension) = Self::media_descriptor(media)?;
                let directory = self
                    .data_root
                    .join("source-temporary")
                    .join(&self.source_id);
                std::fs::create_dir_all(&directory)
                    .map_err(|_| CatalogError::Internal("could not stage source media".into()))?;
                let path = directory.join(format!(
                    "{}.{extension}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                ));
                progress.downloading(None);
                tauri::async_runtime::block_on(async {
                    let cancellation = self.cancellation.clone();
                    let download = self.host.download_media(
                        &self.source_id,
                        descriptor,
                        path.clone(),
                        cancellation.clone(),
                    );
                    tokio::pin!(download);
                    loop {
                        tokio::select! {
                            result = &mut download => break result.map_err(host_error),
                            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                                if progress.is_cancelled() {
                                    cancellation.cancel();
                                }
                            }
                        }
                    }
                })?;
                progress.downloading(Some(100));
                Ok(StreamingResolveOutcome::File(ResolvedStreamingFile {
                    path,
                    title: track.as_ref().map(|t| t.title.clone()),
                    artist: track.as_ref().map(|t| t.artist.clone()),
                    album: track.and_then(|t| t.album),
                }))
            }
        }
    }
    fn preview(&self, id: &str) -> Result<StreamingPreviewOutcome, CatalogError> {
        let track = self.cache.get(&self.source_id, id);
        match self.resolution(id, "preview")? {
            Resolution::Media { media } => {
                let (descriptor, extension) = Self::media_descriptor(media)?;
                Ok(StreamingPreviewOutcome::Stream(ResolvedStreamingUrl {
                    descriptor,
                    extension,
                }))
            }
            Resolution::Refusal { refusal } => Ok(StreamingPreviewOutcome::Refusal(self.refusal(
                refusal,
                track.as_ref().map(|t| t.title.as_str()).unwrap_or(""),
                track.as_ref().map(|t| t.artist.as_str()).unwrap_or(""),
            )?)),
        }
    }
}
