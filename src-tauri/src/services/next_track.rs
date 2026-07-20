use crate::{
    audio::{
        coordinator::PlaybackCommand, error::PlaybackError, output_format::OutputFormatSnapshot,
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

/// If the sample rate or channel count already matches, the audio is returned
/// unchanged. Otherwise a simple linear resample / channel remap is applied.
///
/// The preload scheduler uses this so the prepared track's PCM exactly
/// matches what the render callback expects, avoiding a resampler cache
/// miss on the first gapless frame.
fn normalize_to_output_format(
    mut audio: crate::audio::decode::DecodedAudio,
    target_sample_rate: u32,
    target_channels: usize,
) -> crate::audio::decode::DecodedAudio {
    if audio.channels != target_channels {
        audio.samples = remap_channels(
            &audio.samples,
            audio.channels,
            target_channels,
            audio.samples.len() / audio.channels.max(1),
        );
        audio.channels = target_channels;
    }

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

    audio
}

/// Remap interleaved samples from `src_channels` to `dst_channels`.
fn remap_channels(
    samples: &[f32],
    src_channels: usize,
    dst_channels: usize,
    frames: usize,
) -> Vec<f32> {
    if src_channels == 0 || dst_channels == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(frames * dst_channels);
    for frame in 0..frames {
        let src_base = frame * src_channels;
        match (src_channels, dst_channels) {
            (1, 2) => {
                let s = samples[src_base];
                out.push(s);
                out.push(s);
            }
            (2, 1) => {
                let l = samples[src_base];
                let r = samples[src_base + 1];
                out.push((l + r) * 0.5);
            }
            _ => {
                for dst_ch in 0..dst_channels {
                    let src_ch = if dst_ch < src_channels {
                        dst_ch
                    } else {
                        dst_ch % src_channels
                    };
                    out.push(samples[src_base + src_ch]);
                }
            }
        }
    }
    out
}

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

/// Only local, non-streaming, non-Media+G songs are eligible — the preload
/// scheduler fully decodes the audio into memory so it must be a format that
/// `load_playback_source` can decode without streaming.
fn is_eligible_for_gapless(song: &Song) -> bool {
    if song.is_media_g() {
        return false;
    }
    if song.is_remote() {
        return false;
    }
    true
}

fn prepare_next_track(
    app_data_dir: &Path,
    connection: &Connection,
    library_root: &LibraryRoot,
    song: &Song,
    output_format: OutputFormatSnapshot,
    preload_request_generation: crate::audio::playback::PreloadRequestGeneration,
) -> Result<PreparedTrack, PlaybackError> {
    if !is_eligible_for_gapless(song) {
        return Err(PlaybackError::Internal(
            "song is not eligible for gapless preload".to_owned(),
        ));
    }

    let load =
        playback_source::load_playback_source(Some(app_data_dir), connection, library_root, song)?;

    let normalized = normalize_to_output_format(
        load.decoded_audio,
        output_format.sample_rate,
        output_format.channels as usize,
    );

    Ok(PreparedTrack {
        preload_request_generation,
        preload_generation: output_format.generation,
        song_id: song.hash.clone(),
        output_format,
        audio: normalized,
    })
}

/// The thread checks `shutdown` before decoding and before sending; if a
/// newer preload is requested the old thread bails out.
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
    preload_request_generation: crate::audio::playback::PreloadRequestGeneration,
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
        let output_format = match state
            .playback
            .output_format
            .read()
            .ok()
            .and_then(|guard| *guard)
        {
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
        let normalized = normalize_to_output_format(audio.clone(), 44_100, 2);
        assert_eq!(normalized.sample_rate, 44_100);
        assert_eq!(normalized.channels, 2);
        assert_eq!(normalized.samples.len(), audio.samples.len());
    }

    #[test]
    fn normalize_remaps_mono_to_stereo() {
        let audio = make_audio(44_100, 1, 100);
        let normalized = normalize_to_output_format(audio, 44_100, 2);
        assert_eq!(normalized.channels, 2);
        assert_eq!(normalized.samples.len(), 200);
        for i in 0..100 {
            assert_eq!(normalized.samples[i * 2], normalized.samples[i * 2 + 1]);
        }
    }

    #[test]
    fn normalize_remaps_stereo_to_mono() {
        let audio = make_audio(44_100, 2, 100);
        let normalized = normalize_to_output_format(audio, 44_100, 1);
        assert_eq!(normalized.channels, 1);
        assert_eq!(normalized.samples.len(), 100);
    }

    #[test]
    fn normalize_resamples_44100_to_48000() {
        let audio = make_audio(44_100, 2, 4410); // 0.1s at 44.1kHz
        let normalized = normalize_to_output_format(audio, 48_000, 2);
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
        let normalized = normalize_to_output_format(audio, 48_000, 2);
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
        let out = remap_channels(&samples, 1, 2, 3);
        assert_eq!(out.len(), 6);
        assert_eq!(out, vec![0.1, 0.1, 0.2, 0.2, 0.3, 0.3]);
    }

    #[test]
    fn remap_channels_stereo_to_mono_halves_length() {
        let samples = vec![0.2_f32, 0.4, 0.6, 0.8]; // 2 stereo frames
        let out = remap_channels(&samples, 2, 1, 2);
        assert_eq!(out.len(), 2);
        // (0.2+0.4)*0.5 and (0.6+0.8)*0.5 — use approximate comparison
        // because f32 arithmetic does not produce exactly 0.3 / 0.7.
        assert!((out[0] - 0.3).abs() < 1e-5);
        assert!((out[1] - 0.7).abs() < 1e-5);
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
        let normalized = normalize_to_output_format(audio, 44_100, 2);
        assert_eq!(normalized.sample_rate, 44_100);
    }
}
