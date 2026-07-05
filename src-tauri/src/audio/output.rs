use crate::airplay_stream::AirPlayAudioTap;
use crate::audio::decode::DecodedAudio;
use crate::audio::error::PlaybackError;
use crate::audio::playback::{LoadedStems, PlaybackController, StemVolumes};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, SizedSample, Stream};
use rubato::{Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType};
use std::collections::HashMap;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

/// Cache for rubato resamplers keyed by (src_rate, dst_rate, channel, output_chunk).
/// Resamplers maintain internal state (filter coefficients, buffer history)
/// so they must be reused across consecutive audio callbacks for the same rate
/// pair and output frame count. Pre-allocated scratch buffers are stored
/// alongside each resampler to avoid per-callback heap allocation on the
/// realtime audio thread.
#[derive(Default)]
pub struct ResamplerCache {
    cache: HashMap<(u32, u32, usize, usize), ResamplerEntry>,
}

/// A cached resampler plus reusable scratch buffers for its input/output.
struct ResamplerEntry {
    resampler: Async<f32>,
    /// Reusable planar input buffer (1 channel, resized per callback).
    channel_input: Vec<f32>,
    /// Reusable outer Vec for the SequentialSliceOfVecs adapter (always 1 element).
    input_vecs: Vec<Vec<f32>>,
}

impl ResamplerCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a resampler entry for the given rate pair, channel index,
    /// and output chunk size. Each channel needs its own resampler because
    /// rubato maintains per-channel filter state — sharing one resampler across
    /// channels produces phase-blurred output.
    ///
    /// The output chunk size is included in the key because `FixedAsync::Output`
    /// fixes the output frame count at creation time. If the callback buffer
    /// size changes a new resampler is created; in practice cpal uses a stable
    /// buffer size so the resampler is reused across callbacks.
    fn get_or_create_mut(
        &mut self,
        src_rate: u32,
        dst_rate: u32,
        channel: usize,
        output_chunk: usize,
    ) -> &mut ResamplerEntry {
        self.cache
            .entry((src_rate, dst_rate, channel, output_chunk))
            .or_insert_with(|| {
                // High-quality sinc interpolation parameters.
                // 128 taps, 256× oversampling — good quality with reasonable CPU cost.
                let params = SincInterpolationParameters {
                    sinc_len: 128,
                    f_cutoff: rubato::calculate_cutoff(128, rubato::WindowFunction::Blackman2),
                    interpolation: SincInterpolationType::Quadratic,
                    oversampling_factor: 256,
                    window: rubato::WindowFunction::Blackman2,
                };
                // Process mono per call (channels=1), de-interleave externally.
                // Each channel gets its own resampler to maintain independent filter state.
                //
                // FixedAsync::Output: each process() call produces exactly
                // `output_chunk` output frames and consumes `input_frames_next()`
                // input frames (variable, ~output_chunk * src_rate / dst_rate).
                // This avoids zero-padding on every callback — the previous
                // FixedAsync::Input with chunk_size=1024 forced padding ~472 real
                // frames up to 1024, corrupting the 128-tap sinc delay line with
                // trailing zeros on every call and producing repeating phase
                // artifacts at callback boundaries.
                let resampler = Async::<f32>::new_sinc(
                    src_rate as f64 / dst_rate as f64,
                    1.1, // max relative ratio
                    &params,
                    output_chunk, // chunk_size = output frames per call
                    1,            // channels (mono; we de-interleave per channel)
                    FixedAsync::Output,
                )
                .expect("failed to create rubato resampler");
                ResamplerEntry {
                    resampler,
                    channel_input: Vec::new(),
                    input_vecs: vec![Vec::new()],
                }
            })
    }
}

pub fn ensure_output_thread(
    started: &Arc<AtomicBool>,
    start_lock: &Arc<Mutex<()>>,
    playback: Arc<Mutex<PlaybackController>>,
    airplay_audio_tap: Arc<AirPlayAudioTap>,
    airplay_local_output_suppressed: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) -> Result<(), PlaybackError> {
    if started.load(Ordering::SeqCst) {
        return Ok(());
    }

    let _guard = start_lock.lock().map_err(|_| {
        PlaybackError::AudioOutputUnavailable("audio output start lock was poisoned".to_owned())
    })?;
    if started.load(Ordering::SeqCst) {
        return Ok(());
    }

    let (startup_tx, startup_rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        if let Err(error) = start_output_thread(
            playback,
            airplay_audio_tap,
            airplay_local_output_suppressed,
            startup_tx,
            shutdown,
        ) {
            eprintln!("audio output thread failed to start: {error:#}");
        }
    });

    startup_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| {
            PlaybackError::AudioOutputUnavailable(
                "timed out while waiting for audio output thread startup".to_owned(),
            )
        })?
        .map_err(|e| PlaybackError::AudioOutputUnavailable(e.to_string()))?;
    started.store(true, Ordering::SeqCst);

    Ok(())
}

