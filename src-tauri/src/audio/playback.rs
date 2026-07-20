use crate::audio::decode::DecodedAudio;
use crate::audio::error::PlaybackError;
use crate::audio::output_format::OutputFormatSnapshot;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const FADE_DURATION: Duration = Duration::from_millis(50);

/// 8ms is short enough to be perceptually transparent while masking any
/// amplitude discontinuity at the new playback position.
const SEEK_FADE_DURATION: Duration = Duration::from_millis(8);

pub const PLAYBACK_POSITION_EVENT: &str = "playback-position";
pub const PLAYBACK_ENDED_EVENT: &str = "playback-ended";
pub const PLAYBACK_ERROR_EVENT: &str = "playback-error";
pub const TRACK_TRANSITIONED_EVENT: &str = "track-transitioned";
/// Emitted by the playback reconnect coordinator (PR #7, issue #151) before
/// each re-resolve attempt so the frontend (PR #8) can show a "reconnecting…"
/// state. Payload: [`RemotePlaybackReconnectEvent`].
pub const REMOTE_PLAYBACK_RECONNECT_EVENT: &str = "remote-playback-reconnect";
/// Emitted when a reconnected source could not seek to the exact preserved
/// position and snapped to a preceding resumable boundary. Payload:
/// [`RemotePlaybackResyncEvent`].
pub const REMOTE_PLAYBACK_RESYNC_EVENT: &str = "remote-playback-resync";
/// Emitted after the reconnect attempt budget is exhausted or a permanent
/// error occurs. Payload: [`RemotePlaybackFailedEvent`].
pub const REMOTE_PLAYBACK_FAILED_EVENT: &str = "remote-playback-failed";
pub const PLAYBACK_POSITION_POLL_INTERVAL_MS: u64 = 33;

/// Opaque handle for the preload-request generation space.
///
/// `preload_request_generation` and `output_format.generation` are both
/// monotonic `u64` counters stored on `PreparedTrack` and compared in
/// `install_prepared_track`. Without a type wrapper, accidentally comparing
/// `prepared.preload_request_generation` against
/// `current_output_format.generation` (or vice versa) would compile silently
/// and produce a stale-track bug that only manifests under specific timing.
///
/// This newtype makes that cross-space comparison a compile error. The
/// `output_format.generation` field stays `u64` because it lives inside
/// the typed `OutputFormatSnapshot` struct, which already provides
/// namespace isolation.
///
/// This is the minimal scope — the two most confusable counters at the
/// comparison site. The coordinator's `request_id` (load-from-AtomicU64-and-compare)
/// and CDG's
/// `transport_generation` (crosses the IPC boundary to the frontend)
/// would require touching 100+ sites and the wire format for lower
/// marginal safety; they are documented as deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PreloadRequestGeneration(pub u64);

impl PreloadRequestGeneration {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for PreloadRequestGeneration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

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
    TwoStem {
        vocals: DecodedAudio,
        accompaniment: DecodedAudio,
    },
    FourStem(StemSet),
}

/// Validate that all stems in a `LoadedStems` share the same sample rate,
/// channel count, and frame count. The source-domain mix bus (issue #143)
/// pops the same `[frame, frame + budget)` range from every stem, so any
/// mismatch would cause one stem to exhaust early and stall the transport.
///
/// Returns `Ok(())` if all stems are consistent, or an `InvalidPlaybackState`
/// error describing the first mismatch.
fn validate_loaded_stems(stems: &LoadedStems) -> Result<(), PlaybackError> {
    match stems {
        LoadedStems::TwoStem {
            vocals,
            accompaniment,
        } => {
            validate_stem_pair("vocals", vocals, "accompaniment", accompaniment)?;
        }
        LoadedStems::FourStem(set) => {
            validate_stem_pair("vocals", &set.vocals, "drums", &set.drums)?;
            validate_stem_pair("vocals", &set.vocals, "bass", &set.bass)?;
            validate_stem_pair("vocals", &set.vocals, "other", &set.other)?;
        }
    }
    Ok(())
}

