use crate::catalog::types::StreamingTrack;
use std::collections::HashMap;
use std::sync::Mutex;

/// Upper bound on remembered tracks; the cache is dropped whole when a batch
/// would exceed it (a session rarely touches this many distinct songs).
const SONG_CACHE_CAPACITY: usize = 20_000;

/// Session-lifetime, in-memory `song_detail` results keyed by track id, so
/// reopening a playlist or scrolling never refetches. Rights depend on the
/// signed-in account, so it is cleared on sign-in and sign-out.
#[derive(Default)]
pub struct NeteaseSongCache {
    tracks: Mutex<HashMap<String, StreamingTrack>>,
}

impl NeteaseSongCache {
    pub fn get(&self, remote_track_id: &str) -> Option<StreamingTrack> {
        self.tracks.lock().ok()?.get(remote_track_id).cloned()
    }

    pub fn insert_all(&self, tracks: &[StreamingTrack]) {
        let Ok(mut cache) = self.tracks.lock() else {
            return;
        };
        if cache.len() + tracks.len() > SONG_CACHE_CAPACITY {
            cache.clear();
        }
        for track in tracks {
            cache.insert(track.remote_track_id.clone(), track.clone());
        }
    }

    pub fn clear(&self) {
        if let Ok(mut cache) = self.tracks.lock() {
            cache.clear();
        }
    }

    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.tracks.lock().map(|cache| cache.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: usize) -> StreamingTrack {
        StreamingTrack {
            source_id: "netease".to_owned(),
            remote_track_id: id.to_string(),
            title: "T".to_owned(),
            artist: "A".to_owned(),
            album: None,
            duration_ms: None,
            refusal: None,
            library_song_id: None,
            cover_url: None,
        }
    }

    #[test]
    fn remembers_tracks_and_clears() {
        let cache = NeteaseSongCache::default();
        cache.insert_all(&[track(1), track(2)]);
        assert_eq!(cache.get("1").unwrap().remote_track_id, "1");
        assert!(cache.get("3").is_none());
        cache.clear();
        assert!(cache.get("1").is_none());
    }

    #[test]
    fn stays_bounded() {
        let cache = NeteaseSongCache::default();
        let batch: Vec<_> = (0..SONG_CACHE_CAPACITY).map(track).collect();
        cache.insert_all(&batch);
        assert_eq!(cache.count(), SONG_CACHE_CAPACITY);
        cache.insert_all(&[track(SONG_CACHE_CAPACITY + 1)]);
        assert_eq!(cache.count(), 1);
    }
}