pub fn render_output_buffer(
    playback: &mut PlaybackController,
    output: &mut [f32],
    stem_scratch: &mut Vec<f32>,
    device_sample_rate: u32,
    device_channels: usize,
    resampler_cache: &mut ResamplerCache,
) -> usize {
    output.fill(0.0);

    // In streaming mode, ALWAYS check buffer levels — even when is_buffering is
    // true and snapshot() has set is_playing to false.  Without this, the buffer
    // recovery check (all_above_high → is_buffering = false) is never reached
    // once playback enters the buffering state, because the early return
    // prevents the code below from running.
    if playback
        .current_track
        .as_ref()
        .is_some_and(|t| t.streaming.is_some())
    {
        let track = playback.current_track.as_mut().unwrap();
        let streaming = track.streaming.as_mut().unwrap();

        acknowledge_flush_if_needed(streaming);

        let below_low = any_consumer_below_low_water(streaming);
        let all_above_high = all_consumers_above_high_water(streaming);

        if below_low {
            playback.is_buffering = true;
            // Clear fade-out/pause fades — buffer underrun means we can't produce
            // audio.  Preserve FadingAfterSeek so the seek click-prevention fade
            // resumes once buffering completes.
            if !matches!(
                playback.fade,
                crate::audio::playback::FadeState::FadingAfterSeek { .. }
            ) {
                playback.fade = crate::audio::playback::FadeState::None;
            }
        } else if playback.is_buffering && all_above_high {
            playback.is_buffering = false;
            // Reset the seek fade timer when buffering clears. FadingAfterSeek
            // uses a wall-clock Instant, so real time keeps advancing during the
            // buffering pause even though no audio is being rendered. Without
            // this reset, the 8 ms seek fade would have already expired by the
            // time audio resumes, and take_fade_gain() would return 1.0
            // immediately — defeating the click-prevention mask for every
            // streaming seek that triggers a buffer underrun.
            if let crate::audio::playback::FadeState::FadingAfterSeek { .. } = playback.fade {
                playback.fade = crate::audio::playback::FadeState::FadingAfterSeek {
                    start: std::time::Instant::now(),
                };
            }
        }
    }

    // Check if a fade-out has completed since the last callback. If so,
    // finalize it (set is_playing = false) before taking the snapshot so
    // the snapshot correctly reports paused state.
    playback.finalize_fade_if_complete();

    // Take the snapshot AFTER the buffer-level update so state reflects the
    // current buffering flag (the snapshot taken before the update may still
    // carry the old is_buffering value).
    let snapshot = playback.snapshot();
    // Buffer underrun: snapshot may still report is_playing=true (transport intent)
    // but we must output silence until the ring buffers recover.
    if playback.is_buffering {
        return 0;
    }
    // During a fade-out, snapshot reports is_playing=false for UI transport while
    // the render callback still outputs the envelope until finalize_fade_if_complete.
    if !snapshot.is_playing
        && !matches!(
            playback.fade,
            crate::audio::playback::FadeState::FadingOut { .. }
        )
    {
        return 0;
    }

    let Some(track) = &playback.current_track else {
        return 0;
    };

    let master = snapshot.volume;
    let sv = snapshot.stem_volumes;
    let render_frame = track.render_frame;
    let has_stems = track.stems.is_some();
    let has_streaming = track.streaming.is_some();

    let (rendered, src_frames_advanced) = if has_streaming {
        // Streaming mode: pop from ring buffer consumers.
        // We need mutable access to streaming, so borrow the track mutably.
        let track = playback.current_track.as_mut().unwrap();
        let streaming = track.streaming.as_mut().unwrap();

        let result = match streaming {
            crate::audio::streaming::StreamingTrack::Single { consumer } => {
                render_streaming_single(
                    output,
                    consumer,
                    stem_scratch,
                    master,
                    device_sample_rate,
                    device_channels,
                    None,
                )
            }
            crate::audio::streaming::StreamingTrack::TwoStem {
                vocals,
                accompaniment,
            } => render_streaming_two_stem(
                output,
                vocals,
                accompaniment,
                stem_scratch,
                master,
                sv,
                device_sample_rate,
                device_channels,
            ),
            crate::audio::streaming::StreamingTrack::FourStem {
                vocals,
                drums,
                bass,
                other,
            } => render_streaming_four_stem(
                output,
                vocals,
                drums,
                bass,
                other,
                stem_scratch,
                master,
                sv,
                device_sample_rate,
                device_channels,
            ),
        };

        // Clamp to prevent clipping
        for sample in output.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }

        result
    } else if has_stems {
        // Stem mode: mix from pre-decoded stem buffers
        let track = playback.current_track.as_ref().unwrap();
        let loaded_stems = track.stems.as_ref().unwrap();
        let result = match loaded_stems {
            LoadedStems::TwoStem {
                vocals,
                accompaniment,
            } => {
                let accomp_gain = sv.drums.max(sv.bass).max(sv.other);
                let (r1, f1) = mix_stem_resampled(
                    output,
                    vocals,
                    render_frame,
                    master * sv.vocals,
                    device_sample_rate,
                    device_channels,
                    Some(resampler_cache),
                );
                let (r2, f2) = mix_stem_resampled(
                    output,
                    accompaniment,
                    render_frame,
                    master * accomp_gain,
                    device_sample_rate,
                    device_channels,
                    Some(resampler_cache),
                );
                (r1.max(r2), f1.max(f2))
            }
            LoadedStems::FourStem(stems) => {
                let (r1, f1) = mix_stem_resampled(
                    output,
                    &stems.vocals,
                    render_frame,
                    master * sv.vocals,
                    device_sample_rate,
                    device_channels,
                    Some(resampler_cache),
                );
                let (r2, f2) = mix_stem_resampled(
                    output,
                    &stems.drums,
                    render_frame,
                    master * sv.drums,
                    device_sample_rate,
                    device_channels,
                    Some(resampler_cache),
                );
                let (r3, f3) = mix_stem_resampled(
                    output,
                    &stems.bass,
                    render_frame,
                    master * sv.bass,
                    device_sample_rate,
                    device_channels,
                    Some(resampler_cache),
                );
                let (r4, f4) = mix_stem_resampled(
                    output,
                    &stems.other,
                    render_frame,
                    master * sv.other,
                    device_sample_rate,
                    device_channels,
                    Some(resampler_cache),
                );
                (r1.max(r2).max(r3).max(r4), f1.max(f2).max(f3).max(f4))
            }
        };

        // Clamp to prevent clipping
        for sample in output.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }

        result
    } else {
        // Fallback: play original audio with master volume
        let track = playback.current_track.as_ref().unwrap();
        let original = &track.original_audio;
        let result = mix_stem_resampled(
            output,
            original,
            render_frame,
            master,
            device_sample_rate,
            device_channels,
            Some(resampler_cache),
        );

        // Clamp to prevent clipping
        for sample in output.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }

        result
    };

    // Apply fade-in/fade-out envelope if active.
    if let Some(fade_gain) = playback.take_fade_gain() {
        if fade_gain < 1.0 {
            for sample in output.iter_mut() {
                *sample *= fade_gain;
            }
        }
        // When fade_gain reaches 0.0 (fade-out complete), take_fade_gain already
        // set is_playing = false, so the next callback will return 0 immediately.
    }

    // Advance the render frame counter so the next callback continues seamlessly
    playback.advance_render_frame(src_frames_advanced);

    if has_streaming {
        finalize_streaming_natural_end(playback);
    }

    rendered
}

