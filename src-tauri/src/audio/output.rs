use crate::airplay_stream::AirPlayAudioTap;
use crate::audio::decode::DecodedAudio;
use crate::audio::error::PlaybackError;
use crate::audio::playback::{LoadedStems, PlaybackController, StemVolumes};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, SizedSample, Stream};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

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
    device_sample_rate: u32,
    device_channels: usize,
) -> usize {
    output.fill(0.0);

    let snapshot = playback.snapshot();
    if !snapshot.is_playing {
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
                    master,
                    device_sample_rate,
                    device_channels,
                )
            }
            crate::audio::streaming::StreamingTrack::TwoStem {
                vocals,
                accompaniment,
            } => render_streaming_two_stem(
                output,
                vocals,
                accompaniment,
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

        // Handle flush after seek and buffering underrun/recovery.
        let track = playback.current_track.as_mut().unwrap();
        let streaming = track.streaming.as_mut().unwrap();

        // First, acknowledge any pending flush (drains stale samples after seek).
        acknowledge_flush_if_needed(streaming);

        // Then check buffering state.
        let below_low = any_consumer_below_low_water(streaming);
        let all_above_high = all_consumers_above_high_water(streaming);

        if below_low {
            playback.is_buffering = true;
        } else if playback.is_buffering && all_above_high {
            // All stems have refilled — resume playback.
            playback.is_buffering = false;
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
                );
                let (r2, f2) = mix_stem_resampled(
                    output,
                    accompaniment,
                    render_frame,
                    master * accomp_gain,
                    device_sample_rate,
                    device_channels,
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
                );
                let (r2, f2) = mix_stem_resampled(
                    output,
                    &stems.drums,
                    render_frame,
                    master * sv.drums,
                    device_sample_rate,
                    device_channels,
                );
                let (r3, f3) = mix_stem_resampled(
                    output,
                    &stems.bass,
                    render_frame,
                    master * sv.bass,
                    device_sample_rate,
                    device_channels,
                );
                let (r4, f4) = mix_stem_resampled(
                    output,
                    &stems.other,
                    render_frame,
                    master * sv.other,
                    device_sample_rate,
                    device_channels,
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
        );

        // Clamp to prevent clipping
        for sample in output.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }

        result
    };

    // Advance the render frame counter so the next callback continues seamlessly
    playback.advance_render_frame(src_frames_advanced);

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

/// Render a single streaming track into the output buffer.
/// Pops samples from the ring buffer consumer and applies gain.
/// Returns (rendered_output_samples, source_frames_consumed).
fn render_streaming_single(
    output: &mut [f32],
    consumer: &mut crate::audio::streaming::AudioConsumer,
    gain: f32,
    device_sample_rate: u32,
    device_channels: usize,
) -> (usize, u64) {
    if gain == 0.0 {
        // Drain the buffer to keep it flowing, but don't write to output
        let src_channels = consumer.channels;
        let output_frames = output.len() / device_channels;
        let mut scratch = vec![0.0f32; output_frames * src_channels];
        let popped = consumer.pop_samples(&mut scratch);
        let src_frames = (popped / src_channels) as u64;
        return (0, src_frames);
    }

    let src_channels = consumer.channels;
    let src_rate = consumer.sample_rate;
    let output_frames = output.len() / device_channels;

    if src_rate == device_sample_rate {
        // Same rate — direct pop with channel mapping
        let mut scratch = vec![0.0f32; output_frames * src_channels];
        let popped = consumer.pop_samples(&mut scratch);
        let src_frames = popped / src_channels;

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
        let needed_src_frames = (output_frames as f64 * rate_ratio).ceil() as usize + 1;
        let mut scratch = vec![0.0f32; needed_src_frames * src_channels];
        let popped = consumer.pop_samples(&mut scratch);
        let available_src_frames = popped / src_channels;

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
            (rendered_out_frames as f64 * rate_ratio).round() as u64
        } else {
            0
        };

        (written, src_frames_consumed)
    }
}

