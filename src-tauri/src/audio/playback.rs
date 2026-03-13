use crate::audio::decode::DecodedAudio;
use anyhow::{bail, Result};
use serde::Serialize;
use std::sync::OnceLock;
use std::time::Instant;

pub const PLAYBACK_POSITION_EVENT: &str = "playback-position";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaybackStateSnapshot {
    pub song_id: Option<String>,
    pub is_playing: bool,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
}

#[derive(Debug)]
struct LoadedTrack {
    song_id: String,
    decoded_audio: DecodedAudio,
    base_position_ms: u64,
    started_at_ms: Option<u64>,
}

#[derive(Debug)]
pub struct PlaybackController {
    current_track: Option<LoadedTrack>,
    volume: f32,
}

impl Default for PlaybackController {
    fn default() -> Self {
        Self {
            current_track: None,
            volume: 1.0,
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
            decoded_audio,
            base_position_ms: 0,
            started_at_ms: Some(now_ms),
        });
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
            };
        }

        PlaybackStateSnapshot {
            song_id: None,
            is_playing: false,
            position_ms: 0,
            duration_ms: None,
            volume: self.volume,
        }
    }

    pub fn loaded_audio(&self) -> Option<&DecodedAudio> {
        self.current_track
            .as_ref()
            .map(|track| &track.decoded_audio)
    }
}

impl LoadedTrack {
    fn duration_ms(&self) -> u64 {
        self.decoded_audio.duration_ms
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