/// Mix a single audio source into the output buffer with sample-rate conversion
/// and channel mapping. Uses linear interpolation for resampling.
///
/// Returns `(written_output_samples, source_frames_consumed)`.
fn mix_stem_resampled(
    output: &mut [f32],
    audio: &DecodedAudio,
    start_frame: u64,
    gain: f32,
    device_sample_rate: u32,
    device_channels: usize,
    resampler_cache: Option<&mut ResamplerCache>,
) -> (usize, u64) {
    if gain == 0.0 {
        return (0, 0);
    }

    // Most desktop devices run the same 44.1 kHz rate as the source media.
    // Skipping interpolation in that common case removes hot-path math without
    // changing channel mapping or render-frame progression semantics.
    if audio.sample_rate == device_sample_rate {
        return mix_stem_same_rate(output, audio, start_frame, gain, device_channels);
    }

    // Use rubato sinc interpolation when a cache is available (preferred).
    if let Some(cache) = resampler_cache {
        return mix_stem_rubato(
            output,
            audio,
            start_frame,
            gain,
            device_sample_rate,
            device_channels,
            cache,
        );
    }

    // Fallback to linear interpolation.
    mix_stem_linearly_resampled(
        output,
        audio,
        start_frame,
        gain,
        device_sample_rate,
        device_channels,
    )
}

fn mix_stem_same_rate(
    output: &mut [f32],
    audio: &DecodedAudio,
    start_frame: u64,
    gain: f32,
    device_channels: usize,
) -> (usize, u64) {
    let src_channels = audio.channels;
    let total_src_frames = audio.samples.len() / src_channels;
    let src_start_frame = start_frame as usize;
    if src_start_frame >= total_src_frames {
        return (0, 0);
    }

    let output_frames = output.len() / device_channels;
    let available_frames = (total_src_frames - src_start_frame).min(output_frames);

    for out_frame in 0..available_frames {
        let src_frame = src_start_frame + out_frame;
        for out_ch in 0..device_channels {
            let src_ch = if out_ch < src_channels {
                out_ch
            } else {
                out_ch % src_channels
            };
            let sample = audio.samples[src_frame * src_channels + src_ch];
            output[out_frame * device_channels + out_ch] += sample * gain;
        }
    }

    (available_frames * device_channels, available_frames as u64)
}

fn mix_stem_linearly_resampled(
    output: &mut [f32],
    audio: &DecodedAudio,
    start_frame: u64,
    gain: f32,
    device_sample_rate: u32,
    device_channels: usize,
) -> (usize, u64) {
    if gain == 0.0 {
        return (0, 0);
    }

    let src_rate = audio.sample_rate as f64;
    let dst_rate = device_sample_rate as f64;
    let src_channels = audio.channels;
    let total_src_frames = audio.samples.len() / src_channels;

    let src_start_frame = start_frame as usize;
    if src_start_frame >= total_src_frames {
        return (0, 0);
    }

    let output_frames = output.len() / device_channels;
    let rate_ratio = src_rate / dst_rate;
    let mut written = 0;
    let mut rendered_out_frames: usize = 0;

    for out_frame in 0..output_frames {
        // Map output frame to source frame with fractional position
        let src_pos = src_start_frame as f64 + out_frame as f64 * rate_ratio;
        let src_frame_lo = src_pos as usize;

        if src_frame_lo >= total_src_frames {
            break;
        }

        let can_interpolate = src_frame_lo + 1 < total_src_frames;
        let frac = (src_pos - src_frame_lo as f64) as f32;

        for out_ch in 0..device_channels {
            let src_ch = if out_ch < src_channels {
                out_ch
            } else {
                out_ch % src_channels
            };
            let idx_lo = src_frame_lo * src_channels + src_ch;
            let sample = if can_interpolate && frac > 0.0 {
                let idx_hi = (src_frame_lo + 1) * src_channels + src_ch;
                audio.samples[idx_lo] * (1.0 - frac) + audio.samples[idx_hi] * frac
            } else {
                audio.samples[idx_lo]
            };
            output[out_frame * device_channels + out_ch] += sample * gain;
        }

        rendered_out_frames = out_frame + 1;
        written = rendered_out_frames * device_channels;
    }

    // Calculate how many source frames the next call should skip over.
    // This must match precisely so consecutive buffers join seamlessly.
    let src_frames_consumed = (rendered_out_frames as f64 * rate_ratio).round() as u64;

    (written, src_frames_consumed)
}