/// Render two streaming stems (vocals + accompaniment).
fn render_streaming_two_stem(
    output: &mut [f32],
    vocals: &mut crate::audio::streaming::AudioConsumer,
    accompaniment: &mut crate::audio::streaming::AudioConsumer,
    master: f32,
    sv: StemVolumes,
    device_sample_rate: u32,
    device_channels: usize,
) -> (usize, u64) {
    let accomp_gain = sv.drums.max(sv.bass).max(sv.other);
    let (r1, f1) = render_streaming_single(
        output,
        vocals,
        master * sv.vocals,
        device_sample_rate,
        device_channels,
    );
    let (r2, f2) = render_streaming_single(
        output,
        accompaniment,
        master * accomp_gain,
        device_sample_rate,
        device_channels,
    );
    (r1.max(r2), f1.max(f2))
}

/// Render four streaming stems.
fn render_streaming_four_stem(
    output: &mut [f32],
    vocals: &mut crate::audio::streaming::AudioConsumer,
    drums: &mut crate::audio::streaming::AudioConsumer,
    bass: &mut crate::audio::streaming::AudioConsumer,
    other: &mut crate::audio::streaming::AudioConsumer,
    master: f32,
    sv: StemVolumes,
    device_sample_rate: u32,
    device_channels: usize,
) -> (usize, u64) {
    let (r1, f1) = render_streaming_single(
        output,
        vocals,
        master * sv.vocals,
        device_sample_rate,
        device_channels,
    );
    let (r2, f2) = render_streaming_single(
        output,
        drums,
        master * sv.drums,
        device_sample_rate,
        device_channels,
    );
    let (r3, f3) = render_streaming_single(
        output,
        bass,
        master * sv.bass,
        device_sample_rate,
        device_channels,
    );
    let (r4, f4) = render_streaming_single(
        output,
        other,
        master * sv.other,
        device_sample_rate,
        device_channels,
    );
    (r1.max(r2).max(r3).max(r4), f1.max(f2).max(f3).max(f4))
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
                    rendered_samples =
                        render_output_buffer(&mut controller, &mut scratch, sample_rate, channels);
                } else {
                    scratch.fill(0.0);
                }

                forward_rendered_audio_to_airplay(
                    rendered_samples,
                    &scratch,
                    channels,
                    sample_rate,
                    &airplay_audio_tap,
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
) {
    if rendered_samples == 0 {
        return;
    }

    let rendered_samples = rendered_samples.min(scratch.len());
    if rendered_samples == 0 {
        return;
    }

    let tap_samples = downmix_for_airplay(&scratch[..rendered_samples], channels);
    if !tap_samples.is_empty() {
        airplay_audio_tap.push_interleaved(sample_rate, 2, &tap_samples);
    }
}

fn downmix_for_airplay(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels == 0 || samples.is_empty() {
        return Vec::new();
    }

    let mut stereo = Vec::with_capacity((samples.len() / channels).saturating_mul(2));
    for frame in samples.chunks(channels) {
        let (left, right) = match channels {
            1 => (frame[0], frame[0]),
            2 => (frame[0], frame[1]),
            _ => {
                // For multi-channel audio (e.g., 5.1), mix down to stereo.
                // Standard downmix: L = FL + 0.707*C + 0.707*SL, R = FR + 0.707*C + 0.707*SR
                // Without knowing exact layout, average all channels into left and right.
                let sum: f32 = frame.iter().sum();
                let avg = sum / channels as f32;
                (avg, avg)
            }
        };
        stereo.push(left);
        stereo.push(right);
    }

    stereo
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
    use super::{forward_rendered_audio_to_airplay, write_output_samples};
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
        forward_rendered_audio_to_airplay(0, &[0.8, 0.7, 0.6, 0.5], 2, 44_100, &tap);

        assert!(tap.drain_pending().is_empty());
    }

    #[test]
    fn forward_rendered_audio_to_airplay_limits_payload_to_rendered_samples() {
        let tap = AirPlayAudioTap::new(4);
        forward_rendered_audio_to_airplay(4, &[0.1, 0.2, 0.3, 0.4, 0.9, 0.8], 2, 44_100, &tap);

        let drained = tap.drain_pending();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].samples, vec![0.1, 0.2, 0.3, 0.4]);
    }
}
