use crate::audio::decode::DecodedAudio;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Instant;

pub const PLAYBACK_POSITION_EVENT: &str = "playback-position";
pub const PLAYBACK_POSITION_POLL_INTERVAL_MS: u64 = 16;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaybackStateSnapshot {
    pub song_id: Option<String>,
    pub is_playing: bool,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub mode: PlaybackMode,
}

#[derive(Debug)]
struct LoadedTrack {
    song_id: String,
    original_audio: DecodedAudio,
    karaoke_audio: Option<DecodedAudio>,
    base_position_ms: u64,
    started_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackMode {
    Original,
    Karaoke,
}

#[derive(Debug)]
pub struct PlaybackController {
    current_track: Option<LoadedTrack>,
    volume: f32,
    mode: PlaybackMode,
}

impl Default for PlaybackController {
    fn default() -> Self {
        Self {
            current_track: None,
            volume: 1.0,
            mode: PlaybackMode::Original,
        }
    }
}

impl PlaybackController {
    pub fn start_track(
        &mut self,
        song_id: String,
        decoded_audio: DecodedAudio,
        now_ms: u64,
    ) -> PlaybackStateSnapshot {
        self.current_track = Some(LoadedTrack {
            song_id,
            original_audio: decoded_audio,
            karaoke_audio: None,
            base_position_ms: 0,
            started_at_ms: Some(now_ms),
        });
        self.mode = PlaybackMode::Original;
        self.snapshot(now_ms)
    }

    pub fn play(&mut self, now_ms: u64) -> Result<PlaybackStateSnapshot> {
        let track = self
            .current_track
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no track is loaded"))?;
        let position_ms = track.position_ms(now_ms);
        track.base_position_ms = position_ms;
        track.started_at_ms = Some(now_ms);

        Ok(self.snapshot(now_ms))
    }

    pub fn pause(&mut self, now_ms: u64) -> Result<PlaybackStateSnapshot> {
        let track = self
            .current_track
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no track is loaded"))?;
        let position_ms = track.position_ms(now_ms);
        track.base_position_ms = position_ms;
        track.started_at_ms = None;

        Ok(self.snapshot(now_ms))
    }

    pub fn seek(&mut self, target_ms: u64, now_ms: u64) -> Result<PlaybackStateSnapshot> {
        let track = self
            .current_track
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no track is loaded"))?;
        track.base_position_ms = target_ms.min(track.duration_ms());
        if track.started_at_ms.is_some() {
            track.started_at_ms = Some(now_ms);
        }

        Ok(self.snapshot(now_ms))
    }

    pub fn set_volume(&mut self, level: f32) -> Result<PlaybackStateSnapshot> {
        self.volume = level.clamp(0.0, 1.0);
        Ok(self.snapshot(monotonic_now_ms()))
    }

    pub fn set_mode(&mut self, mode: PlaybackMode) -> Result<PlaybackStateSnapshot> {
        if self.current_track.is_none() {
            return Err(anyhow::anyhow!("no track is loaded"));
        }

        if mode == PlaybackMode::Karaoke && !self.has_karaoke_track() {
            bail!("karaoke audio is not loaded for the current track");
        }

        self.mode = mode;
        Ok(self.snapshot(monotonic_now_ms()))
    }

    pub fn attach_karaoke_track(
        &mut self,
        song_id: &str,
        decoded_audio: DecodedAudio,
    ) -> Result<()> {
        let track = self
            .current_track
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no track is loaded"))?;

        if track.song_id != song_id {
            bail!(
                "cannot attach karaoke audio for song {} while {} is loaded",
                song_id,
                track.song_id
            );
        }

        track.karaoke_audio = Some(decoded_audio);
        Ok(())
    }

    pub fn snapshot(&mut self, now_ms: u64) -> PlaybackStateSnapshot {
        if let Some(track) = self.current_track.as_mut() {
            let position_ms = track.position_ms(now_ms);
            let duration_ms = track.duration_ms();

            if position_ms >= duration_ms {
                track.base_position_ms = duration_ms;
                track.started_at_ms = None;
            }

            return PlaybackStateSnapshot {
                song_id: Some(track.song_id.clone()),
                is_playing: track.started_at_ms.is_some(),
                position_ms: track.position_ms(now_ms),
                duration_ms: Some(duration_ms),
                volume: self.volume,
                mode: self.mode,
            };
        }

        PlaybackStateSnapshot {
            song_id: None,
            is_playing: false,
            position_ms: 0,
            duration_ms: None,
            volume: self.volume,
            mode: self.mode,
        }
    }

    pub fn loaded_audio(&self) -> Option<&DecodedAudio> {
        self.current_track.as_ref().map(|track| match self.mode {
            PlaybackMode::Original => &track.original_audio,
            PlaybackMode::Karaoke => track
                .karaoke_audio
                .as_ref()
                .unwrap_or(&track.original_audio),
        })
    }

    pub fn current_song_id(&self) -> Option<&str> {
        self.current_track
            .as_ref()
            .map(|track| track.song_id.as_str())
    }

    pub fn has_karaoke_track(&self) -> bool {
        self.current_track
            .as_ref()
            .and_then(|track| track.karaoke_audio.as_ref())
            .is_some()
    }
}

impl LoadedTrack {
    fn duration_ms(&self) -> u64 {
        self.original_audio.duration_ms
    }

    fn position_ms(&self, now_ms: u64) -> u64 {
        let elapsed_ms = self
            .started_at_ms
            .map(|started_at_ms| now_ms.saturating_sub(started_at_ms))
            .unwrap_or(0);

        (self.base_position_ms + elapsed_ms).min(self.duration_ms())
    }
}

pub fn monotonic_now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PlaybackPositionEvent {
    pub ms: u64,
}

pub fn playback_position_event(snapshot: &PlaybackStateSnapshot) -> Result<PlaybackPositionEvent> {
    if snapshot.song_id.is_none() {
        bail!("cannot emit playback position without a loaded track");
    }

    Ok(PlaybackPositionEvent {
        ms: snapshot.position_ms,
    })
}