/// Mix a single audio source into the output buffer using rubato sinc resampling.
/// Higher quality than linear interpolation — uses windowed-sinc with 128 taps.
///
/// Returns `(written_output_samples, source_frames_consumed)`.
fn mix_stem_rubato(
    output: &mut [f32],
    audio: &DecodedAudio,
    start_frame: u64,
    gain: f32,
    device_sample_rate: u32,
    device_channels: usize,
    resampler_cache: &mut ResamplerCache,
) -> (usize, u64) {
    let src_channels = audio.channels;
    let total_src_frames = audio.samples.len() / src_channels;
    let src_start_frame = start_frame as usize;

    if src_start_frame >= total_src_frames {
        return (0, 0);
    }

    let output_frames = output.len() / device_channels;

    // With FixedAsync::Output, each process() call produces exactly
    // `output_frames` output frames and consumes `input_frames_next()` input
    // frames (the variable number needed for this rate pair and filter state).
    // We feed exactly that many frames — real source frames, zero-padded only
    // at end-of-track. This avoids the per-callback zero-padding that corrupted
    // the sinc delay line when using FixedAsync::Input with a large chunk_size.
    let input_needed = resampler_cache
        .get_or_create_mut(audio.sample_rate, device_sample_rate, 0, output_frames)
        .resampler
        .input_frames_next();
    let real_available = total_src_frames - src_start_frame;
    let frames_from_source = real_available.min(input_needed);
    let feed_frames = input_needed;

    let mut max_out_frames = 0usize;

    // rubato uses planar (non-interleaved) buffers. Process each source channel
    // through its own mono resampler (independent filter state) and mix into the
    // interleaved output. Scratch buffers (channel_input, input_vecs) are reused
    // from the cache to avoid per-callback heap allocation on the realtime
    // audio thread.
    for src_ch in 0..src_channels {
        let entry = resampler_cache.get_or_create_mut(
            audio.sample_rate,
            device_sample_rate,
            src_ch,
            output_frames,
        );

        // De-interleave source frames into the reusable buffer, zero-padding
        // the tail only at end-of-track (real_available < input_needed).
        entry.channel_input.resize(feed_frames, 0.0);
        for (frame, slot) in entry
            .channel_input
            .iter_mut()
            .enumerate()
            .take(frames_from_source)
        {
            *slot = audio.samples[(src_start_frame + frame) * src_channels + src_ch];
        }

        // Construct a planar adapter for rubato: 1 channel, feed_frames.
        // input_vecs always has exactly 1 element (mono); reuse the allocation.
        entry.input_vecs[0] = std::mem::take(&mut entry.channel_input);
        let input_adapter = match rubato::audioadapter_buffers::direct::SequentialSliceOfVecs::new(
            &entry.input_vecs,
            1,
            feed_frames,
        ) {
            Ok(adapter) => adapter,
            Err(_) => {
                // Reclaim channel_input for the next callback.
                entry.channel_input = std::mem::take(&mut entry.input_vecs[0]);
                continue;
            }
        };

        // Process through rubato. Returns an interleaved owned buffer.
        // FixedAsync::Output guarantees exactly output_frames output frames.
        let output_adapter = match entry.resampler.process(&input_adapter, 0, None) {
            Ok(out) => out,
            Err(_) => {
                entry.channel_input = std::mem::take(&mut entry.input_vecs[0]);
                continue;
            }
        };

        // Reclaim the input buffer for the next callback. take_data() consumed
        // the adapter's internal Vec, but input_vecs[0] still owns the input.
        entry.channel_input = std::mem::take(&mut entry.input_vecs[0]);

        // For mono (1 channel), interleaved = sequential. take_data() gives Vec<f32>.
        let out_data = output_adapter.take_data();

        // Write all produced output frames (capped to the callback's output).
        // At end-of-track the tail may contain flushed filter output plus silence
        // from zero-padding — the silence is harmless and the track ends here.
        let frames_to_write = out_data.len().min(output_frames);
        for out_frame in 0..frames_to_write {
            let sample = out_data[out_frame];
            for out_ch in 0..device_channels {
                let target_ch = if out_ch < src_channels {
                    out_ch
                } else {
                    out_ch % src_channels
                };
                if target_ch == src_ch {
                    output[out_frame * device_channels + out_ch] += sample * gain;
                }
            }
        }
        max_out_frames = max_out_frames.max(frames_to_write);
    }

    // Source frames consumed = the real (non-padded) frames we fed.
    let src_frames_consumed = frames_from_source as u64;

    (max_out_frames * device_channels, src_frames_consumed)
}

/// Source frames needed to fill `output_frames` device frames at the given rates.
fn src_frames_for_output(output_frames: usize, src_rate: u32, device_rate: u32) -> u64 {
    if src_rate == device_rate {
        output_frames as u64
    } else {
        (output_frames as f64 * src_rate as f64 / device_rate as f64).ceil() as u64 + 1
    }
}

/// Source frames to drain when a stem is muted (no interpolation lookahead).
fn src_frames_for_muted_drain(output_frames: usize, src_rate: u32, device_rate: u32) -> usize {
    if src_rate == device_rate {
        output_frames
    } else {
        (output_frames as f64 * src_rate as f64 / device_rate as f64).round() as usize
    }
}

/// Shared source-frame budget for multi-stem streaming: min(available, needed) per stem.
fn compute_shared_src_frame_budget(
    consumers: &[&crate::audio::streaming::AudioConsumer],
    output_frames: usize,
    device_sample_rate: u32,
) -> u64 {
    let mut budget = u64::MAX;
    for consumer in consumers {
        let needed = src_frames_for_output(output_frames, consumer.sample_rate, device_sample_rate);
        let available = consumer.available_src_frames() as u64;
        budget = budget.min(available.min(needed));
    }
    if budget == u64::MAX {
        0
    } else {
        budget
    }
}

/// When every streaming consumer has EOF'd and drained, stop playback and backfill
/// unknown duration from the final render position.
fn finalize_streaming_natural_end(playback: &mut PlaybackController) {
    playback.finalize_streaming_natural_end();
}

/// Render a single streaming track into the output buffer.
/// Pops samples from the ring buffer consumer and applies gain.
/// Returns (rendered_output_samples, source_frames_consumed).
///
/// `scratch` is a reusable buffer — callers pass one pre-allocated instance to
/// avoid `vec![]` allocations on the realtime audio thread.
fn render_streaming_single(
    output: &mut [f32],
    consumer: &mut crate::audio::streaming::AudioConsumer,
    scratch: &mut Vec<f32>,
    gain: f32,
    device_sample_rate: u32,
    device_channels: usize,
    max_src_frames: Option<u64>,
) -> (usize, u64) {
    let output_frames = output.len() / device_channels;
    let src_channels = consumer.channels;

    let frame_cap = max_src_frames.map(|b| b as usize).unwrap_or_else(|| {
        if gain == 0.0 {
            src_frames_for_muted_drain(output_frames, consumer.sample_rate, device_sample_rate)
        } else if consumer.sample_rate == device_sample_rate {
            output_frames
        } else {
            src_frames_for_output(output_frames, consumer.sample_rate, device_sample_rate) as usize
        }
    });

    if gain == 0.0 {
        // Muted streaming tracks still advance so re-enabling a stem stays
        // aligned with the shared render clock. Drain by source frames, not
        // device frames, because common 44.1kHz→48kHz output resampling would
        // otherwise skip too far while the stem is muted.
        scratch.resize(frame_cap.saturating_mul(src_channels), 0.0);
        let popped = consumer.pop_samples(scratch);
        return (0, (popped / src_channels.max(1)) as u64);
    }

    let src_rate = consumer.sample_rate;

    if src_rate == device_sample_rate {
        // Same rate — direct pop with channel mapping
        scratch.resize(frame_cap.saturating_mul(src_channels), 0.0);
        let popped = consumer.pop_samples(scratch);
        let src_frames = (popped / src_channels.max(1)).min(frame_cap);

        for out_frame in 0..src_frames {
            for out_ch in 0..device_channels {
                let src_ch = if out_ch < src_channels {
                    out_ch
                } else {
                    out_ch % src_channels
                };
                let sample = scratch[out_frame * src_channels + src_ch];
                output[out_frame * device_channels + out_ch] += sample * gain;
            }
        }

        (src_frames * device_channels, src_frames as u64)
    } else {
        // Different rate — linear interpolation resampling
        let rate_ratio = src_rate as f64 / device_sample_rate as f64;
        scratch.resize(frame_cap.saturating_mul(src_channels), 0.0);
        let popped = consumer.pop_samples(scratch);
        let available_src_frames = popped / src_channels.max(1);

        let mut written = 0;
        let mut rendered_out_frames = 0;
        for out_frame in 0..output_frames {
            let src_pos = out_frame as f64 * rate_ratio;
            let src_idx = src_pos as usize;
            let frac = src_pos - src_idx as f64;

            if src_idx + 1 >= available_src_frames {
                break;
            }

            for out_ch in 0..device_channels {
                let src_ch = if out_ch < src_channels {
                    out_ch
                } else {
                    out_ch % src_channels
                };
                let s0 = scratch[src_idx * src_channels + src_ch];
                let s1 = scratch[(src_idx + 1) * src_channels + src_ch];
                let sample = s0 + (s1 - s0) * frac as f32;
                output[out_frame * device_channels + out_ch] += sample * gain;
            }
            written += device_channels;
            rendered_out_frames += 1;
        }

        let src_frames_consumed = if rendered_out_frames > 0 {
            (rendered_out_frames as f64 * rate_ratio)
                .round()
                .min(frame_cap as f64) as u64
        } else {
            0
        };
        let consumed_samples = (src_frames_consumed as usize)
            .saturating_mul(src_channels)
            .min(popped);
        if consumed_samples < popped {
            consumer.prepend_samples(&scratch[consumed_samples..popped]);
        }

        (written, src_frames_consumed)
    }
}

