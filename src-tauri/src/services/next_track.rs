//! #88: NextTrack preload scheduler.
//!
//! Decodes the next queued song off-thread, normalizes the PCM to the active
//! output format (sample rate + channel count), and sends a `PrepareNext`
//! command to the coordinator. The coordinator validates the output-format
//! generation before installing; if the device restarted or the format
//! changed, the prepared payload is silently dropped and the scheduler does
//! not retry — the next `set_preload_candidate` call will re-prepare with the
//! new format.
//!
//! Eligibility: gapless preload is only attempted for local, non-streaming,
//! non-Media+G songs whose source format matches the current track's format.
//! Remote songs, Media+G containers, and streaming tracks fall back to the
//! normal `play()` path (the frontend will call `play()` when
//! `track-transitioned` does not arrive).

use crate::{
    audio::{
        coordinator::PlaybackCommand,
        error::PlaybackError,
        output_format::{self, OutputFormatSnapshot},
        playback::PreparedTrack,
    },
    cache,
    library::Song,
    library_root::LibraryRoot,
    services::playback_source,
    state::AppState,
};
use rusqlite::Connection;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// Normalize decoded audio to the target output format. If the sample rate or
/// channel count already matches, the audio is returned unchanged. Otherwise
/// a simple linear resample / channel remap is applied.
///
/// The preload scheduler uses this so the prepared track's PCM exactly
/// matches what the render callback expects, avoiding a resampler cache
/// miss on the first gapless frame.
///
/// Returns `None` if the channel layout is unsupported per the #88 rules:
/// matching, mono→N, N→mono (N≥2), and stereo→N (N>2) are supported; any
/// other layout (e.g. 3→5, 5→2) returns `None` so the caller falls back.
fn normalize_to_output_format(
    mut audio: crate::audio::decode::DecodedAudio,
    target_sample_rate: u32,
    target_channels: usize,
) -> Option<crate::audio::decode::DecodedAudio> {
    // Channel remap if needed.
    if audio.channels != target_channels {
        let frames = audio.samples.len() / audio.channels.max(1);
        audio.samples = remap_channels(&audio.samples, audio.channels, target_channels, frames)?;
        audio.channels = target_channels;
    }

    // Sample-rate conversion if needed (linear interpolation).
    if audio.sample_rate != target_sample_rate {
        audio.samples = linear_resample(
            &audio.samples,
            audio.sample_rate,
            target_sample_rate,
            audio.channels,
        );
        audio.sample_rate = target_sample_rate;
    }

    // Recompute duration_ms from the normalized samples. Guard against
    // division by zero — a valid decoded audio should always have a non-zero
    // sample rate, but the DecodedAudio struct does not enforce this
    // invariant at construction time.
    if audio.sample_rate > 0 {
        if let Some(frames) = audio.samples.len().checked_div(audio.channels) {
            audio.duration_ms = (frames as u64 * 1000) / audio.sample_rate as u64;
        }
    }

    Some(audio)
}

/// Remap interleaved samples from `src_channels` to `dst_channels` following
/// the deterministic #88 channel mapping rules:
/// - matching channel count: preserve channels (caller skips this case)
/// - mono to two or more: duplicate mono to every output channel
/// - two or more to mono: average channels 0 and 1 with 0.5 each
/// - stereo to more than two: left/right to 0/1 and zeros to remaining
/// - any unsupported layout returns `None`
fn remap_channels(
    samples: &[f32],
    src_channels: usize,
    dst_channels: usize,
    frames: usize,
) -> Option<Vec<f32>> {
    if src_channels == 0 || dst_channels == 0 {
        return None;
    }
    let mut out = Vec::with_capacity(frames * dst_channels);
    for frame in 0..frames {
        let src_base = frame * src_channels;
        match (src_channels, dst_channels) {
            (1, _) => {
                // mono to two or more: duplicate mono to every output channel
                let s = samples[src_base];
                for _ in 0..dst_channels {
                    out.push(s);
                }
            }
            (_, 1) if src_channels >= 2 => {
                // two or more to mono: average channels 0 and 1 with 0.5 each
                let l = samples[src_base];
                let r = samples[src_base + 1];
                out.push((l + r) * 0.5);
            }
            (2, _) if dst_channels > 2 => {
                // stereo to more than two: left/right to 0/1, zeros to rest
                out.push(samples[src_base]);
                out.push(samples[src_base + 1]);
                out.extend(std::iter::repeat_n(0.0, dst_channels - 2));
            }
            // matching or unsupported layouts should not reach here (caller
            // skips matching; unsupported returns None)
            _ => return None,
        }
    }
    Some(out)
}