fn validate_stem_pair(
    name_a: &str,
    a: &DecodedAudio,
    name_b: &str,
    b: &DecodedAudio,
) -> Result<(), PlaybackError> {
    if a.sample_rate != b.sample_rate {
        return Err(PlaybackError::InvalidPlaybackState(format!(
            "stem timeline mismatch: {name_a} sample_rate {} != {name_b} sample_rate {}",
            a.sample_rate, b.sample_rate
        )));
    }
    if a.channels != b.channels {
        return Err(PlaybackError::InvalidPlaybackState(format!(
            "stem timeline mismatch: {name_a} channels {} != {name_b} channels {}",
            a.channels, b.channels
        )));
    }
    let frames_a = a.samples.len() / a.channels.max(1);
    let frames_b = b.samples.len() / b.channels.max(1);
    if frames_a != frames_b {
        return Err(PlaybackError::InvalidPlaybackState(format!(
            "stem timeline mismatch: {name_a} frame_count {frames_a} != {name_b} frame_count {frames_b}"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaybackStateSnapshot {
    pub song_id: Option<String>,
    /// Incremented when a new song load starts so webviews can discard
    /// delayed events from the previous transport.
    pub transport_generation: u64,
    /// `playing` means a decoded track owns the transport, not that time is advancing.
    pub state: String,
    pub is_playing: bool,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    /// In whole-track mode equals `duration_ms`; in streaming mode driven
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
    is_playing: bool,
    /// The sole authority for `position_ms`. Updated exclusively by the render
    /// callback; reset by seek / start_track.
    pub(crate) render_frame: u64,
    /// When `Some`, the render callback reads from these instead of
    /// `original_audio.samples`.
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
    pub preload_request_generation: PreloadRequestGeneration,
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
    /// Authoritative post-transition snapshot captured at the moment
    /// the track switched, not when the position emitter drains the
    /// transition. If the listener manually picks a different song in the
    /// brief gap between the swap and the notification, this snapshot still
    /// describes the song that actually played, so the frontend's
    /// `transport_generation` guard rejects the stale event and the queue
    /// is not reconciled against a song the listener never heard.
    pub snapshot: PlaybackStateSnapshot,
}

/// Active crossfade state owned by the realtime callback.
/// The callback creates this when the overlap begins, advances it each
/// frame, and promotes the incoming track when the overlap completes.
#[derive(Debug)]
pub struct ActiveCrossfade {
    pub prepared: PreparedTrack,
    pub total_frames: u64,
    pub rendered_frames: u64,
    /// Incoming track position in *source* frames. Advanced by
    /// `mix_stem_resampled`'s source-frame consumption so rate conversion
    /// cannot be fed a device-frame index.
    pub incoming_source_frame: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossfadeConfig {
    pub enabled: bool,
    pub duration_ms: u32,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CrossfadeState {
    pub enabled: bool,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FadeState {
    None,
    FadingIn {
        start: Instant,
    },
    FadingOut {
        start: Instant,
    },
    /// Short fade-in after a seek to mask amplitude discontinuity.
    FadingAfterSeek {
        start: Instant,
    },
}

#[derive(Debug)]
pub struct PlaybackController {
    pub(crate) current_track: Option<LoadedTrack>,
    loading_song_id: Option<String>,
    transport_generation: u64,
    volume: f32,
    stem_volumes: StemVolumes,
    /// When `true` and a track is loaded, `snapshot()` reports
    /// `state: "buffering"`. Set by the streaming layer on underrun; cleared
    /// when the buffer refills.
    pub(crate) is_buffering: bool,
    pub(crate) fade: FadeState,
    /// EQ config snapshot published by the controller and polled by the
    /// realtime output callback via `eq_config()`. The revision is bumped on
    /// every successful setter so the callback can detect changes without
    /// comparing the full struct each tick.
    pub(crate) eq_config: crate::audio::eq::EqConfig,
    /// A fully decoded, normalized next track prepared by the preload
    /// scheduler. The realtime callback is the only consumer — it swaps this
    /// into `current_track` when the current track reaches EOF. Stays `None`
    /// when no candidate is queued, the format changed, or the candidate was
    /// cancelled.
    pub(crate) prepared_track: Option<PreparedTrack>,
    /// Monotonic serial stamped onto each completed transition. Drained
    /// by the position emitter to emit `track-transitioned` before the next
    /// position event so the frontend can reconcile its queue head.
    pub(crate) transition_serial: u64,
    /// Completed transition metadata produced by the realtime callback
    /// after a gapless swap. The position emitter drains this (under the
    /// playback lock) and emits `TRACK_TRANSITIONED_EVENT` before the next
    /// `PLAYBACK_POSITION_EVENT`.
    pub(crate) pending_transition_out: Option<CompletedTransition>,
    /// Monotonic generation of the latest `set_preload_candidate`
    /// request. Incremented on every `cancel_prepared_track` call so that
    /// stale `PrepareNext` commands from an older preload thread (which
    /// passed its shutdown check before the flag was set but sends after the
    /// cancel) are rejected by `install_prepared_track`.
    pub(crate) expected_preload_request_generation: PreloadRequestGeneration,
    /// The output callback reads this while holding the playback lock to
    /// decide whether to start an overlap.
    pub(crate) crossfade_config: CrossfadeConfig,
    /// Owned by the realtime callback — created when the overlap begins,
    /// advanced each frame, and consumed when the overlap completes
    /// (promoting the incoming track).
    pub(crate) active_crossfade: Option<ActiveCrossfade>,
    /// Set by `abort_active_crossfade` so the realtime callback knows to
    /// clear the incoming resampler cache even though `prepared_track` was
    /// restored (which would otherwise skip the normal cleanup guard).
    pub(crate) crossfade_abort_pending: bool,
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
            prepared_track: None,
            transition_serial: 0,
            pending_transition_out: None,
            expected_preload_request_generation: PreloadRequestGeneration(0),
            crossfade_config: CrossfadeConfig {
                enabled: false,
                duration_ms: 3_000,
                revision: 0,
            },
            active_crossfade: None,
            crossfade_abort_pending: false,
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
        // An explicit track install cancels any pending gapless
        // successor — the new track is not the prepared one.
        // Also cancel any active crossfade — a manual load during an
        // active overlap must not leave a stale crossfade mixing the new
        // track with the old prepared payload.
        self.cancel_crossfade_and_prepared();
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
        // An explicit track install cancels any pending gapless
        // successor — the new track is not the prepared one.
        // Also cancel any active crossfade — see `start_track`.
        self.cancel_crossfade_and_prepared();
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
        // A new load request cancels any active crossfade and prepared
        // track — the incoming track is not the prepared one, and a stale
        // crossfade must not mix against the about-to-be-replaced current
        // track.
        self.cancel_crossfade_and_prepared();
        self.snapshot()
    }

    pub fn play(&mut self, _now_ms: u64) -> Result<PlaybackStateSnapshot, PlaybackError> {
        if self.current_track.is_none() {
            // RATIONALE: During the loading window (`start_track_loading`
            // set `loading_song_id` but the decoded audio hasn't arrived
            // yet), transport commands are semantically no-ops — the
            // track isn't installed. Return the loading snapshot so the
            // frontend can reconcile without surfacing a user-visible
            // error toast. When truly idle (no load in progress), keep
            // the error to surface real caller bugs.
            if self.loading_song_id.is_some() {
                return Ok(self.snapshot());
            }
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
            // See `play` for the loading-window no-op rationale.
            if self.loading_song_id.is_some() {
                return Ok(self.snapshot());
            }
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
        if self.current_track.is_none() {
            // See `play` for the loading-window no-op rationale.
            if self.loading_song_id.is_some() {
                return Ok(self.snapshot());
            }
            return Err(PlaybackError::InvalidPlaybackState(
                "no track is loaded".to_owned(),
            ));
        }
        // Cancel any active fade and start a short seek fade to mask
        // the amplitude discontinuity at the new position.
        self.bump_transport_generation();
        // Seek during an active crossfade aborts the overlap, restoring
        // the incoming payload to prepared_track at frame zero. The outgoing
        // track is then seeked normally, and a fresh crossfade may start
        // later if the remaining time permits.
        self.abort_active_crossfade();
        self.fade = FadeState::FadingAfterSeek {
            start: Instant::now(),
        };
        let track = self
            .current_track
            .as_mut()
            .expect("track existence checked before generation bump");
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

    pub fn set_eq_enabled(&mut self, enabled: bool) -> PlaybackStateSnapshot {
        if self.eq_config.enabled != enabled {
            self.eq_config.enabled = enabled;
            self.eq_config.revision = self.eq_config.revision.saturating_add(1);
        }
        self.snapshot()
    }

    /// The caller is expected to have validated the gains via
    /// `eq::validate_gains_db` before dispatching the command.
    pub fn set_eq_gains(&mut self, gains_db: [f32; 5]) -> PlaybackStateSnapshot {
        if self.eq_config.gains_db != gains_db {
            self.eq_config.gains_db = gains_db;
            self.eq_config.revision = self.eq_config.revision.saturating_add(1);
        }
        self.snapshot()
    }

    pub fn eq_config(&self) -> crate::audio::eq::EqConfig {
        self.eq_config
    }

    pub fn set_crossfade_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<CrossfadeState, PlaybackError> {
        if self.crossfade_config.enabled != enabled {
            self.crossfade_config.enabled = enabled;
            self.crossfade_config.revision = self.crossfade_config.revision.saturating_add(1);
        }
        Ok(self.crossfade_state())
    }

    /// The caller validates the range (500..=10_000). Bumps the config
    /// revision only when the value actually changes.
    pub fn set_crossfade_duration(
        &mut self,
        duration_ms: u32,
    ) -> Result<CrossfadeState, PlaybackError> {
        if self.crossfade_config.duration_ms != duration_ms {
            self.crossfade_config.duration_ms = duration_ms;
            self.crossfade_config.revision = self.crossfade_config.revision.saturating_add(1);
        }
        Ok(self.crossfade_state())
    }

    pub fn crossfade_state(&self) -> CrossfadeState {
        CrossfadeState {
            enabled: self.crossfade_config.enabled,
            duration_ms: self.crossfade_config.duration_ms,
        }
    }

    /// Abort an active crossfade, restoring the incoming payload to
    /// `prepared_track` at frame zero. Called when a seek occurs during
    /// an active overlap. The outgoing track is seeked separately by the
    /// caller.
    pub(crate) fn abort_active_crossfade(&mut self) {
        if let Some(active) = self.active_crossfade.take() {
            self.prepared_track = Some(active.prepared);
            // Signal the realtime callback to clear the incoming resampler
            // cache. The normal cleanup guard checks
            // `prepared_track.is_none()`, but abort restores the prepared
            // track, so without this flag the stale sinc state from the
            // aborted overlap position would persist into the next crossfade.
            self.crossfade_abort_pending = true;
        }
    }

    /// Cancel both active crossfade and prepared track. Called by
    /// manual play, stem attachment, and output-device recreation.
    pub(crate) fn cancel_crossfade_and_prepared(&mut self) {
        self.active_crossfade = None;
        self.prepared_track = None;
        // Also clear any pending outgoing transition. A manual load,
        // clear, stem attach, or load failure supersedes the track context
        // that produced the transition. Without this, the position emitter
        // could drain a stale transition (A→B) after the user has already
        // switched to an unrelated song C, causing the frontend to
        // reconcile the queue with the wrong song IDs.
        self.pending_transition_out = None;
    }

    pub fn attach_stems(&mut self, song_id: &str, stems: LoadedStems) -> Result<(), PlaybackError> {
        // Validate ownership first. Cancelling before the song-id guard would
        // permanently destroy an in-progress crossfade / prepared next-track
        // when a stale or misrouted attach arrives for a different song.
        let track = self
            .current_track
            .as_ref()
            .ok_or_else(|| PlaybackError::InvalidPlaybackState("no track is loaded".to_owned()))?;
        if track.song_id != song_id {
            return Err(PlaybackError::InvalidPlaybackState(format!(
                "cannot attach stems for song {} while {} is loaded",
                song_id, track.song_id
            )));
        }
        // Validate stem timeline consistency before installing. The mix bus
        // (issue #143) pops the same source-frame range from every stem, so
        // mismatched sample_rate, channels, or frame count would cause one
        // stem to run out of data early and stall the transport. Reject early
        // rather than producing silent drift or glitches mid-playback.
        validate_loaded_stems(&stems)?;

        // Successful attach on the current track cancels both active
        // crossfade and prepared track — stems change the render path and
        // make an outgoing plain-track overlap invalid.
        self.cancel_crossfade_and_prepared();
        self.current_track
            .as_mut()
            .expect("current_track present after ownership check")
            .stems = Some(stems);
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

    /// Atomically replace the active streaming source while preserving the
    /// playback timeline (PR #7, issue #151 defect #12).
    ///
    /// Called by the coordinator's `ReplaceStreamingSource` handler after a
    /// reconnect re-resolves a fresh source. The swap happens in one critical
    /// section (under the playback mutex): the old source is dropped only
    /// after the new one is installed, so there is no window where no source
    /// is active.
    ///
    /// `position_ms` is the position the old source was at when the failure
    /// occurred. The new source's consumers are seeked to that position
    /// (via their `seek_target`) so playback continues without an audible
    /// jump. Returns the snapshot after the swap, or `None` when the current
    /// track does not match `song_id` (the user skipped — stale reconnect).
    pub fn replace_streaming_source(
        &mut self,
        song_id: &str,
        new_streaming: super::streaming::StreamingTrack,
        position_ms: u64,
    ) -> Option<PlaybackStateSnapshot> {
        let track = self.current_track.as_mut()?;
        if track.song_id != song_id {
            // Stale reconnect — the user skipped to a different song. Drop
            // the new source (it drops here on return) and return None so
            // the coordinator no-ops.
            return None;
        }
        // Preserve the timeline: set render_frame to the position captured
        // before the swap. This is the sole authority for position_ms, so
        // the next snapshot reports the preserved position.
        let sample_rate = track.original_audio.sample_rate as f64;
        let target_frame = if sample_rate > 0.0 {
            (position_ms as f64 * sample_rate / 1000.0) as u64
        } else {
            0
        };
        track.render_frame = target_frame;
        let mut new_streaming = new_streaming;
        // Seek the new source's consumers to the preserved position so the
        // decode threads refill from the right offset.
        for consumer in new_streaming.consumers_mut() {
            consumer
                .seek_target()
                .store(target_frame as i64, std::sync::atomic::Ordering::Relaxed);
        }
        // Atomic swap: the old source is dropped when the field is
        // overwritten, after the new one is installed.
        track.streaming = Some(new_streaming);
        // Mark buffering while the new source's decode threads seek and
        // refill, so the snapshot reports "buffering" rather than
        // implying continuous audio.
        self.is_buffering = true;
        Some(self.snapshot())
    }

    pub fn has_stems(&self) -> bool {
        self.current_track
            .as_ref()
            .and_then(|t| t.stems.as_ref())
            .is_some()
    }

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

    /// Borrow the current track for read-only inspection (PR #7 reconnect).
    /// Exposed so the playback service can read the current position and
    /// song id without taking a mutable borrow.
    pub(crate) fn current_track_ref(&self) -> Option<&LoadedTrack> {
        self.current_track.as_ref()
    }

    /// Return the song identifier whose decode/load operation is still pending.
    ///
    /// The coordinator uses this to invalidate only an in-flight request for a
    /// song that has just been deleted, without canceling unrelated work.
    pub fn loading_song_id(&self) -> Option<&str> {
        self.loading_song_id.as_deref()
    }

    pub fn clear_track(&mut self) {
        self.current_track = None;
        self.loading_song_id = None;
        self.fade = FadeState::None;
        // Clearing the current track also invalidates any prepared
        // gapless successor — it was prepared relative to the track that is
        // now being replaced by an explicit user action.
        // Also cancel any active crossfade.
        self.cancel_crossfade_and_prepared();
    }

    /// Install a prepared next track for gapless transition. Called by
    /// the coordinator after the preload scheduler decodes and normalizes the
    /// candidate. Returns `Err` if the output format no longer matches the
    /// snapshot captured at prepare time (device restart or format change).
    ///
    /// `current_output_format` is the output-format snapshot re-captured by
    /// the coordinator after acquiring the playback lock. The prepared
    /// track's audio was normalized to the output format at prepare time;
    /// we validate against the output format (NOT the current track's source
    /// format, which may differ from the output format when the render
    /// callback resamples).
    pub fn install_prepared_track(
        &mut self,
        prepared: PreparedTrack,
        current_output_format: OutputFormatSnapshot,
    ) -> Result<(), PlaybackError> {
        // Reject stale preload requests. `cancel_prepared_track` bumps
        // `expected_preload_request_generation` on every cancel; if the
        // prepared track's generation doesn't match, it came from an older
        // preload thread that raced with a newer cancel (the thread passed
        // its shutdown check before the flag was set but sent PrepareNext
        // after the coordinator processed the cancel).
        if prepared.preload_request_generation != self.expected_preload_request_generation {
            return Err(PlaybackError::Internal(format!(
                "prepared track from stale preload request (prepared gen={}, expected gen={})",
                prepared.preload_request_generation.get(),
                self.expected_preload_request_generation.get(),
            )));
        }

        // The coordinator already validates the output format against the
        // current descriptor before calling this; this controller-level guard
        // is a defensive check against a stale prepared payload that slipped
        // through (e.g. format changed between the coordinator check and the
        // lock acquisition). We compare the prepared track's captured output
        // format against the re-captured current output format — NOT the
        // current track's source format, which may differ from the output
        // format when the render callback resamples (e.g. 48 kHz source on a
        // 44.1 kHz device).
        if prepared.output_format.generation != current_output_format.generation
            || prepared.output_format.sample_rate != current_output_format.sample_rate
            || prepared.output_format.channels != current_output_format.channels
        {
            return Err(PlaybackError::Internal(format!(
                "prepared track output format does not match current output format \
                 (prepared gen={}, rate={}, ch={} vs current gen={}, rate={}, ch={})",
                prepared.output_format.generation,
                prepared.output_format.sample_rate,
                prepared.output_format.channels,
                current_output_format.generation,
                current_output_format.sample_rate,
                current_output_format.channels,
            )));
        }
        self.prepared_track = Some(prepared);
        Ok(())
    }

    /// Cancel a pending prepared track. Called when the user manually
    /// skips, seeks, plays a different song, or the queue head changes.
    /// Returns `true` if a prepared track was present and cancelled.
    ///
    /// `expected_generation` is the new preload request generation (bumped
    /// by `set_preload_candidate` before sending the cancel command). We
    /// stamp it onto `expected_preload_request_generation` so that any
    /// `PrepareNext` from an older preload thread (which passed its shutdown
    /// check before the flag was set but sends after this cancel) is
    /// rejected by `install_prepared_track` as stale.
    pub fn cancel_prepared_track(&mut self, expected_generation: PreloadRequestGeneration) -> bool {
        self.expected_preload_request_generation = expected_generation;
        self.prepared_track.take().is_some()
    }

    /// Drain a completed transition produced by the realtime callback.
    /// The position emitter calls this under the playback lock to emit
    /// `track-transitioned` before the next position event.
    pub fn drain_pending_transition(&mut self) -> Option<CompletedTransition> {
        self.pending_transition_out.take()
    }

    /// Bump the transition serial and stamp it onto a new
    /// `CompletedTransition`. Called by the realtime callback after a
    /// gapless swap.
    /// The post-transition snapshot is captured here — under the
    /// playback lock at the moment of the swap — so the `track-transitioned`
    /// event describes the song that actually played even if the listener
    /// manually changes tracks before the position emitter drains the
    /// transition.
    fn stamp_transition(
        &mut self,
        from_song_id: String,
        to_song_id: String,
        preload_generation: u64,
    ) {
        self.transition_serial = self.transition_serial.saturating_add(1);
        let snapshot = self.snapshot();
        self.pending_transition_out = Some(CompletedTransition {
            transition_serial: self.transition_serial,
            preload_generation,
            from_song_id,
            to_song_id,
            snapshot,
        });
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

    /// Invalidate current and/or pending loads that match any of `song_ids`.
    /// Used after integrity cleanup deletes DB rows so the realtime path cannot
    /// keep rendering deleted media. Returns `true` when any state changed.
    pub fn invalidate_songs(&mut self, song_ids: &[String]) -> bool {
        let mut changed = false;
        for song_id in song_ids {
            if self.clear_track_if_matching(song_id) {
                changed = true;
            }
            if self.cancel_loading_if_matching(song_id) {
                changed = true;
            }
        }
        changed
    }

    fn idle_snapshot(&self) -> PlaybackStateSnapshot {
        PlaybackStateSnapshot {
            volume: self.volume,
            stem_volumes: self.stem_volumes,
            ..PlaybackStateSnapshot::idle()
        }
    }

    pub fn current_render_frame(&self) -> u64 {
        self.current_track.as_ref().map_or(0, |t| t.render_frame)
    }

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

    /// Check whether the current decoded (non-streaming) track has
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
        // Guard against a zero-channel track, which would panic on the
        // division below. A track with no channels can't produce audio, so
        // treat it as already at EOF.
        if track.original_audio.channels == 0 {
            return true;
        }
        let total_frames = track.original_audio.samples.len() / track.original_audio.channels;
        track.render_frame >= total_frames as u64
    }

    /// Whether the current track is actively playing — i.e. the user
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

    /// Perform a gapless swap from the current track to the prepared
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
        // A normal transition must discard inherited transport state, but a
        // user resuming at EOF intentionally has a FadingIn envelope. Keep
        // that envelope through the swap so the newly rendered tail cannot
        // jump from silence straight to full amplitude.
        let preserve_fade_in = matches!(self.fade, FadeState::FadingIn { .. });

        self.current_track = Some(LoadedTrack {
            song_id: prepared.song_id,
            original_audio: prepared.audio,
            stems: None,
            is_playing: true,
            render_frame: 0,
            streaming: None,
        });

        // Clear transport state carried over from the previous track. A
        // FadingOut must never carry into the successor, but preserve the
        // deliberate fade-in described above. `is_buffering` is only set for
        // streaming tracks, but clearing it defensively guards against stale
        // state.
        if !preserve_fade_in {
            self.fade = FadeState::None;
        }
        self.is_buffering = false;

        // Bump the transport generation so the frontend's stale-event
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

    /// Promote the incoming track from an active crossfade to the
    /// current track. Called by the realtime callback when the overlap
    /// completes. The incoming track starts at `render_frame =
    /// incoming_frame_offset` (the number of overlap source frames already
    /// consumed), so subsequent callbacks continue seamlessly from the
    /// promoted source. A `CompletedTransition` is stamped for the position
    /// emitter to drain.
    pub(crate) fn promote_crossfade_track(
        &mut self,
        prepared: PreparedTrack,
        incoming_frame_offset: u64,
    ) {
        let from_song_id = self
            .current_track
            .as_ref()
            .map(|t| t.song_id.clone())
            .unwrap_or_default();
        let to_song_id = prepared.song_id.clone();
        let preload_generation = prepared.preload_generation;

        self.current_track = Some(LoadedTrack {
            song_id: prepared.song_id,
            original_audio: prepared.audio,
            stems: None,
            is_playing: true,
            render_frame: incoming_frame_offset,
            streaming: None,
        });

        self.fade = FadeState::None;
        self.is_buffering = false;

        // Bump the transport generation so the frontend's stale-event
        // filter rejects any delayed `playback-position` event from the old
        // song. Without this bump, a same-generation position event for song-a
        // could arrive after the new-song snapshot and be accepted, reverting
        // the clock and queue reconciliation back to song-a. This mirrors the
        // gapless swap path (`perform_gapless_swap`).
        self.bump_transport_generation();

        self.stamp_transition(from_song_id, to_song_id, preload_generation);
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

    /// Read-only access to the current position for the reconnect path
    /// (PR #7). Mirrors [`position_ms`](Self::position_ms) but is `pub(crate)`
    /// so `services::playback` can capture the timeline before a source swap.
    pub(crate) fn position_ms_for_reconnect(&self) -> u64 {
        self.position_ms()
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

#[derive(Debug, Clone, Serialize)]
pub struct TrackTransitionedEvent {
    pub transition_serial: u64,
    pub from_song_id: String,
    pub to_song_id: String,
    pub state: PlaybackStateSnapshot,
}

/// Payload for the `remote-playback-reconnect` event (PR #7, issue #151).
/// Emitted before each re-resolve attempt so PR #8 can render a
/// "reconnecting…" state.
#[derive(Debug, Clone, Serialize)]
pub struct RemotePlaybackReconnectEvent {
    pub song_id: String,
    pub request_id: u64,
    pub attempt: u32,
    pub max_attempts: u32,
    pub reason: String,
}

/// Payload for the `remote-playback-resync` event (PR #7, issue #151).
/// Emitted when a reconnected source could not seek to the exact preserved
/// position and snapped to a preceding resumable boundary.
/// `actual_position_ms` is always `<= requested_position_ms`.
#[derive(Debug, Clone, Serialize)]
pub struct RemotePlaybackResyncEvent {
    pub song_id: String,
    pub requested_position_ms: u64,
    pub actual_position_ms: u64,
}

/// Payload for the `remote-playback-failed` event (PR #7, issue #151).
/// Emitted after the reconnect attempt budget is exhausted or a permanent
/// error occurs.
#[derive(Debug, Clone, Serialize)]
pub struct RemotePlaybackFailedEvent {
    pub song_id: String,
    pub request_id: u64,
    pub reason: String,
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
    fn invalidate_songs_clears_matching_current_and_loading() {
        let mut controller = super::PlaybackController::default();
        let decoded = crate::audio::decode::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 1_000,
            samples: vec![0.0; 100],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        assert_eq!(controller.current_song_id(), Some("song-a"));

        assert!(controller.invalidate_songs(&[String::from("song-a")]));
        assert!(controller.current_song_id().is_none());

        controller.start_track_loading("song-b");
        assert_eq!(controller.loading_song_id(), Some("song-b"));
        assert!(controller.invalidate_songs(&[String::from("song-b")]));
        assert!(controller.loading_song_id().is_none());
        assert!(!controller.cancel_loading_if_matching("song-b"));
    }

    #[test]
    fn invalidate_songs_ignores_unrelated_ids() {
        let mut controller = super::PlaybackController::default();
        let decoded = crate::audio::decode::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 1_000,
            samples: vec![0.0; 100],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        assert!(!controller.invalidate_songs(&[String::from("other")]));
        assert_eq!(controller.current_song_id(), Some("song-a"));
    }

    #[test]
    fn position_ms_derives_from_render_frame_not_wall_clock() {
        use super::DecodedAudio;

        let mut controller = super::PlaybackController::default();
        let decoded = DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 100);
        controller.play(100).unwrap();

        controller.advance_render_frame(44_100);

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

        let snap = controller.snapshot();
        assert_eq!(snap.state, "playing");
        assert!(snap.is_playing);

        controller.is_buffering = true;
        let snap = controller.snapshot();
        assert_eq!(snap.state, "buffering");
        assert!(snap.is_playing);

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

        controller.fade = FadeState::None;

        controller.seek(2_000, 0).unwrap();
        assert!(
            matches!(controller.fade, FadeState::FadingAfterSeek { .. }),
            "seek should activate FadingAfterSeek, got {:?}",
            controller.fade
        );

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

        controller.play(0).unwrap();
        assert!(matches!(controller.fade, FadeState::FadingIn { .. }));

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

        let snap = controller.snapshot();
        assert_eq!(snap.buffered_ms, 0);

        let decoded = DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);

        let snap = controller.snapshot();
        assert_eq!(snap.buffered_ms, 5_000);
        assert_eq!(snap.buffered_ms, snap.duration_ms.unwrap());
    }

    fn make_prepared(
        song_id: &str,
        preload_request_generation: u64,
        output_format: super::OutputFormatSnapshot,
    ) -> super::PreparedTrack {
        super::PreparedTrack {
            preload_request_generation: super::PreloadRequestGeneration(preload_request_generation),
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

        assert!(!controller.cancel_prepared_track(super::PreloadRequestGeneration(1)));

        let stale = make_prepared("song-old", 0, fmt);
        assert!(controller.install_prepared_track(stale, fmt).is_err());
        assert!(controller.prepared_track.is_none());

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
        let decoded = super::DecodedAudio {
            sample_rate: 48_000,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 48_000 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);

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

        let prepared = make_prepared("song-b", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());
        assert!(controller.prepared_track.is_some());

        // stamp expected generation to 1.
        assert!(controller.cancel_prepared_track(super::PreloadRequestGeneration(1)));
        assert!(controller.prepared_track.is_none());
        assert_eq!(
            controller.expected_preload_request_generation,
            super::PreloadRequestGeneration(1)
        );
    }

    #[test]
    fn perform_gapless_swap_clears_fade_and_buffering() {
        let mut controller = super::PlaybackController::default();
        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);

        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        controller.play(0).unwrap();
        controller.pause(0).unwrap();
        assert!(matches!(
            controller.fade,
            super::FadeState::FadingOut { .. }
        ));
        controller.is_buffering = true;

        let prepared = make_prepared("song-b", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());

        assert!(controller.perform_gapless_swap());

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
        // A gapless swap replaces song-a with song-b but must bump the
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

        let prepared = make_prepared("song-b", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());
        assert!(controller.prepared_track.is_some());

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
        // After pausing (fade-out) and then resuming (play), the
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

        controller.pause(0).unwrap();
        assert!(!controller.current_track_is_playing());

        // must return true (FadingIn is not FadingOut).
        controller.play(0).unwrap();
        assert!(
            controller.current_track_is_playing(),
            "should be playing after resume (FadingIn is not FadingOut)"
        );
    }

    // Transport commands issued while a track is loading (current_track
    // is None but loading_song_id is Some) must be benign no-ops returning the
    // loading snapshot, not user-visible errors. The error remains for the
    // truly-idle case to surface real caller bugs.
    #[test]
    fn play_during_loading_is_no_op_returning_loading_snapshot() {
        let mut controller = super::PlaybackController::default();
        let loading = controller.start_track_loading("song-a");
        let generation_before = loading.transport_generation;

        let result = controller.play(0).expect("loading no-op should be Ok");
        assert_eq!(result.song_id.as_deref(), Some("song-a"));
        assert_eq!(result.state, "loading");
        assert!(!result.is_playing);
        assert_eq!(result.transport_generation, generation_before);
    }

    #[test]
    fn pause_during_loading_is_no_op_returning_loading_snapshot() {
        let mut controller = super::PlaybackController::default();
        let loading = controller.start_track_loading("song-a");
        let generation_before = loading.transport_generation;

        let result = controller.pause(0).expect("loading no-op should be Ok");
        assert_eq!(result.song_id.as_deref(), Some("song-a"));
        assert_eq!(result.state, "loading");
        assert_eq!(result.transport_generation, generation_before);
    }

    #[test]
    fn seek_during_loading_is_no_op_returning_loading_snapshot() {
        let mut controller = super::PlaybackController::default();
        let loading = controller.start_track_loading("song-a");
        let generation_before = loading.transport_generation;

        let result = controller
            .seek(5_000, 0)
            .expect("loading no-op should be Ok");
        assert_eq!(result.song_id.as_deref(), Some("song-a"));
        assert_eq!(result.state, "loading");
        assert_eq!(result.position_ms, 0);
        assert_eq!(result.transport_generation, generation_before);
    }

    #[test]
    fn play_pause_seek_when_truly_idle_still_error() {
        let mut controller = super::PlaybackController::default();
        assert!(controller.play(0).is_err());
        assert!(controller.pause(0).is_err());
        assert!(controller.seek(0, 0).is_err());
    }

    // rewrote the crossfade implementation but shipped with fewer
    // state-machine regression tests than the original #89 branch. The tests
    // below cover seek-abort, pause-preserve, manual-load cancellation,
    // stem-attach ownership guards, and promotion invariants.

    fn make_decoded() -> super::DecodedAudio {
        super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        }
    }

    fn make_crossfade_active(song_id: &str, total_frames: u64) -> super::ActiveCrossfade {
        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);
        super::ActiveCrossfade {
            prepared: make_prepared(song_id, 0, fmt),
            total_frames,
            rendered_frames: 0,
            incoming_source_frame: 0,
        }
    }

    fn dummy_two_stem() -> super::LoadedStems {
        let audio = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 1_000,
            samples: vec![0.0; 44_100 * 2],
        };
        super::LoadedStems::TwoStem {
            vocals: audio.clone(),
            accompaniment: audio,
        }
    }

    #[test]
    fn seek_aborts_active_crossfade_and_restores_prepared_track() {
        let mut controller = super::PlaybackController::default();
        controller.start_track("song-a".to_owned(), make_decoded(), 0);
        controller.play(0).unwrap();

        controller.active_crossfade = Some(make_crossfade_active("song-b", 44_100));
        assert!(controller.active_crossfade.is_some());
        assert!(controller.prepared_track.is_none());

        // Seek aborts the crossfade and restores the prepared track.
        controller.seek(1_000, 0).unwrap();

        assert!(
            controller.active_crossfade.is_none(),
            "seek must abort active crossfade"
        );
        assert!(
            controller.prepared_track.is_some(),
            "seek must restore the incoming payload to prepared_track"
        );
        let prepared = controller.prepared_track.as_ref().unwrap();
        assert_eq!(prepared.song_id, "song-b");
    }

    #[test]
    fn pause_preserves_active_crossfade() {
        let mut controller = super::PlaybackController::default();
        controller.start_track("song-a".to_owned(), make_decoded(), 0);
        controller.play(0).unwrap();

        let mut active = make_crossfade_active("song-b", 44_100);
        active.rendered_frames = 22_050;
        controller.active_crossfade = Some(active);

        controller.pause(0).unwrap();

        assert!(
            controller.active_crossfade.is_some(),
            "pause must preserve active crossfade state"
        );
        let active = controller.active_crossfade.as_ref().unwrap();
        assert_eq!(
            active.rendered_frames, 22_050,
            "pause must not reset crossfade progress"
        );
    }

    #[test]
    fn start_track_cancels_active_crossfade() {
        // A manual track load during an active crossfade must cancel both
        // the active crossfade and the prepared track. A stale crossfade
        // must not mix the new track against the old prepared payload on
        // the next callback.
        let mut controller = super::PlaybackController::default();
        controller.start_track("song-a".to_owned(), make_decoded(), 0);
        controller.play(0).unwrap();

        controller.active_crossfade = Some(make_crossfade_active("song-b", 44_100));
        controller.prepared_track = None; // consumed by the active crossfade

        controller.start_track("song-c".to_owned(), make_decoded(), 0);

        assert!(
            controller.active_crossfade.is_none(),
            "start_track must cancel active crossfade"
        );
        assert!(
            controller.prepared_track.is_none(),
            "start_track must cancel prepared track"
        );
        assert_eq!(
            controller.current_track.as_ref().unwrap().song_id,
            "song-c",
            "current track must be the newly loaded track"
        );
    }

    #[test]
    fn start_track_loading_cancels_active_crossfade() {
        // A new load request must cancel an active crossfade — the incoming
        // track is not the prepared one, and a stale crossfade must not mix
        // against the about-to-be-replaced current track.
        let mut controller = super::PlaybackController::default();
        controller.start_track("song-a".to_owned(), make_decoded(), 0);
        controller.play(0).unwrap();

        controller.active_crossfade = Some(make_crossfade_active("song-b", 44_100));
        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);
        let prepared = make_prepared("song-d", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());
        controller.stamp_transition("song-a".to_owned(), "song-d".to_owned(), 0);

        controller.start_track_loading("song-c");

        assert!(
            controller.active_crossfade.is_none(),
            "start_track_loading must cancel active crossfade"
        );
        assert!(
            controller.prepared_track.is_none(),
            "start_track_loading must cancel prepared track"
        );
        assert_eq!(
            controller.loading_song_id.as_deref(),
            Some("song-c"),
            "loading song must be set"
        );
        assert!(
            controller.pending_transition_out.is_none(),
            "start_track_loading must clear pending transition to prevent stale queue reconciliation"
        );
    }

    #[test]
    fn start_track_streaming_cancels_active_crossfade() {
        use crate::audio::streaming::{self, StreamingTrack};

        let mut controller = super::PlaybackController::default();
        controller.start_track("song-a".to_owned(), make_decoded(), 0);
        controller.play(0).unwrap();

        controller.active_crossfade = Some(make_crossfade_active("song-b", 44_100));

        let (_prod, consumer) = streaming::create_stream_pair(44_100, 2);
        controller.start_track_streaming(
            "song-c".to_owned(),
            44_100,
            2,
            5_000,
            StreamingTrack::Single { consumer },
            0,
        );

        assert!(
            controller.active_crossfade.is_none(),
            "start_track_streaming must cancel active crossfade"
        );
        assert_eq!(
            controller.current_track.as_ref().unwrap().song_id,
            "song-c",
            "current track must be the newly loaded streaming track"
        );
    }

    #[test]
    fn attach_stems_same_song_cancels_crossfade_and_installs() {
        let mut controller = super::PlaybackController::default();
        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);
        controller.active_crossfade = Some(make_crossfade_active("song-b", 44_100));

        controller
            .attach_stems("song-a", dummy_two_stem())
            .expect("same-song attach must succeed");

        assert!(
            controller.active_crossfade.is_none(),
            "successful attach cancels active crossfade"
        );
        assert!(
            controller.current_track.as_ref().unwrap().stems.is_some(),
            "successful attach installs stems"
        );
    }

    #[test]
    fn attach_stems_wrong_song_preserves_active_crossfade_and_prepared() {
        let mut controller = super::PlaybackController::default();
        let decoded = super::DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        };
        controller.start_track("song-a".to_owned(), decoded, 0);

        let fmt = super::OutputFormatSnapshot::new(1, 44_100, 2);
        let prepared = make_prepared("song-b", 0, fmt);
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());
        controller.active_crossfade = Some(make_crossfade_active("song-c", 44_100));
        let prepared_again = make_prepared("song-d", 0, fmt);
        assert!(controller
            .install_prepared_track(prepared_again, fmt)
            .is_ok());

        let _err = controller
            .attach_stems("other-song", dummy_two_stem())
            .expect_err("mismatched song must reject");

        assert!(
            controller.active_crossfade.is_some(),
            "rejected attach must not destroy active crossfade"
        );
        assert!(
            controller.prepared_track.is_some(),
            "rejected attach must not destroy prepared track"
        );
        assert!(
            controller.current_track.as_ref().unwrap().stems.is_none(),
            "rejected attach must not install stems"
        );
    }

    #[test]
    fn promote_crossfade_track_stamps_transition_and_promotes_incoming() {
        let mut controller = super::PlaybackController::default();
        controller.start_track("song-a".to_owned(), make_decoded(), 0);
        controller.play(0).unwrap();

        let initial_serial = controller.transition_serial;
        let prepared = make_prepared("song-b", 0, super::OutputFormatSnapshot::new(1, 44_100, 2));

        controller.promote_crossfade_track(prepared, 44_100);

        let track = controller.current_track.as_ref().unwrap();
        assert_eq!(track.song_id, "song-b");
        assert_eq!(track.render_frame, 44_100);

        assert!(controller.transition_serial > initial_serial);
        let pending = controller.pending_transition_out.as_ref().unwrap();
        assert_eq!(pending.from_song_id, "song-a");
        assert_eq!(pending.to_song_id, "song-b");
    }

    #[test]
    fn promote_crossfade_track_bumps_transport_generation() {
        // A crossfade promotion replaces song-a with song-b but must bump the
        // transport generation so the frontend's stale-event filter rejects
        // delayed `playback-position` events from song-a. Without the bump, a
        // same-generation position event for the old song could arrive after
        // the new-song snapshot and be accepted, reverting the clock and
        // queue reconciliation back to song-a.
        let mut controller = super::PlaybackController::default();
        controller.start_track("song-a".to_owned(), make_decoded(), 0);
        controller.play(0).unwrap();
        let gen_before = controller.transport_generation;

        let prepared = make_prepared("song-b", 0, super::OutputFormatSnapshot::new(1, 44_100, 2));
        controller.promote_crossfade_track(prepared, 0);

        assert!(
            controller.transport_generation > gen_before,
            "crossfade promotion must bump transport_generation (was {gen_before}, is {})",
            controller.transport_generation
        );
    }

    #[test]
    fn promote_crossfade_track_clears_fade_and_buffering() {
        let mut controller = super::PlaybackController::default();
        controller.start_track("song-a".to_owned(), make_decoded(), 0);
        controller.play(0).unwrap();

        controller.fade = super::FadeState::FadingIn {
            start: std::time::Instant::now(),
        };
        controller.is_buffering = true;

        let prepared = make_prepared("song-b", 0, super::OutputFormatSnapshot::new(1, 44_100, 2));
        controller.promote_crossfade_track(prepared, 0);

        assert!(
            matches!(controller.fade, super::FadeState::None),
            "promote must clear fade"
        );
        assert!(!controller.is_buffering, "promote must clear buffering");
    }
}