/// Render two streaming stems (vocals + accompaniment).
fn render_streaming_two_stem(
    output: &mut [f32],
    vocals: &mut crate::audio::streaming::AudioConsumer,
    accompaniment: &mut crate::audio::streaming::AudioConsumer,
    scratch: &mut Vec<f32>,
    master: f32,
    sv: StemVolumes,
    device_sample_rate: u32,
    device_channels: usize,
) -> (usize, u64) {
    let output_frames = output.len() / device_channels;
    let budget = compute_shared_src_frame_budget(
        &[vocals as &_, accompaniment as &_],
        output_frames,
        device_sample_rate,
    );
    if budget == 0 {
        return (0, 0);
    }
    let max_frames = Some(budget);
    let accomp_gain = sv.drums.max(sv.bass).max(sv.other);
    let (r1, _) = render_streaming_single(
        output,
        vocals,
        scratch,
        master * sv.vocals,
        device_sample_rate,
        device_channels,
        max_frames,
    );
    let (r2, _) = render_streaming_single(
        output,
        accompaniment,
        scratch,
        master * accomp_gain,
        device_sample_rate,
        device_channels,
        max_frames,
    );
    (r1.max(r2), budget)
}

/// Render four streaming stems.
fn render_streaming_four_stem(
    output: &mut [f32],
    vocals: &mut crate::audio::streaming::AudioConsumer,
    drums: &mut crate::audio::streaming::AudioConsumer,
    bass: &mut crate::audio::streaming::AudioConsumer,
    other: &mut crate::audio::streaming::AudioConsumer,
    scratch: &mut Vec<f32>,
    master: f32,
    sv: StemVolumes,
    device_sample_rate: u32,
    device_channels: usize,
) -> (usize, u64) {
    let output_frames = output.len() / device_channels;
    let budget = compute_shared_src_frame_budget(
        &[vocals as &_, drums as &_, bass as &_, other as &_],
        output_frames,
        device_sample_rate,
    );
    if budget == 0 {
        return (0, 0);
    }
    let max_frames = Some(budget);
    let (r1, _) = render_streaming_single(
        output,
        vocals,
        scratch,
        master * sv.vocals,
        device_sample_rate,
        device_channels,
        max_frames,
    );
    let (r2, _) = render_streaming_single(
        output,
        drums,
        scratch,
        master * sv.drums,
        device_sample_rate,
        device_channels,
        max_frames,
    );
    let (r3, _) = render_streaming_single(
        output,
        bass,
        scratch,
        master * sv.bass,
        device_sample_rate,
        device_channels,
        max_frames,
    );
    let (r4, _) = render_streaming_single(
        output,
        other,
        scratch,
        master * sv.other,
        device_sample_rate,
        device_channels,
        max_frames,
    );
    (r1.max(r2).max(r3).max(r4), budget)
}

fn acknowledge_flush_if_needed(streaming: &mut crate::audio::streaming::StreamingTrack) {
    match streaming {
        crate::audio::streaming::StreamingTrack::Single { consumer } => {
            if consumer.needs_flush() {
                consumer.acknowledge_flush();
            }
        }
        crate::audio::streaming::StreamingTrack::TwoStem {
            vocals,
            accompaniment,
        } => {
            if vocals.needs_flush() {
                vocals.acknowledge_flush();
            }
            if accompaniment.needs_flush() {
                accompaniment.acknowledge_flush();
            }
        }
        crate::audio::streaming::StreamingTrack::FourStem {
            vocals,
            drums,
            bass,
            other,
        } => {
            if vocals.needs_flush() {
                vocals.acknowledge_flush();
            }
            if drums.needs_flush() {
                drums.acknowledge_flush();
            }
            if bass.needs_flush() {
                bass.acknowledge_flush();
            }
            if other.needs_flush() {
                other.acknowledge_flush();
            }
        }
    }
}

// R5 RATIONALE: All-or-nothing buffering policy.
//
// Multi-stem rendering requires every stem to stay frame-synchronized with the
// shared source clock. If we allowed playback to continue while one stem is
// below the low-water mark, the render callback would mix rendered audio from
// buffered stems with silence (or stale data) from the depleted stem, producing
// audible artifacts and — because the source clock advances — permanent drift
// between stems that can never self-heal.
//
// Muting the entire stream when *any* required stem is below low water
// guarantees that when playback resumes (all stems above high water), every
// consumer is at the same source-clock position. This is the simplest
// correctness strategy for synchronised multi-stem playback.
fn any_consumer_below_low_water(streaming: &crate::audio::streaming::StreamingTrack) -> bool {
    match streaming {
        crate::audio::streaming::StreamingTrack::Single { consumer } => {
            consumer.is_below_low_water()
        }
        crate::audio::streaming::StreamingTrack::TwoStem {
            vocals,
            accompaniment,
        } => vocals.is_below_low_water() || accompaniment.is_below_low_water(),
        crate::audio::streaming::StreamingTrack::FourStem {
            vocals,
            drums,
            bass,
            other,
        } => {
            vocals.is_below_low_water()
                || drums.is_below_low_water()
                || bass.is_below_low_water()
                || other.is_below_low_water()
        }
    }
}