/// Linear-interpolation resampling for interleaved samples.
fn linear_resample(samples: &[f32], src_rate: u32, dst_rate: u32, channels: usize) -> Vec<f32> {
    if src_rate == dst_rate || channels == 0 || samples.is_empty() {
        return samples.to_vec();
    }
    // Guard against zero rates — would produce a zero or infinite ratio and
    // a division by zero in the frame count computation below.
    if src_rate == 0 || dst_rate == 0 {
        return samples.to_vec();
    }
    let src_frames = samples.len() / channels;
    let ratio = src_rate as f64 / dst_rate as f64;
    let dst_frames = (src_frames as f64 / ratio).round() as usize;
    let mut out = Vec::with_capacity(dst_frames * channels);

    for dst_frame in 0..dst_frames {
        let src_pos = dst_frame as f64 * ratio;
        let src_idx = src_pos as usize;
        let frac = (src_pos - src_idx as f64) as f32;
        let can_interp = src_idx + 1 < src_frames;
        for ch in 0..channels {
            let s0 = samples[src_idx * channels + ch];
            let s1 = if can_interp {
                samples[(src_idx + 1) * channels + ch]
            } else {
                s0
            };
            out.push(s0 + (s1 - s0) * frac);
        }
    }
    out
}

/// Maximum decoded PCM size (bytes) for gapless preparation. 512 MiB
/// covers ~50 minutes of stereo 96 kHz / ~3 hours of stereo 44.1 kHz.
/// Tracks exceeding this cap fall back to the normal `play()` path.
const PREPARATION_CAP_BYTES: usize = 512 * 1024 * 1024;

/// Check whether a song is eligible for gapless preload. Only local,
/// non-streaming, non-Media+G songs are eligible — the preload scheduler
/// fully decodes the audio into memory so it must be a format that
/// `load_playback_source` can decode without streaming.
fn is_eligible_for_gapless(song: &Song) -> bool {
    // Media+G containers need special handling (ZIP extraction) and are
    // not eligible for the simple decode path.
    if song.is_media_g() {
        return false;
    }
    // Remote songs use streaming/byte-range playback; gapless preload
    // requires full decode into memory which defeats the low-latency
    // streaming design.
    if song.is_remote() {
        return false;
    }
    true
}

/// Decode and normalize the next track for gapless transition. Called on a
/// background thread. Returns `Some(PreparedTrack)` on success, or `None`
/// if the song is not eligible or decoding failed.
fn prepare_next_track(
    app_data_dir: &Path,
    connection: &Connection,
    library_root: &LibraryRoot,
    song: &Song,
    output_format: OutputFormatSnapshot,
    preload_request_generation: u64,
) -> Result<PreparedTrack, PlaybackError> {
    if !is_eligible_for_gapless(song) {
        return Err(PlaybackError::Internal(
            "song is not eligible for gapless preload".to_owned(),
        ));
    }

    let load =
        playback_source::load_playback_source(Some(app_data_dir), connection, library_root, song)?;

    // #88: Reject oversized PCM — decoded audio plus metadata must fit the
    // 512 MiB preparation cap. `samples.len() * size_of::<f32>()` is the
    // dominant memory cost; metadata is negligible by comparison.
    let pcm_bytes = load.decoded_audio.samples.len() * std::mem::size_of::<f32>();
    if pcm_bytes > PREPARATION_CAP_BYTES {
        return Err(PlaybackError::Internal(format!(
            "decoded PCM ({} bytes) exceeds 512 MiB preparation cap",
            pcm_bytes
        )));
    }

    // Normalize the decoded audio to the output format. Stems are not
    // preloaded for gapless — the new track starts in base-audio mode and
    // the frontend can call `load_stems()` after the transition.
    let normalized = normalize_to_output_format(
        load.decoded_audio,
        output_format.sample_rate,
        output_format.channels as usize,
    )
    .ok_or_else(|| {
        PlaybackError::Internal("unsupported channel layout for gapless preload".to_owned())
    })?;

    Ok(PreparedTrack {
        preload_request_generation,
        preload_generation: output_format.generation,
        song_id: song.hash.clone(),
        output_format,
        audio: normalized,
    })
}

