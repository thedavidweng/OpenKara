use crate::audio::decode::DecodedAudio;
use crate::audio::error::PlaybackError;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Duration of the fade-in/fade-out envelope applied to play/pause transitions.
const FADE_DURATION: Duration = Duration::from_millis(50);

/// Duration of the fade-in applied after a seek to prevent audible clicks.
/// 8ms is short enough to be perceptually transparent while masking any
/// amplitude discontinuity at the new playback position.
const SEEK_FADE_DURATION: Duration = Duration::from_millis(8);

pub const PLAYBACK_POSITION_EVENT: &str = "playback-position";
pub const PLAYBACK_ENDED_EVENT: &str = "playback-ended";
pub const PLAYBACK_ERROR_EVENT: &str = "playback-error";
pub const PLAYBACK_POSITION_POLL_INTERVAL_MS: u64 = 33;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StemVolumes {
    pub vocals: f32,
    pub drums: f32,
    pub bass: f32,
    pub other: f32,
}

impl Default for StemVolumes {
    fn default() -> Self {
        Self {
            vocals: 1.0,
            drums: 1.0,
            bass: 1.0,
            other: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StemName {
    Vocals,
    Drums,
    Bass,
    Other,
}

#[derive(Debug)]
pub struct StemSet {
    pub vocals: DecodedAudio,
    pub drums: DecodedAudio,
    pub bass: DecodedAudio,
    pub other: DecodedAudio,
}

#[derive(Debug)]
pub enum LoadedStems {
    /// Vocals + mixed accompaniment (2-stem mode)
    TwoStem {
        vocals: DecodedAudio,
        accompaniment: DecodedAudio,
    },
    /// Individual stems (4-stem mode)
    FourStem(StemSet),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaybackStateSnapshot {
    pub song_id: Option<String>,
    /// Monotonic transport generation. Incremented when a new song load starts
    /// so webviews can discard delayed events from the previous transport.
    pub transport_generation: u64,
    /// Backend transport lifecycle; pause is represented by `is_playing: false`.
    /// `playing` means a decoded track owns the transport, not that time is advancing.
    pub state: String,
    pub is_playing: bool,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    /// Maximum safe playback position (ms) that has been buffered.
    /// In whole-track mode equals `duration_ms`; in streaming mode (P1+) driven
    /// by ring-buffer water level. Used by the UI for the grey buffer bar.
    pub buffered_ms: u64,
    pub volume: f32,
    pub stem_volumes: StemVolumes,
    pub has_stems: bool,
    pub stem_mode: Option<String>,
}

impl PlaybackStateSnapshot {
    pub fn idle() -> Self {
        Self {
            song_id: None,
            transport_generation: 0,
            state: "idle".to_owned(),
            is_playing: false,
            position_ms: 0,
            duration_ms: None,
            buffered_ms: 0,
            volume: 1.0,
            stem_volumes: StemVolumes::default(),
            has_stems: false,
            stem_mode: None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct LoadedTrack {
    pub(crate) song_id: String,
    pub(crate) original_audio: DecodedAudio,
    pub(crate) stems: Option<LoadedStems>,
    /// Whether the audio output is actively producing sound.
    /// `false` when paused or after end-of-track.
    is_playing: bool,
    /// Source-rate frame index where the audio output thread renders from next.
    /// The sole authority for `position_ms`. Updated exclusively by the render
    /// callback; reset by seek / start_track.
    pub(crate) render_frame: u64,
    /// Streaming ring-buffer consumers. When `Some`, the render callback reads
    /// from these instead of `original_audio.samples`.
    pub(crate) streaming: Option<super::streaming::StreamingTrack>,
}

/// A fully decoded, normalized next track ready for gapless transition.
/// The audio callback is the only code allowed to consume this.
#[derive(Debug)]
pub struct PreparedTrack {
    /// Monotonic generation of the `set_preload_candidate` call that
    /// initiated this preload. The coordinator increments its expected
    /// generation on every `CancelPreparedNext` and rejects `PrepareNext`
    /// commands whose generation is stale — this closes the race where an
    /// old preload thread passes its shutdown check before the flag is set
    /// but sends `PrepareNext` after the cancel has been processed.
    pub preload_request_generation: u64,
    /// Output-format generation captured at prepare time. Used for the
    /// `CompletedTransition` event and for stale-format rejection.
    pub preload_generation: u64,
    pub song_id: String,
    pub output_format: OutputFormatSnapshot,
    pub audio: DecodedAudio,
}

/// Completed gapless transition metadata, drained by the position emitter
/// to emit `track-transitioned` before the next position event.
#[derive(Debug, Clone)]
pub struct CompletedTransition {
    pub transition_serial: u64,
    pub preload_generation: u64,
    pub from_song_id: String,
    pub to_song_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FadeState {
    /// No fade active.
    None,
    /// Fading in (volume ramping up) after a play command.
    FadingIn { start: Instant },
    /// Fading out (volume ramping down) after a pause command.
    FadingOut { start: Instant },
    /// Short fade-in after a seek to mask amplitude discontinuity.
    FadingAfterSeek { start: Instant },
}

#[derive(Debug)]
pub struct PlaybackController {
    pub(crate) current_track: Option<LoadedTrack>,
    loading_song_id: Option<String>,
    transport_generation: u64,
    volume: f32,
    stem_volumes: StemVolumes,
    /// Transport-level buffering flag. When `true` and a track is loaded,
    /// `snapshot()` reports `state: "buffering"`. Set by the streaming layer
    /// (P1/P2) on underrun; cleared when the buffer refills.
    pub(crate) is_buffering: bool,
    /// Active fade envelope for play/pause transitions.
    pub(crate) fade: FadeState,
    /// EQ config snapshot published by the controller and polled by the
    /// realtime output callback via `eq_config()`. The revision is bumped on
    /// every successful setter so the callback can detect changes without
    /// comparing the full struct each tick.
    pub(crate) eq_config: crate::audio::eq::EqConfig,
}

impl Default for PlaybackController {
    fn default() -> Self {
        Self {
            current_track: None,
            loading_song_id: None,
            transport_generation: 0,
            volume: 1.0,
            stem_volumes: StemVolumes::default(),
            is_buffering: false,
            fade: FadeState::None,
            eq_config: crate::audio::eq::EqConfig::flat(),
        }
    }
}

impl PlaybackController {
    fn bump_transport_generation(&mut self) {
        self.transport_generation = self.transport_generation.saturating_add(1);
    }

    pub fn start_track(
        &mut self,
        song_id: String,
        decoded_audio: DecodedAudio,
        _now_ms: u64,
    ) -> PlaybackStateSnapshot {
        self.loading_song_id = None;
        self.current_track = Some(LoadedTrack {
            song_id,
            original_audio: decoded_audio,
            stems: None,
            is_playing: true,
            render_frame: 0,
            streaming: None,
        });
        self.snapshot()
    }

    /// Start a track in streaming mode. The audio samples live in ring buffers
    /// (held in `streaming`) rather than in `original_audio.samples`.
    /// `metadata` provides sample_rate/channels/duration_ms for position calculation.
    pub fn start_track_streaming(
        &mut self,
        song_id: String,
        sample_rate: u32,
        channels: usize,
        duration_ms: u64,
        streaming: super::streaming::StreamingTrack,
        _now_ms: u64,
    ) -> PlaybackStateSnapshot {
        self.loading_song_id = None;
        self.current_track = Some(LoadedTrack {
            song_id,
            original_audio: DecodedAudio {
                sample_rate,
                channels,
                duration_ms,
                samples: Vec::new(), // samples live in the ring buffer
            },
            stems: None,
            is_playing: true,
            render_frame: 0,
            streaming: Some(streaming),
        });
        self.snapshot()
    }

    /// Mark a track as loading — the audio data will arrive later from a
    /// background download/decode task.  The snapshot reports `state: "loading"`
    /// so the UI can show a spinner without freezing the window.
    pub fn start_track_loading(&mut self, song_id: &str) -> PlaybackStateSnapshot {
        self.bump_transport_generation();
        self.current_track = None;
        self.loading_song_id = Some(song_id.to_owned());
        self.snapshot()
    }

    pub fn play(&mut self, _now_ms: u64) -> Result<PlaybackStateSnapshot, PlaybackError> {
        if self.current_track.is_none() {
            return Err(PlaybackError::InvalidPlaybackState(
                "no track is loaded".to_owned(),
            ));
        }
        self.bump_transport_generation();
        let track = self
            .current_track
            .as_mut()
            .expect("track existence checked before generation bump");
        track.is_playing = true;
        self.fade = FadeState::FadingIn {
            start: Instant::now(),
        };

        Ok(self.snapshot())
    }

    pub fn pause(&mut self, _now_ms: u64) -> Result<PlaybackStateSnapshot, PlaybackError> {
        if self.current_track.is_none() {
            return Err(PlaybackError::InvalidPlaybackState(
                "no track is loaded".to_owned(),
            ));
        }
        self.bump_transport_generation();
        // Start a fade-out. The render callback will set is_playing = false
        // once the fade envelope completes.
        self.fade = FadeState::FadingOut {
            start: Instant::now(),
        };

        Ok(self.snapshot())
    }

    pub fn seek(
        &mut self,
        target_ms: u64,
        _now_ms: u64,
    ) -> Result<PlaybackStateSnapshot, PlaybackError> {
        // Cancel any active fade and start a short seek fade to mask
        // the amplitude discontinuity at the new position.
        self.bump_transport_generation();
        self.fade = FadeState::FadingAfterSeek {
            start: Instant::now(),
        };
        let track = self
            .current_track
            .as_mut()
            .ok_or_else(|| PlaybackError::InvalidPlaybackState("no track is loaded".to_owned()))?;
        let clamped_ms = if track.duration_ms() == 0 {
            target_ms
        } else {
            target_ms.min(track.duration_ms())
        };
        // Reset render frame to match the new seek position — this is the
        // sole authority for position_ms.
        let sample_rate = track.original_audio.sample_rate as f64;
        let target_frame = (clamped_ms as f64 * sample_rate / 1000.0) as u64;
        track.render_frame = target_frame;

        // Propagate seek to streaming consumers so their decode threads
        // seek the symphonia decoder to the new position.
        if let Some(ref mut streaming) = track.streaming {
            for consumer in streaming.consumers_mut() {
                consumer
                    .seek_target()
                    .store(target_frame as i64, std::sync::atomic::Ordering::Relaxed);
            }
            // Set buffering while the decode threads seek and refill.
            self.is_buffering = true;
        }

        Ok(self.snapshot())
    }

    pub fn set_volume(&mut self, level: f32) -> Result<PlaybackStateSnapshot, PlaybackError> {
        self.volume = level.clamp(0.0, 1.0);
        Ok(self.snapshot())
    }

    pub fn set_stem_volume(
        &mut self,
        stem: StemName,
        level: f32,
    ) -> Result<PlaybackStateSnapshot, PlaybackError> {
        let level = level.clamp(0.0, 1.0);
        match stem {
            StemName::Vocals => self.stem_volumes.vocals = level,
            StemName::Drums => self.stem_volumes.drums = level,
            StemName::Bass => self.stem_volumes.bass = level,
            StemName::Other => self.stem_volumes.other = level,
        }
        Ok(self.snapshot())
    }

    /// Update the EQ enabled flag and bump the config revision so the realtime
    /// output callback picks up the change. Returns the current snapshot.
    pub fn set_eq_enabled(&mut self, enabled: bool) -> PlaybackStateSnapshot {
        if self.eq_config.enabled != enabled {
            self.eq_config.enabled = enabled;
            self.eq_config.revision = self.eq_config.revision.saturating_add(1);
        }
        self.snapshot()
    }

    /// Update the per-band EQ gains (dB) and bump the config revision so the
    /// realtime output callback picks up the change. Returns the current
    /// snapshot. The caller is expected to have validated the gains via
    /// `eq::validate_gains_db` before dispatching the command.
    pub fn set_eq_gains(&mut self, gains_db: [f32; 5]) -> PlaybackStateSnapshot {
        if self.eq_config.gains_db != gains_db {
            self.eq_config.gains_db = gains_db;
            self.eq_config.revision = self.eq_config.revision.saturating_add(1);
        }
        self.snapshot()
    }

    /// Current EQ config snapshot. The realtime output callback polls this
    /// while it already holds the controller lock and compares the revision
    /// with the processor's last-applied revision.
    pub fn eq_config(&self) -> crate::audio::eq::EqConfig {
        self.eq_config
    }

    pub fn attach_stems(&mut self, song_id: &str, stems: LoadedStems) -> Result<(), PlaybackError> {
        let track = self
            .current_track
            .as_mut()
            .ok_or_else(|| PlaybackError::InvalidPlaybackState("no track is loaded".to_owned()))?;
        if track.song_id != song_id {
            return Err(PlaybackError::InvalidPlaybackState(format!(
                "cannot attach stems for song {} while {} is loaded",
                song_id, track.song_id
            )));
        }
        track.stems = Some(stems);
        Ok(())
    }

    /// Replace the streaming track's single consumer with multi-stem consumers.
    /// Used when stems are loaded in streaming mode after the main track started.
    pub fn attach_streaming_stems(
        &mut self,
        song_id: &str,
        stem_track: super::streaming::StreamingTrack,
    ) -> Result<(), PlaybackError> {
        let track = self
            .current_track
            .as_mut()
            .ok_or_else(|| PlaybackError::InvalidPlaybackState("no track is loaded".to_owned()))?;
        if track.song_id != song_id {
            return Err(PlaybackError::InvalidPlaybackState(format!(
                "cannot attach streaming stems for song {} while {} is loaded",
                song_id, track.song_id
            )));
        }
        track.streaming = Some(stem_track);
        Ok(())
    }

    pub fn has_stems(&self) -> bool {
        self.current_track
            .as_ref()
            .and_then(|t| t.stems.as_ref())
            .is_some()
    }

    /// Returns the stem mode string if stems are loaded: "two_stem" or "four_stem".
    pub fn stem_variant(&self) -> Option<&str> {
        self.current_track
            .as_ref()
            .and_then(|t| t.stems.as_ref())
            .map(|s| match s {
                LoadedStems::TwoStem { .. } => "two_stem",
                LoadedStems::FourStem(_) => "four_stem",
            })
    }

    pub fn snapshot(&mut self) -> PlaybackStateSnapshot {
        if let Some(track) = self.current_track.as_mut() {
            let duration_ms = track.duration_ms();
            let raw_position = track.position_ms();

            // Clamp to duration and stop playback if past the end.
            // duration_ms == 0 means unknown — do not clamp until EOF backfill.
            let position_ms = if duration_ms > 0 && raw_position >= duration_ms {
                track.is_playing = false;
                duration_ms
            } else {
                raw_position
            };

            let stem_mode = track.stems.as_ref().map(|s| match s {
                LoadedStems::TwoStem { .. } => "two_stem".to_owned(),
                LoadedStems::FourStem(_) => "four_stem".to_owned(),
            });

            // Streaming stems also count as "has stems".
            let has_stems = track.stems.is_some()
                || matches!(
                    track.streaming,
                    Some(super::streaming::StreamingTrack::TwoStem { .. })
                        | Some(super::streaming::StreamingTrack::FourStem { .. })
                );

            // Derive stem_mode from streaming track if not already set by pre-decoded stems.
            let stem_mode = stem_mode.or_else(|| match track.streaming {
                Some(super::streaming::StreamingTrack::TwoStem { .. }) => {
                    Some("two_stem".to_owned())
                }
                Some(super::streaming::StreamingTrack::FourStem { .. }) => {
                    Some("four_stem".to_owned())
                }
                _ => None,
            });

            // RATIONALE: is_playing reflects transport intent for the UI. During a
            // fade-out the audio thread still renders the envelope, but the user has
            // already paused — report is_playing=false immediately. During buffer
            // underrun the state becomes "buffering" but the user has not paused.
            // output.rs gates silence on is_buffering separately from is_playing.
            let transport_playing =
                track.is_playing && !matches!(self.fade, FadeState::FadingOut { .. });
            let (state, is_playing) = if self.is_buffering {
                ("buffering", transport_playing)
            } else {
                ("playing", transport_playing)
            };

            // Derive buffered_ms: in streaming mode, compute from ring-buffer
            // water level (min across all consumers); in whole-track mode, the
            // entire track is buffered.
            let buffered_ms = if let Some(ref streaming) = track.streaming {
                let min_available_ms = match streaming {
                    super::streaming::StreamingTrack::Single { consumer } => {
                        consumer.available_ms()
                    }
                    super::streaming::StreamingTrack::TwoStem {
                        vocals,
                        accompaniment,
                    } => vocals.available_ms().min(accompaniment.available_ms()),
                    super::streaming::StreamingTrack::FourStem {
                        vocals,
                        drums,
                        bass,
                        other,
                    } => vocals
                        .available_ms()
                        .min(drums.available_ms())
                        .min(bass.available_ms())
                        .min(other.available_ms()),
                };
                let cap = if duration_ms > 0 {
                    duration_ms
                } else {
                    u64::MAX
                };
                (position_ms + min_available_ms).min(cap)
            } else {
                duration_ms
            };

            return PlaybackStateSnapshot {
                song_id: Some(track.song_id.clone()),
                transport_generation: self.transport_generation,
                state: state.to_owned(),
                is_playing,
                position_ms,
                duration_ms: if duration_ms > 0 {
                    Some(duration_ms)
                } else {
                    None
                },
                buffered_ms,
                volume: self.volume,
                stem_volumes: self.stem_volumes,
                has_stems,
                stem_mode,
            };
        }

        if let Some(song_id) = &self.loading_song_id {
            return PlaybackStateSnapshot {
                song_id: Some(song_id.clone()),
                transport_generation: self.transport_generation,
                state: "loading".to_owned(),
                is_playing: false,
                position_ms: 0,
                duration_ms: None,
                buffered_ms: 0,
                volume: self.volume,
                stem_volumes: self.stem_volumes,
                has_stems: false,
                stem_mode: None,
            };
        }

        self.idle_snapshot()
    }

    pub fn current_song_id(&self) -> Option<&str> {
        self.current_track
            .as_ref()
            .map(|track| track.song_id.as_str())
    }

    pub fn clear_track(&mut self) {
        self.current_track = None;
        self.loading_song_id = None;
        self.fade = FadeState::None;
    }

    /// Clear a pending background load when decode/start fails for the given song.
    pub fn cancel_loading_if_matching(&mut self, song_id: &str) -> bool {
        if self.loading_song_id.as_deref() == Some(song_id) {
            self.loading_song_id = None;
            return true;
        }
        false
    }

    /// Clear an installed track when output-device startup fails after
    /// `InstallReady` has already called `start_track` / `start_track_streaming`.
    /// Unlike `cancel_loading_if_matching`, this handles the post-install case
    /// where `loading_song_id` is already `None` but `current_track` holds the
    /// song that cannot play without an output device. Returns `true` when the
    /// installed track matched and was cleared.
    pub fn clear_track_if_matching(&mut self, song_id: &str) -> bool {
        if self
            .current_track
            .as_ref()
            .is_some_and(|t| t.song_id == song_id)
        {
            self.clear_track();
            return true;
        }
        false
    }

    fn idle_snapshot(&self) -> PlaybackStateSnapshot {
        PlaybackStateSnapshot {
            volume: self.volume,
            stem_volumes: self.stem_volumes,
            ..PlaybackStateSnapshot::idle()
        }
    }

    /// Returns the current render frame (source-rate frame index).
    pub fn current_render_frame(&self) -> u64 {
        self.current_track.as_ref().map_or(0, |t| t.render_frame)
    }

    /// Advance the render frame counter after the output callback renders audio.
    pub fn advance_render_frame(&mut self, frames: u64) {
        if let Some(track) = &mut self.current_track {
            track.render_frame += frames;
        }
    }

    /// Called from the audio output thread when every streaming consumer has
    /// reached EOF and drained its ring buffer.
    pub(crate) fn finalize_streaming_natural_end(&mut self) {
        let Some(track) = self.current_track.as_mut() else {
            return;
        };
        let Some(streaming) = track.streaming.as_ref() else {
            return;
        };
        if !streaming.all_eof_and_drained() {
            return;
        }
        if track.is_playing {
            track.is_playing = false;
        }
        if track.original_audio.duration_ms == 0 {
            track.original_audio.duration_ms = track.position_ms();
        }
    }

    /// #88: Check whether the current decoded (non-streaming) track has
    /// reached its end. The render callback calls this after advancing
    /// `render_frame` to decide whether a gapless swap should occur.
    pub(crate) fn current_track_reached_eof(&self) -> bool {
        let Some(track) = self.current_track.as_ref() else {
            return false;
        };
        if let Some(streaming) = &track.streaming {
            // Streaming tracks use `all_eof_and_drained` on the ring buffer.
            return streaming.all_eof_and_drained();
        }
        let total_frames = track.original_audio.samples.len() / track.original_audio.channels;
        track.render_frame >= total_frames as u64
    }

    /// #103: Whether the current track is actively playing — i.e. the user
    /// has not paused and no pause fade-out is in progress. The gapless swap
    /// path checks this before advancing to the prepared next track so that a
    /// track reaching EOF during a user-initiated pause (or while paused with
    /// `render_frame` already at EOF) does not auto-advance. Without this
    /// guard, pausing near the end of a track would still swap to the
    /// preloaded next track once the fade-out renders the final frames,
    /// defeating the user's intent to stop at the current song.
    ///
    /// A `FadingIn` state (user just pressed play/resume) also counts as
    /// playing, even if `snapshot()` has set `is_playing = false` because
    /// the track is at EOF. Without this, a user who pauses near EOF and
    /// then resumes would be stuck — the track is at EOF, `is_playing` is
    /// false, and the gapless swap is permanently suppressed.
    pub(crate) fn current_track_is_playing(&self) -> bool {
        let Some(track) = self.current_track.as_ref() else {
            return false;
        };
        if matches!(self.fade, FadeState::FadingOut { .. }) {
            return false;
        }
        track.is_playing || matches!(self.fade, FadeState::FadingIn { .. })
    }

    /// #88: Perform a gapless swap from the current track to the prepared
    /// track. Called by the realtime callback when the current track reaches
    /// EOF and a prepared track is available. Returns `true` if the swap
    /// occurred.
    ///
    /// This is the only path that consumes `prepared_track`. The new track
    /// starts playing immediately at `render_frame = 0` with `is_playing =
    /// true`, and a `CompletedTransition` is stamped for the position emitter
    /// to drain.
    pub(crate) fn perform_gapless_swap(&mut self) -> bool {
        let Some(prepared) = self.prepared_track.take() else {
            return false;
        };
        let Some(current) = self.current_track.as_ref() else {
            // No current track — shouldn't happen, but be defensive.
            self.prepared_track = Some(prepared);
            return false;
        };

        // The prepared track's audio was already normalized to the output
        // format by the preload scheduler, so sample_rate and channels match
        // the current track. We construct a new LoadedTrack with the
        // prepared audio.
        let from_song_id = current.song_id.clone();
        let to_song_id = prepared.song_id.clone();
        let preload_generation = prepared.preload_generation;

        self.current_track = Some(LoadedTrack {
            song_id: prepared.song_id,
            original_audio: prepared.audio,
            stems: None,
            is_playing: true,
            render_frame: 0,
            streaming: None,
        });

        // Clear transport state carried over from the previous track. The
        // new track starts fresh at frame 0 with no fade and no buffering
        // flag. Without clearing `fade`, a fade-out in progress when the
        // previous track reached EOF would be applied to the new track,
        // briefly attenuating it and then setting is_playing=false when the
        // fade completes — defeating the gapless transition. `is_buffering`
        // is only set for streaming tracks, but clearing it defensively
        // guards against any stale state.
        self.fade = FadeState::None;
        self.is_buffering = false;

        // #103: Bump the transport generation so the frontend's stale-event
        // filter rejects any delayed `playback-position` event from the old
        // song. The frontend only discards events with a *lower* generation,
        // so without this bump a same-generation position event for song-a
        // could arrive after the new-song snapshot and be accepted, reverting
        // the clock and queue reconciliation back to song-a.
        self.bump_transport_generation();

        // Stamp the transition for the position emitter to drain.
        self.stamp_transition(from_song_id, to_song_id, preload_generation);

        true
    }

    /// If a fade-out has elapsed past `FADE_DURATION`, finalize it: set
    /// `is_playing = false` and clear the fade state.  Called before
    /// `snapshot()` so the snapshot correctly reports the paused state.
    pub(crate) fn finalize_fade_if_complete(&mut self) {
        if let FadeState::FadingOut { start } = self.fade {
            if start.elapsed() >= FADE_DURATION {
                self.fade = FadeState::None;
                if let Some(track) = &mut self.current_track {
                    track.is_playing = false;
                }
            }
        }
    }

    /// Compute the fade gain to apply to the rendered buffer.  Returns `None`
    /// if no fade is active, or `Some(gain)` with the gain to multiply into
    /// every output sample.
    ///
    /// For fade-ins, resets `fade` to `None` once the envelope completes.
    pub(crate) fn take_fade_gain(&mut self) -> Option<f32> {
        match self.fade {
            FadeState::None => None,
            FadeState::FadingIn { start } => {
                let elapsed = start.elapsed();
                if elapsed >= FADE_DURATION {
                    self.fade = FadeState::None;
                    Some(1.0)
                } else {
                    Some(elapsed.as_secs_f32() / FADE_DURATION.as_secs_f32())
                }
            }
            FadeState::FadingOut { start } => {
                let elapsed = start.elapsed();
                if elapsed >= FADE_DURATION {
                    // Already finalized by finalize_fade_if_complete; this
                    // branch is a safety net.
                    self.fade = FadeState::None;
                    Some(0.0)
                } else {
                    Some(1.0 - elapsed.as_secs_f32() / FADE_DURATION.as_secs_f32())
                }
            }
            FadeState::FadingAfterSeek { start } => {
                let elapsed = start.elapsed();
                if elapsed >= SEEK_FADE_DURATION {
                    self.fade = FadeState::None;
                    Some(1.0)
                } else {
                    Some(elapsed.as_secs_f32() / SEEK_FADE_DURATION.as_secs_f32())
                }
            }
        }
    }
}

impl LoadedTrack {
    fn duration_ms(&self) -> u64 {
        self.original_audio.duration_ms
    }

    /// Position derived solely from `render_frame` — the authoritative clock.
    fn position_ms(&self) -> u64 {
        let sample_rate = self.original_audio.sample_rate as u64;
        if sample_rate == 0 {
            return 0;
        }
        let pos = (self.render_frame * 1000) / sample_rate;
        if self.duration_ms() > 0 {
            pos.min(self.duration_ms())
        } else {
            pos
        }
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaybackPositionEvent {
    pub ms: u64,
    pub transport_generation: u64,
    pub snapshot: PlaybackStateSnapshot,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaybackEndedEvent {
    pub song_id: String,
}

pub fn playback_position_event(snapshot: &PlaybackStateSnapshot) -> PlaybackPositionEvent {
    PlaybackPositionEvent {
        ms: snapshot.position_ms,
        transport_generation: snapshot.transport_generation,
        snapshot: snapshot.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        playback_position_event, PlaybackStateSnapshot, StemVolumes,
        PLAYBACK_POSITION_POLL_INTERVAL_MS,
    };

    #[test]
    fn playback_position_poll_interval_targets_thirty_hz() {
        assert_eq!(PLAYBACK_POSITION_POLL_INTERVAL_MS, 33);
    }

    #[test]
    fn playback_position_event_carries_the_authoritative_snapshot() {
        let snapshot = PlaybackStateSnapshot {
            song_id: Some("song-a".to_owned()),
            transport_generation: 7,
            state: "playing".to_owned(),
            is_playing: true,
            position_ms: 1_234,
            duration_ms: Some(5_000),
            buffered_ms: 5_000,
            volume: 0.8,
            stem_volumes: StemVolumes::default(),
            has_stems: false,
            stem_mode: None,
        };

        let event = playback_position_event(&snapshot);

        assert_eq!(event.ms, 1_234);
        assert_eq!(event.transport_generation, 7);
        assert_eq!(event.snapshot, snapshot);
    }

    #[test]
    fn playback_controller_reports_loading_until_track_starts() {
        let mut controller = super::PlaybackController::default();

        let loading = controller.start_track_loading("song-a");
        assert_eq!(loading.song_id.as_deref(), Some("song-a"));
        assert_eq!(loading.transport_generation, 1);
        assert_eq!(loading.state, "loading");

        let snapshot = controller.snapshot();
        assert_eq!(snapshot.song_id.as_deref(), Some("song-a"));
        assert_eq!(snapshot.transport_generation, 1);
        assert_eq!(snapshot.state, "loading");
        assert!(!snapshot.is_playing);

        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 1_000,
            samples: vec![0.0; 44_100 * 2],
        };
        let started = controller.start_track("song-a".to_owned(), decoded, 1_000);
        assert_eq!(started.state, "playing");
        assert_eq!(started.transport_generation, 1);
        assert!(started.is_playing);
    }

    #[test]
    fn cancel_loading_if_matching_clears_pending_load() {
        let mut controller = super::PlaybackController::default();
        controller.start_track_loading("song-a");
        assert!(controller.cancel_loading_if_matching("song-a"));

        let snapshot = controller.snapshot();
        assert_eq!(snapshot.song_id, None);
        assert_eq!(snapshot.state, "idle");
    }

    #[test]
    fn position_ms_derives_from_render_frame_not_wall_clock() {
        use super::DecodedAudio;

        let mut controller = super::PlaybackController::default();
        // 5-second track at 44.1 kHz stereo
        let decoded = DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 100);
        controller.play(100).unwrap();

        // Advance render_frame by 44100 samples = 1 second at 44.1 kHz
        controller.advance_render_frame(44_100);

        // Wall clock says 5100ms (5 seconds later), but render_frame only
        // advanced 1 second — position must reflect render_frame, not wall.
        let snapshot = controller.snapshot();
        assert_eq!(
            snapshot.position_ms, 1_000,
            "position_ms must derive from render_frame, not wall clock"
        );
    }

    #[test]
    fn snapshot_reports_paused_during_fade_out() {
        use super::DecodedAudio;

        let mut controller = super::PlaybackController::default();
        let decoded = DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        controller.play(0).unwrap();

        controller.pause(0).unwrap();
        let snap = controller.snapshot();
        assert_eq!(snap.state, "playing");
        assert!(
            !snap.is_playing,
            "pause must report is_playing=false while the fade-out envelope runs"
        );
    }

    #[test]
    fn snapshot_reports_buffering_state_when_flag_set() {
        use super::DecodedAudio;

        let mut controller = super::PlaybackController::default();
        let decoded = DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);

        // Normal playing state
        let snap = controller.snapshot();
        assert_eq!(snap.state, "playing");
        assert!(snap.is_playing);

        // Set buffering flag — state changes, transport intent stays playing
        controller.is_buffering = true;
        let snap = controller.snapshot();
        assert_eq!(snap.state, "buffering");
        assert!(snap.is_playing);

        // Clear buffering — back to playing
        controller.is_buffering = false;
        let snap = controller.snapshot();
        assert_eq!(snap.state, "playing");
        assert!(snap.is_playing);
    }

    #[test]
    fn seek_activates_fade_to_prevent_click() {
        use super::DecodedAudio;
        use super::FadeState;

        let mut controller = super::PlaybackController::default();
        let decoded = DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        controller.play(0).unwrap();

        // Before seek, fade should be None (play fade already completed).
        controller.fade = FadeState::None;

        // Seek should activate FadingAfterSeek.
        controller.seek(2_000, 0).unwrap();
        assert!(
            matches!(controller.fade, FadeState::FadingAfterSeek { .. }),
            "seek should activate FadingAfterSeek, got {:?}",
            controller.fade
        );

        // take_fade_gain should return a value < 1.0 immediately after seek.
        let gain = controller.take_fade_gain();
        assert!(gain.is_some(), "fade gain should be active after seek");
        let gain = gain.unwrap();
        assert!(
            gain < 1.0,
            "gain immediately after seek should be < 1.0, got {gain}"
        );
    }

    #[test]
    fn seek_fade_is_shorter_than_play_fade() {
        use super::DecodedAudio;
        use super::FadeState;

        let mut controller = super::PlaybackController::default();
        let decoded = DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);

        // Play fade starts
        controller.play(0).unwrap();
        assert!(matches!(controller.fade, FadeState::FadingIn { .. }));

        // Seek should override play fade with seek fade.
        controller.seek(1_000, 0).unwrap();
        assert!(matches!(controller.fade, FadeState::FadingAfterSeek { .. }));
    }

    #[test]
    fn seek_does_not_report_paused() {
        use super::DecodedAudio;

        let mut controller = super::PlaybackController::default();
        let decoded = DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        controller.play(0).unwrap();

        // Seek should keep is_playing = true (unlike pause which sets it to false).
        let snap = controller.seek(2_000, 0).unwrap();
        assert!(
            snap.is_playing,
            "seek should keep is_playing = true, got false"
        );
    }

    #[test]
    fn buffered_ms_defaults_to_duration_in_whole_track_mode() {
        use super::DecodedAudio;

        let mut controller = super::PlaybackController::default();

        // Idle: buffered_ms = 0
        let snap = controller.snapshot();
        assert_eq!(snap.buffered_ms, 0);

        let decoded = DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);

        // Whole-track mode: buffered_ms == duration_ms
        let snap = controller.snapshot();
        assert_eq!(snap.buffered_ms, 5_000);
        assert_eq!(snap.buffered_ms, snap.duration_ms.unwrap());
    }

    // ── #88: Gapless prepared-track tests ──────────────────────────────

    fn make_prepared(
        song_id: &str,
        preload_request_generation: u64,
        output_format: super::OutputFormatSnapshot,
    ) -> super::PreparedTrack {
        super::PreparedTrack {
            preload_request_generation,
            preload_generation: output_format.generation,
            song_id: song_id.to_owned(),
            output_format,
            audio: super::DecodedAudio {
                sample_rate: output_format.sample_rate,
                channels: output_format.channels as usize,
                duration_ms: 5_000,
                samples: vec![
                    0.0;
                    (output_format.sample_rate as usize)
                        * (output_format.channels as usize)
                        * 5
                ],
            },
        }
    }

    #[test]
    fn install_prepared_track_accepts_matching_generation_and_format() {
        let mut controller = super::PlaybackController::default();
        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);
        let prepared = make_prepared("song-b", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());
        assert!(controller.prepared_track.is_some());
    }

    #[test]
    fn install_prepared_track_rejects_stale_preload_request_generation() {
        // #88 race-condition fix: an old preload thread that passed its
        // shutdown check before the flag was set but sends PrepareNext after
        // the coordinator processed CancelPreparedNext must be rejected.
        let mut controller = super::PlaybackController::default();
        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);

        // Cancel bumps expected generation to 1.
        assert!(!controller.cancel_prepared_track(1));

        // Old preload thread sends PrepareNext with generation 0 — rejected.
        let stale = make_prepared("song-old", 0, fmt);
        assert!(controller.install_prepared_track(stale, fmt).is_err());
        assert!(controller.prepared_track.is_none());

        // New preload thread sends PrepareNext with generation 1 — accepted.
        let fresh = make_prepared("song-new", 1, fmt);
        assert!(controller.install_prepared_track(fresh, fmt).is_ok());
        assert!(controller.prepared_track.is_some());
    }

    #[test]
    fn install_prepared_track_rejects_mismatched_output_format() {
        let mut controller = super::PlaybackController::default();
        let prepared_fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);
        let current_fmt = super::OutputFormatSnapshot::new(2, 48_000, 2);
        let prepared = make_prepared("song-b", 0, prepared_fmt);
        assert!(controller
            .install_prepared_track(prepared, current_fmt)
            .is_err());
        assert!(controller.prepared_track.is_none());
    }

    #[test]
    fn install_prepared_track_accepts_when_source_rate_differs_from_output() {
        // The prepared track is normalized to the OUTPUT format, not the
        // current track's SOURCE format. This test verifies the fix for the
        // original bug where the check compared against the current track's
        // source format instead of the output format.
        let mut controller = super::PlaybackController::default();
        // Load a track at 48 kHz source rate.
        let decoded = super::DecodedAudio {
            sample_rate: 48_000,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 48_000 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);

        // Output device runs at 44.1 kHz. Prepared track is normalized to
        // 44.1 kHz. The old (buggy) check would compare 48000 != 44100 and
        // reject; the fix compares against the output format (44100 == 44100).
        let output_fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);
        let prepared = make_prepared("song-b", 0, output_fmt);
        assert!(controller
            .install_prepared_track(prepared, output_fmt)
            .is_ok());
    }

    #[test]
    fn cancel_prepared_track_stamps_expected_generation() {
        let mut controller = super::PlaybackController::default();
        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);

        // Install a prepared track at generation 0.
        let prepared = make_prepared("song-b", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());
        assert!(controller.prepared_track.is_some());

        // Cancel with generation 1 — should clear the prepared track and
        // stamp expected generation to 1.
        assert!(controller.cancel_prepared_track(1));
        assert!(controller.prepared_track.is_none());
        assert_eq!(controller.expected_preload_request_generation, 1);
    }

    #[test]
    fn perform_gapless_swap_clears_fade_and_buffering() {
        let mut controller = super::PlaybackController::default();
        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);

        // Load track A at the output format rate.
        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        controller.play(0).unwrap();
        controller.pause(0).unwrap();
        // Now fade is FadingOut.
        assert!(matches!(
            controller.fade,
            super::FadeState::FadingOut { .. }
        ));
        controller.is_buffering = true;

        // Install a prepared track.
        let prepared = make_prepared("song-b", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());

        // Perform the gapless swap.
        assert!(controller.perform_gapless_swap());

        // Fade and buffering must be cleared — the new track starts fresh.
        assert!(matches!(controller.fade, super::FadeState::None));
        assert!(!controller.is_buffering);
        assert_eq!(controller.current_track.as_ref().unwrap().song_id, "song-b");
    }