fn all_consumers_above_high_water(streaming: &crate::audio::streaming::StreamingTrack) -> bool {
    match streaming {
        crate::audio::streaming::StreamingTrack::Single { consumer } => {
            consumer.is_above_high_water()
        }
        crate::audio::streaming::StreamingTrack::TwoStem {
            vocals,
            accompaniment,
        } => vocals.is_above_high_water() && accompaniment.is_above_high_water(),
        crate::audio::streaming::StreamingTrack::FourStem {
            vocals,
            drums,
            bass,
            other,
        } => {
            vocals.is_above_high_water()
                && drums.is_above_high_water()
                && bass.is_above_high_water()
                && other.is_above_high_water()
        }
    }
}

fn build_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    playback: Arc<Mutex<PlaybackController>>,
    airplay_audio_tap: Arc<AirPlayAudioTap>,
    airplay_local_output_suppressed: Arc<AtomicBool>,
) -> Result<Stream, PlaybackError>
where
    T: SizedSample + Sample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate;
    let mut scratch = Vec::<f32>::new();
    // Pre-allocated scratch buffer for per-stem pop operations inside the audio
    // callback.  Reusing one buffer across all stems avoids `vec![]` allocations
    // on the realtime thread — after the first callback the capacity is sufficient
    // and `resize` becomes a no-op memset.
    let mut stem_scratch = Vec::<f32>::new();
    // R1: Pre-allocated scratch buffer for AirPlay downmix. Eliminates the
    // per-callback heap allocation that `downmix_for_airplay` previously caused.
    let mut airplay_scratch = Vec::<f32>::new();
    // Cached rubato resamplers for sample-rate conversion. Resamplers maintain
    // internal state so they must be reused across consecutive callbacks.
    let mut resampler_cache = ResamplerCache::new();

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _info| {
                // The audio callback is a realtime path. Reallocating a fresh scratch
                // buffer every device tick can introduce allocator stalls and audible
                // glitches, so the closure keeps one buffer and resizes only when the
                // device changes its callback frame count.
                scratch.resize(data.len(), 0.0);

                // Realtime callback: never block on the playback mutex. If control
                // threads hold the lock (seek/volume/load), output silence for this
                // tick rather than stalling the device callback.
                let mut rendered_samples = 0;
                if let Ok(mut controller) = playback.try_lock() {
                    rendered_samples = render_output_buffer(
                        &mut controller,
                        &mut scratch,
                        &mut stem_scratch,
                        sample_rate,
                        channels,
                        &mut resampler_cache,
                    );
                } else {
                    scratch.fill(0.0);
                }

                forward_rendered_audio_to_airplay(
                    rendered_samples,
                    &scratch,
                    channels,
                    sample_rate,
                    &airplay_audio_tap,
                    &mut airplay_scratch,
                );
                write_output_samples(
                    &scratch,
                    data,
                    airplay_local_output_suppressed.load(Ordering::SeqCst),
                );
            },
            move |error| {
                eprintln!("audio output stream error: {error}");
            },
            None,
        )
        .map_err(|e| {
            PlaybackError::AudioOutputUnavailable(format!(
                "failed to build audio output stream: {e}"
            ))
        })?;

    Ok(stream)
}

fn start_output_thread(
    playback: Arc<Mutex<PlaybackController>>,
    airplay_audio_tap: Arc<AirPlayAudioTap>,
    airplay_local_output_suppressed: Arc<AtomicBool>,
    startup_tx: mpsc::SyncSender<Result<(), PlaybackError>>,
    shutdown: Arc<AtomicBool>,
) -> Result<(), PlaybackError> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| {
        PlaybackError::AudioOutputUnavailable(
            "no default output audio device is available".to_owned(),
        )
    })?;
    let config = device.default_output_config().map_err(|e| {
        PlaybackError::AudioOutputUnavailable(format!(
            "failed to read default audio output config: {e}"
        ))
    })?;
    let stream = match config.sample_format() {
        SampleFormat::F32 => build_output_stream::<f32>(
            &device,
            &config.into(),
            playback,
            airplay_audio_tap,
            airplay_local_output_suppressed,
        )?,
        SampleFormat::I16 => build_output_stream::<i16>(
            &device,
            &config.into(),
            playback,
            airplay_audio_tap,
            airplay_local_output_suppressed,
        )?,
        SampleFormat::U16 => build_output_stream::<u16>(
            &device,
            &config.into(),
            playback,
            airplay_audio_tap,
            airplay_local_output_suppressed,
        )?,
        sample_format => {
            return Err(PlaybackError::AudioOutputUnavailable(format!(
                "unsupported audio output sample format: {sample_format:?}"
            )));
        }
    };

    stream.play().map_err(|e| {
        PlaybackError::AudioOutputUnavailable(format!("failed to start audio output stream: {e}"))
    })?;
    let _ = startup_tx.send(Ok(()));

    while !shutdown.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_secs(60));
        let _keep_alive = &stream;
    }

    Ok(())
}

fn forward_rendered_audio_to_airplay(
    rendered_samples: usize,
    scratch: &[f32],
    channels: usize,
    sample_rate: u32,
    airplay_audio_tap: &AirPlayAudioTap,
    airplay_scratch: &mut Vec<f32>,
) {
    if rendered_samples == 0 {
        return;
    }

    let rendered_samples = rendered_samples.min(scratch.len());
    if rendered_samples == 0 {
        return;
    }

    downmix_for_airplay_into(&scratch[..rendered_samples], channels, airplay_scratch);
    if !airplay_scratch.is_empty() {
        // R1: Swap out the buffer so push_interleaved takes ownership (no to_vec).
        // The replacement Vec reuses the same capacity for the next callback.
        let owned = std::mem::replace(
            airplay_scratch,
            Vec::with_capacity(airplay_scratch.capacity()),
        );
        airplay_audio_tap.push_interleaved(sample_rate, 2, owned);
    }
}