/// Spawn a background thread to preload the next track and send a
/// `PrepareNext` command to the coordinator. The thread checks `shutdown`
/// before decoding and before sending; if a newer preload is requested the
/// old thread bails out.
///
/// `preload_request_generation` is the monotonic generation of the
/// `set_preload_candidate` call that initiated this preload. It is included
/// in the `PrepareNext` command so the coordinator can reject stale preloads
/// from older threads that raced with a newer cancel.
///
/// Returns immediately; the caller does not wait for the decode to finish.
pub fn spawn_preload_next(
    state: AppState,
    app_data_dir: std::path::PathBuf,
    song_id: String,
    shutdown: Arc<AtomicBool>,
    preload_request_generation: u64,
) {
    std::thread::spawn(move || {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }

        let library_root = match state.shell.library_root() {
            Ok(root) => root,
            Err(_) => return,
        };

        let connection = match cache::open_database(&library_root.database_path()) {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("next_track: failed to open database: {e}");
                return;
            }
        };

        let song = match cache::get_song_by_hash(&connection, &song_id) {
            Ok(Some(song)) => song,
            Ok(None) => {
                eprintln!("next_track: song not found: {song_id}");
                return;
            }
            Err(e) => {
                eprintln!("next_track: failed to get song: {e}");
                return;
            }
        };

        if !is_eligible_for_gapless(&song) {
            // Not eligible — silently skip. The frontend will fall back to
            // calling `play()` when `track-transitioned` does not arrive.
            return;
        }

        if shutdown.load(Ordering::Relaxed) {
            return;
        }

        // Capture the current output format. If no output stream has been
        // constructed yet, we cannot preload.
        let output_format = match output_format::snapshot(&state.playback.output_format) {
            Some(fmt) => fmt,
            None => return,
        };

        let prepared = match prepare_next_track(
            &app_data_dir,
            &connection,
            &library_root,
            &song,
            output_format,
            preload_request_generation,
        ) {
            Ok(prepared) => prepared,
            Err(e) => {
                eprintln!("next_track: failed to prepare {}: {e}", song.hash);
                return;
            }
        };

        if shutdown.load(Ordering::Relaxed) {
            return;
        }

        // Send PrepareNext to the coordinator (fire-and-forget).
        let _ = state
            .playback
            .command_tx
            .send(PlaybackCommand::PrepareNext {
                prepared: Box::new(prepared),
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_audio(
        sample_rate: u32,
        channels: usize,
        frames: usize,
    ) -> crate::audio::decode::DecodedAudio {
        let samples: Vec<f32> = (0..frames * channels).map(|i| (i as f32) / 100.0).collect();
        let duration_ms = if sample_rate > 0 {
            (frames as u64 * 1000) / sample_rate as u64
        } else {
            0
        };
        crate::audio::decode::DecodedAudio {
            sample_rate,
            channels,
            duration_ms,
            samples,
        }
    }

    #[test]
    fn normalize_same_format_returns_unchanged() {
        let audio = make_audio(44_100, 2, 1000);
        let normalized = normalize_to_output_format(audio.clone(), 44_100, 2).unwrap();
        assert_eq!(normalized.sample_rate, 44_100);
        assert_eq!(normalized.channels, 2);
        // Samples should be identical (no resampling needed).
        assert_eq!(normalized.samples.len(), audio.samples.len());
    }

    #[test]
    fn normalize_remaps_mono_to_stereo() {
        let audio = make_audio(44_100, 1, 100);
        let normalized = normalize_to_output_format(audio, 44_100, 2).unwrap();
        assert_eq!(normalized.channels, 2);
        assert_eq!(normalized.samples.len(), 200);
        // Each mono sample should be duplicated.
        for i in 0..100 {
            assert_eq!(normalized.samples[i * 2], normalized.samples[i * 2 + 1]);
        }
    }

    #[test]
    fn normalize_remaps_stereo_to_mono() {
        let audio = make_audio(44_100, 2, 100);
        let normalized = normalize_to_output_format(audio, 44_100, 1).unwrap();
        assert_eq!(normalized.channels, 1);
        assert_eq!(normalized.samples.len(), 100);
    }

    #[test]
    fn normalize_resamples_44100_to_48000() {
        let audio = make_audio(44_100, 2, 4410); // 0.1s at 44.1kHz
        let normalized = normalize_to_output_format(audio, 48_000, 2).unwrap();
        assert_eq!(normalized.sample_rate, 48_000);
        assert_eq!(normalized.channels, 2);
        // 0.1s at 48kHz = 4800 frames
        let expected_frames = 4800;
        let actual_frames = normalized.samples.len() / 2;
        assert!(
            actual_frames.abs_diff(expected_frames) <= 2,
            "expected ~{expected_frames} frames, got {actual_frames}"
        );
    }

    #[test]
    fn normalize_recomputes_duration_ms() {
        let audio = make_audio(44_100, 2, 44_100); // 1s
        let normalized = normalize_to_output_format(audio, 48_000, 2).unwrap();
        // Duration should be ~1000ms after resampling.
        assert!(
            (normalized.duration_ms as i64 - 1000).abs() <= 2,
            "expected ~1000ms, got {}",
            normalized.duration_ms
        );
    }

    #[test]
    fn is_eligible_for_gapless_rejects_media_g() {
        let mut song = Song {
            hash: "abc".to_owned(),
            file_path: Some("test.mp3".to_owned()),
            cdg_path: None,
            media_g_container: Some("paired".to_owned()),
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: None,
            artist: None,
            album: None,
            duration_ms: 1000,
            cover_art: None,
            has_cover_art: false,
            imported_at: 0,
            original_ext: Some("mp3".to_owned()),
        };
        assert!(!is_eligible_for_gapless(&song));
        song.media_g_container = None;
        assert!(is_eligible_for_gapless(&song));
    }

    #[test]
    fn is_eligible_for_gapless_rejects_remote() {
        let song = Song {
            hash: "abc".to_owned(),
            file_path: Some("test.mp3".to_owned()),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original_remote".to_owned(),
            title: None,
            artist: None,
            album: None,
            duration_ms: 1000,
            cover_art: None,
            has_cover_art: false,
            imported_at: 0,
            original_ext: Some("mp3".to_owned()),
        };
        assert!(!is_eligible_for_gapless(&song));
    }

    #[test]
    fn linear_resample_preserves_silence() {
        let samples = vec![0.0_f32; 4410 * 2]; // 0.1s of silence, stereo
        let out = linear_resample(&samples, 44_100, 48_000, 2);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn remap_channels_mono_to_stereo_doubles_length() {
        let samples = vec![0.1, 0.2, 0.3]; // 3 mono frames
        let out = remap_channels(&samples, 1, 2, 3).unwrap();
        assert_eq!(out.len(), 6);
        assert_eq!(out, vec![0.1, 0.1, 0.2, 0.2, 0.3, 0.3]);
    }

    #[test]
    fn remap_channels_stereo_to_mono_halves_length() {
        let samples = vec![0.2_f32, 0.4, 0.6, 0.8]; // 2 stereo frames
        let out = remap_channels(&samples, 2, 1, 2).unwrap();
        assert_eq!(out.len(), 2);
        // (0.2+0.4)*0.5 and (0.6+0.8)*0.5 — use approximate comparison
        // because f32 arithmetic does not produce exactly 0.3 / 0.7.
        assert!((out[0] - 0.3).abs() < 1e-5);
        assert!((out[1] - 0.7).abs() < 1e-5);
    }

    #[test]
    fn remap_channels_stereo_to_surround_pads_zeros() {
        // stereo to 4 channels: L/R to 0/1, zeros to 2/3
        let samples = vec![0.1_f32, 0.2, 0.3, 0.4]; // 2 stereo frames
        let out = remap_channels(&samples, 2, 4, 2).unwrap();
        assert_eq!(out.len(), 8);
        assert_eq!(out, vec![0.1, 0.2, 0.0, 0.0, 0.3, 0.4, 0.0, 0.0]);
    }

    #[test]
    fn remap_channels_mono_to_six_channels() {
        let samples = vec![0.5_f32, 0.7]; // 2 mono frames
        let out = remap_channels(&samples, 1, 6, 2).unwrap();
        assert_eq!(out.len(), 12);
        // Every channel should be the mono value
        assert!(out.iter().all(|&s| s == 0.5 || s == 0.7));
        assert_eq!(out[0], 0.5);
        assert_eq!(out[5], 0.5);
        assert_eq!(out[6], 0.7);
        assert_eq!(out[11], 0.7);
    }

    #[test]
    fn remap_channels_unsupported_layout_returns_none() {
        // 3-channel to 5-channel is not in the supported mapping rules
        let samples = vec![0.1_f32, 0.2, 0.3, 0.4, 0.5, 0.6]; // 2 frames × 3 ch
        let out = remap_channels(&samples, 3, 5, 2);
        assert!(out.is_none());
    }

    #[test]
    fn remap_channels_five_to_two_returns_none() {
        // 5-channel to 2-channel is not a supported mapping
        let samples = vec![0.1_f32; 10]; // 2 frames × 5 ch
        let out = remap_channels(&samples, 5, 2, 2);
        assert!(out.is_none());
    }

    #[test]
    fn linear_resample_zero_src_rate_returns_input_unchanged() {
        let samples = vec![0.1_f32, 0.2, 0.3, 0.4];
        let out = linear_resample(&samples, 0, 44_100, 2);
        assert_eq!(out, samples);
    }

    #[test]
    fn linear_resample_zero_dst_rate_returns_input_unchanged() {
        let samples = vec![0.1_f32, 0.2, 0.3, 0.4];
        let out = linear_resample(&samples, 44_100, 0, 2);
        assert_eq!(out, samples);
    }

    #[test]
    fn normalize_to_output_format_with_zero_sample_rate_does_not_panic() {
        let audio = make_audio(0, 2, 100);
        // Should not panic even though sample_rate is 0.
        let normalized = normalize_to_output_format(audio, 44_100, 2).unwrap();
        assert_eq!(normalized.sample_rate, 44_100);
    }
}