    #[test]
    fn perform_gapless_swap_stamps_transition() {
        let mut controller = super::PlaybackController::default();
        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);

        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);

        let prepared = make_prepared("song-b", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());

        assert!(controller.perform_gapless_swap());

        let transition = controller.drain_pending_transition().expect("transition");
        assert_eq!(transition.from_song_id, "song-a");
        assert_eq!(transition.to_song_id, "song-b");
        assert_eq!(transition.transition_serial, 1);
    }

    #[test]
    fn perform_gapless_swap_bumps_transport_generation() {
        // #103: A gapless swap replaces song-a with song-b but must bump the
        // transport generation so the frontend's stale-event filter rejects
        // delayed `playback-position` events from song-a. Without the bump,
        // a same-generation position event for the old song could arrive
        // after the new-song snapshot and be accepted, reverting the clock
        // and queue reconciliation back to song-a.
        let mut controller = super::PlaybackController::default();
        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);

        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        let gen_before = controller.transport_generation;

        let prepared = make_prepared("song-b", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());

        assert!(controller.perform_gapless_swap());

        // The generation must have advanced so stale events from song-a are
        // rejected by the frontend's generation filter.
        assert!(
            controller.transport_generation > gen_before,
            "gapless swap must bump transport_generation (was {gen_before}, is {})",
            controller.transport_generation
        );
    }

    #[test]
    fn perform_gapless_swap_returns_false_without_prepared_track() {
        let mut controller = super::PlaybackController::default();
        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        assert!(!controller.perform_gapless_swap());
    }

    #[test]
    fn start_track_clears_prepared_track() {
        let mut controller = super::PlaybackController::default();
        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);

        // Install a prepared track.
        let prepared = make_prepared("song-b", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());
        assert!(controller.prepared_track.is_some());

        // Starting a new track should clear the prepared track — the new
        // track is not the prepared one.
        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-c".to_owned(), decoded, 0);
        assert!(controller.prepared_track.is_none());
    }

    #[test]
    fn clear_track_clears_prepared_track() {
        let mut controller = super::PlaybackController::default();
        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);

        let prepared = make_prepared("song-b", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());
        assert!(controller.prepared_track.is_some());

        controller.clear_track();
        assert!(controller.prepared_track.is_none());
    }

    // ── #103: current_track_is_playing regression tests ───────────────
    //
    // The gapless swap path in the realtime callback checks
    // `current_track_is_playing()` before advancing to the prepared next
    // track. This helper returns false when `is_playing` is false or a
    // `FadingOut` is in progress, so a track reaching EOF during a
    // user-initiated pause does not auto-advance.

    #[test]
    fn current_track_is_playing_true_when_playing_no_fade() {
        let mut controller = super::PlaybackController::default();
        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        controller.play(0).unwrap();
        // Clear the fade-in so we're in a steady playing state.
        controller.fade = super::FadeState::None;

        assert!(
            controller.current_track_is_playing(),
            "should be playing when is_playing=true and no fade-out"
        );
    }

    #[test]
    fn current_track_is_playing_false_during_fade_out() {
        let mut controller = super::PlaybackController::default();
        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        controller.play(0).unwrap();
        controller.pause(0).unwrap();

        // During the fade-out, is_playing is still true on the track, but
        // the FadingOut state means the user has paused — the helper must
        // return false so the gapless swap is suppressed.
        assert!(
            !controller.current_track_is_playing(),
            "should not be playing during a fade-out"
        );
    }

    #[test]
    fn current_track_is_playing_false_when_no_track_loaded() {
        let controller = super::PlaybackController::default();
        assert!(
            !controller.current_track_is_playing(),
            "should return false when no track is loaded"
        );
    }

    #[test]
    fn current_track_is_playing_true_again_after_resume_from_pause() {
        // #103: After pausing (fade-out) and then resuming (play), the
        // helper must return true again so the gapless swap can proceed if
        // the track is at EOF. Without this, a user who pauses near EOF
        // and then resumes would be stuck — the track is at EOF but the
        // gapless swap is permanently suppressed.
        let mut controller = super::PlaybackController::default();
        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        controller.play(0).unwrap();
        controller.fade = super::FadeState::None;

        // Pause — helper returns false.
        controller.pause(0).unwrap();
        assert!(!controller.current_track_is_playing());

        // Resume — play() sets is_playing=true and FadingIn. The helper
        // must return true (FadingIn is not FadingOut).
        controller.play(0).unwrap();
        assert!(
            controller.current_track_is_playing(),
            "should be playing after resume (FadingIn is not FadingOut)"
        );
    }
}