/// R1: Downmix multi-channel audio to stereo, writing into a reusable buffer
/// to avoid heap allocation on the realtime audio callback thread.
fn downmix_for_airplay_into(samples: &[f32], channels: usize, output: &mut Vec<f32>) {
    output.clear();

    if channels == 0 || samples.is_empty() {
        return;
    }

    let stereo_frames = samples.len() / channels;
    output.reserve(stereo_frames * 2);

    for frame in samples.chunks(channels) {
        let (left, right) = match channels {
            1 => (frame[0], frame[0]),
            2 => (frame[0], frame[1]),
            _ => {
                let sum: f32 = frame.iter().sum();
                let avg = sum / channels as f32;
                (avg, avg)
            }
        };
        output.push(left);
        output.push(right);
    }
}

fn write_output_samples<T>(scratch: &[f32], data: &mut [T], suppress_local_output: bool)
where
    T: SizedSample + Sample + cpal::FromSample<f32>,
{
    if suppress_local_output {
        for output_sample in data.iter_mut() {
            *output_sample = T::from_sample(0.0);
        }
        return;
    }

    for (input_sample, output_sample) in scratch.iter().zip(data.iter_mut()) {
        *output_sample = T::from_sample(*input_sample);
    }
}

#[cfg(test)]
mod tests {
    use super::{forward_rendered_audio_to_airplay, render_output_buffer, write_output_samples};
    use crate::airplay_stream::AirPlayAudioTap;

    #[test]
    fn write_output_samples_preserves_rendered_audio_when_not_suppressed() {
        let mut output = [0.0_f32; 4];
        write_output_samples(&[0.1, -0.2, 0.3, -0.4], &mut output, false);
        assert_eq!(output, [0.1, -0.2, 0.3, -0.4]);
    }

    #[test]
    fn write_output_samples_silences_local_device_when_suppressed() {
        let mut output = [1.0_f32; 4];
        write_output_samples(&[0.1, -0.2, 0.3, -0.4], &mut output, true);
        assert_eq!(output, [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn forward_rendered_audio_to_airplay_skips_unrendered_frames() {
        let tap = AirPlayAudioTap::new(4);
        let mut airplay_scratch = Vec::new();
        forward_rendered_audio_to_airplay(
            0,
            &[0.8, 0.7, 0.6, 0.5],
            2,
            44_100,
            &tap,
            &mut airplay_scratch,
        );

        assert!(tap.drain_pending().is_empty());
    }

    #[test]
    fn forward_rendered_audio_to_airplay_limits_payload_to_rendered_samples() {
        let tap = AirPlayAudioTap::new(4);
        let mut airplay_scratch = Vec::new();
        forward_rendered_audio_to_airplay(
            4,
            &[0.1, 0.2, 0.3, 0.4, 0.9, 0.8],
            2,
            44_100,
            &tap,
            &mut airplay_scratch,
        );

        let drained = tap.drain_pending();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].samples, vec![0.1, 0.2, 0.3, 0.4]);
    }

    /// Regression test: once `is_buffering` is set (e.g. initial empty buffer),
    /// `render_output_buffer` must continue checking buffer levels on every
    /// callback — even when `snapshot()` reports `is_playing: false` — so the
    /// high-water recovery path can clear the flag and resume playback.
    #[test]
    fn streaming_buffering_recovers_after_underrun() {
        use crate::audio::playback::PlaybackController;
        use crate::audio::streaming::{self, StreamingTrack};

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        let (mut prod, consumer) = streaming::create_stream_pair(sample_rate, channels);

        let mut controller = PlaybackController::default();
        controller.start_track_streaming(
            "test-recovery".to_owned(),
            sample_rate,
            channels,
            30_000, // 30-second track
            StreamingTrack::Single { consumer },
            0,
        );

        // 1st callback: buffer is empty → below low water → is_buffering = true.
        let device_channels = 2;
        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = super::ResamplerCache::new();
        let rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            sample_rate,
            device_channels,
            &mut rc,
        );
        assert_eq!(rendered, 0);
        assert!(
            controller.is_buffering,
            "should enter buffering after empty underrun"
        );

        // 2nd callback: is_buffering is true → snapshot reports transport intent
        // (is_playing stays true) but render_output_buffer still outputs silence.
        output.fill(0.0);
        let rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            sample_rate,
            device_channels,
            &mut rc,
        );
        assert_eq!(rendered, 0);

        // Simulate the decode thread filling the buffer past the high-water mark.
        let high_water = 88_200usize; // HIGH_WATER_SAMPLES
        let filler = vec![0.5f32; high_water + 1000];
        prod.push_samples(&filler);

        // 3rd callback: buffer is now above high water → is_buffering clears.
        output.fill(0.0);
        let rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            sample_rate,
            device_channels,
            &mut rc,
        );
        assert!(
            !controller.is_buffering,
            "buffering flag should clear once buffer refills"
        );
        assert!(rendered > 0, "should render audio after recovery");
    }

    #[test]
    fn streaming_resample_keeps_lookahead_sample_for_next_callback() {
        use crate::audio::streaming;

        let (mut prod, mut consumer) = streaming::create_stream_pair(4, 1);
        let input = [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(prod.push_samples(&input), input.len());

        let mut first = vec![0.0_f32; 4];
        let mut scratch = Vec::new();
        let rendered = super::render_streaming_single(
            &mut first,
            &mut consumer,
            &mut scratch,
            1.0,
            8,
            1,
            None,
        );
        assert_eq!(rendered, (4, 2));
        assert_eq!(first, vec![0.0, 0.5, 1.0, 1.5]);

        let mut second = vec![0.0_f32; 4];
        let rendered = super::render_streaming_single(
            &mut second,
            &mut consumer,
            &mut scratch,
            1.0,
            8,
            1,
            None,
        );
        assert_eq!(rendered, (4, 2));
        assert_eq!(second, vec![2.0, 2.5, 3.0, 3.5]);
    }

    #[test]
    fn muted_streaming_resample_advances_by_source_frames() {
        use crate::audio::streaming;

        let (mut prod, mut consumer) = streaming::create_stream_pair(4, 1);
        let input = [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(prod.push_samples(&input), input.len());

        let mut muted = vec![0.0_f32; 4];
        let mut scratch = Vec::new();
        let rendered = super::render_streaming_single(
            &mut muted,
            &mut consumer,
            &mut scratch,
            0.0,
            8,
            1,
            None,
        );
        assert_eq!(rendered, (0, 2));

        let mut audible = vec![0.0_f32; 4];
        let rendered = super::render_streaming_single(
            &mut audible,
            &mut consumer,
            &mut scratch,
            1.0,
            8,
            1,
            None,
        );
        assert_eq!(rendered, (4, 2));
        assert_eq!(audible, vec![2.0, 2.5, 3.0, 3.5]);
    }

    /// R4: When one stem has fewer samples than another, the source clock must
    /// NOT advance past the slow stem. This test forces one stem to
    /// under-render and verifies the committed frame count is the minimum.
    #[test]
    fn streaming_two_stem_clock_does_not_advance_past_slow_stem() {
        use crate::audio::streaming;

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        // Stem 1 (vocals): plenty of data
        let (mut prod1, mut consumer1) = streaming::create_stream_pair(sample_rate, channels);
        let filler1 = vec![0.5_f32; sample_rate as usize * channels]; // 1 second
        prod1.push_samples(&filler1);

        // Stem 2 (accompaniment): only 100ms of data — will under-render
        let (mut prod2, mut consumer2) = streaming::create_stream_pair(sample_rate, channels);
        let filler2 = vec![0.3_f32; (sample_rate as usize / 10) * channels]; // 100ms
        prod2.push_samples(&filler2);

        // Request 512 output frames (= 512 source frames at same rate)
        let device_channels = 2;
        let mut scratch = Vec::new();

        // Render both stems — stem2 only has ~100ms = ~4410 frames, stem1 has 44100
        // Requested: 512 frames. Both stems have enough for one callback.
        // We need more callbacks or a smaller buffer to actually see under-render.
        // Use a buffer of 4410 frames (= 100ms) so stem2 is exactly at the edge.
        let big_frames = (sample_rate / 10) as usize; // 4410 frames = 100ms
        let mut big_output = vec![0.0f32; big_frames * device_channels];

        // First callback: both stems render 4410 frames (stem2 is now empty)
        let result = super::render_streaming_two_stem(
            &mut big_output,
            &mut consumer1,
            &mut consumer2,
            &mut scratch,
            1.0,
            super::StemVolumes::default(),
            sample_rate,
            device_channels,
        );
        let (_rendered_1, frames_1) = result;
        assert!(frames_1 > 0, "first callback should render frames");

        // Second callback: stem1 still has data, stem2 has nothing
        big_output.fill(0.0);
        let result2 = super::render_streaming_two_stem(
            &mut big_output,
            &mut consumer1,
            &mut consumer2,
            &mut scratch,
            1.0,
            super::StemVolumes::default(),
            sample_rate,
            device_channels,
        );
        let (_rendered_2, frames_2) = result2;

        // CRITICAL: frames advanced must be 0 or small — stem2 has no data,
        // so min(f1, f2) must be 0 (f2=0).
        assert_eq!(
            frames_2, 0,
            "when one stem has no data, source clock must not advance (min)"
        );
    }

    /// R4: Four-stem variant — when one of four stems under-renders, the source
    /// clock must advance only by the minimum rendered count.
    #[test]
    fn streaming_four_stem_clock_uses_minimum_across_stems() {
        use crate::audio::streaming;

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;

        let (mut p_v, mut c_v) = streaming::create_stream_pair(sample_rate, channels);
        let (mut p_d, mut c_d) = streaming::create_stream_pair(sample_rate, channels);
        let (mut p_b, mut c_b) = streaming::create_stream_pair(sample_rate, channels);
        let (_p_o, mut c_o) = streaming::create_stream_pair(sample_rate, channels);

        // Fill all stems generously except "other" which is empty
        let fill = vec![0.5_f32; sample_rate as usize * channels];
        p_v.push_samples(&fill);
        p_d.push_samples(&fill);
        p_b.push_samples(&fill);
        // p_o: no data

        let device_channels = 2;
        let frames = 512usize;
        let mut output = vec![0.0f32; frames * device_channels];
        let mut scratch = Vec::new();

        let (_rendered, src_frames) = super::render_streaming_four_stem(
            &mut output,
            &mut c_v,
            &mut c_d,
            &mut c_b,
            &mut c_o,
            &mut scratch,
            1.0,
            super::StemVolumes::default(),
            sample_rate,
            device_channels,
        );

        assert_eq!(
            src_frames, 0,
            "four-stem clock must not advance when any stem has no data"
        );
    }

    /// R5: Position must not advance while any required stem is unavailable
    /// (below low water). The all-or-nothing buffering policy ensures this.
    #[test]
    fn streaming_two_stem_enters_buffering_when_one_stem_below_low_water() {
        use crate::audio::playback::PlaybackController;
        use crate::audio::streaming::{self, StreamingTrack};

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;

        let (mut prod_v, consumer_v) = streaming::create_stream_pair(sample_rate, channels);
        let (_prod_a, consumer_a) = streaming::create_stream_pair(sample_rate, channels);

        // Fill vocals generously but leave accompaniment empty (below low water).
        let filler = vec![0.5_f32; sample_rate as usize * channels];
        prod_v.push_samples(&filler);

        let mut controller = PlaybackController::default();
        controller.start_track_streaming(
            "test-r5".to_owned(),
            sample_rate,
            channels,
            30_000,
            StreamingTrack::TwoStem {
                vocals: consumer_v,
                accompaniment: consumer_a,
            },
            0,
        );

        let device_channels = 2;
        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = super::ResamplerCache::new();
        let rendered = super::render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            sample_rate,
            device_channels,
            &mut rc,
        );

        // Accompaniment is empty → below low water → must enter buffering.
        assert!(
            controller.is_buffering,
            "must enter buffering when a required stem is below low water"
        );
        assert_eq!(rendered, 0, "must render silence during buffering");

        let snapshot = controller.snapshot();
        assert_eq!(
            snapshot.position_ms, 0,
            "position must not advance during buffering"
        );
    }
}
