use crate::airplay_stream::AirPlayAudioTap;
use crate::audio::crossfade::CROSSFADE_SCRATCH_FRAMES;
use crate::audio::eq::EqProcessor;
use crate::audio::error::PlaybackError;
use crate::audio::output_format::{OutputFormatSnapshot, OutputFormatState};
use crate::audio::peaks::{PeakAccumulator, PeakRing};
use crate::audio::playback::PlaybackController;
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

use super::{render_output_buffer, ResamplerCache};

const OUTPUT_DEVICE_POLL_INTERVAL: Duration = Duration::from_millis(200);
const OUTPUT_DEVICE_RETRY_INTERVAL: Duration = Duration::from_secs(1);

fn build_output_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    playback: Arc<Mutex<PlaybackController>>,
    airplay_audio_tap: Arc<AirPlayAudioTap>,
    airplay_local_output_suppressed: Arc<AtomicBool>,
    peak_ring: Arc<PeakRing>,
    output_format: OutputFormatState,
    device_lost: Arc<AtomicBool>,
) -> Result<Stream, PlaybackError>
where
    T: SizedSample + Sample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate;

    let generation = output_format
        .read()
        .ok()
        .and_then(|guard| *guard)
        .map(|s| s.generation.saturating_add(1))
        .unwrap_or(1);
    if let Ok(mut guard) = output_format.write() {
        *guard = Some(OutputFormatSnapshot::new(
            generation,
            sample_rate,
            config.channels,
        ));
    }
    let mut scratch = Vec::<f32>::new();
    let mut stem_scratch = Vec::<f32>::new();
    let mut mix_scratch = Vec::<f32>::new();
    let mut airplay_scratch = Vec::<f32>::new();
    let mut resampler_cache = ResamplerCache::new();
    // Separate resampler cache for the crossfade incoming lane.
    let mut crossfade_incoming_resampler_cache = ResamplerCache::new();
    let mut eq_processor = EqProcessor::new(sample_rate, channels);
    let mut peak_accumulator = PeakAccumulator::new();
    let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * channels];

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _info| {
                scratch.resize(data.len(), 0.0);

                // Never block the device callback: silence if the lock is held.
                let mut rendered_samples = 0;
                if let Ok(mut controller) = playback.try_lock() {
                    let eq_config = controller.eq_config();
                    if eq_config.revision != eq_processor.last_eq_revision() {
                        eq_processor.set_enabled(eq_config.enabled);
                        eq_processor.set_gains(eq_config.gains_db);
                        eq_processor.set_last_eq_revision(eq_config.revision);
                    }

                    rendered_samples = render_output_buffer(
                        &mut controller,
                        &mut scratch,
                        &mut stem_scratch,
                        &mut mix_scratch,
                        &mut crossfade_scratch,
                        sample_rate,
                        channels,
                        &mut resampler_cache,
                        &mut crossfade_incoming_resampler_cache,
                        &mut eq_processor,
                        &mut peak_accumulator,
                        &peak_ring,
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
                tracing::warn!("audio output stream error: {error}");
                device_lost.store(true, Ordering::SeqCst);
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

pub(super) fn start_output_thread(
    playback: Arc<Mutex<PlaybackController>>,
    airplay_audio_tap: Arc<AirPlayAudioTap>,
    airplay_local_output_suppressed: Arc<AtomicBool>,
    startup_tx: mpsc::SyncSender<Result<(), PlaybackError>>,
    shutdown: Arc<AtomicBool>,
    peak_ring: Arc<PeakRing>,
    output_format: OutputFormatState,
) -> Result<(), PlaybackError> {
    let mut startup_tx = Some(startup_tx);

    let mut rebuild_failure_logged = false;
    while !shutdown.load(Ordering::Relaxed) {
        let device_lost = Arc::new(AtomicBool::new(false));
        let stream = match open_output_stream(
            &playback,
            &airplay_audio_tap,
            &airplay_local_output_suppressed,
            &peak_ring,
            &output_format,
            &device_lost,
        ) {
            Ok(stream) => stream,
            Err(error) => {
                // First open failure is startup; later failures retry on replug.
                if let Some(tx) = startup_tx.take() {
                    let _ = tx.send(Err(PlaybackError::AudioOutputUnavailable(
                        error.to_string(),
                    )));
                    return Err(error);
                }
                if !rebuild_failure_logged {
                    tracing::warn!("audio output unavailable, retrying: {error}");
                    rebuild_failure_logged = true;
                }
                thread::sleep(OUTPUT_DEVICE_RETRY_INTERVAL);
                continue;
            }
        };
        rebuild_failure_logged = false;

        // Drop prepared/crossfade from the prior output-format generation.
        if let Ok(mut controller) = playback.try_lock() {
            controller.cancel_crossfade_and_prepared();
        }

        if let Err(error) = stream.play() {
            let error = PlaybackError::AudioOutputUnavailable(format!(
                "failed to start audio output stream: {error}"
            ));
            if let Some(tx) = startup_tx.take() {
                let _ = tx.send(Err(PlaybackError::AudioOutputUnavailable(
                    error.to_string(),
                )));
                return Err(error);
            }
            tracing::warn!("{error}");
            thread::sleep(OUTPUT_DEVICE_RETRY_INTERVAL);
            continue;
        }

        if let Some(tx) = startup_tx.take() {
            let _ = tx.send(Ok(()));
        }

        while !shutdown.load(Ordering::Relaxed) && !device_lost.load(Ordering::SeqCst) {
            thread::sleep(OUTPUT_DEVICE_POLL_INTERVAL);
        }

        if device_lost.load(Ordering::SeqCst) {
            tracing::warn!("audio output device went away; rebuilding the stream");
        }
        drop(stream);
    }

    Ok(())
}

fn open_output_stream(
    playback: &Arc<Mutex<PlaybackController>>,
    airplay_audio_tap: &Arc<AirPlayAudioTap>,
    airplay_local_output_suppressed: &Arc<AtomicBool>,
    peak_ring: &Arc<PeakRing>,
    output_format: &OutputFormatState,
    device_lost: &Arc<AtomicBool>,
) -> Result<Stream, PlaybackError> {
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

    match config.sample_format() {
        SampleFormat::F32 => build_output_stream::<f32>(
            &device,
            config.into(),
            playback.clone(),
            airplay_audio_tap.clone(),
            airplay_local_output_suppressed.clone(),
            peak_ring.clone(),
            output_format.clone(),
            device_lost.clone(),
        ),
        SampleFormat::I16 => build_output_stream::<i16>(
            &device,
            config.into(),
            playback.clone(),
            airplay_audio_tap.clone(),
            airplay_local_output_suppressed.clone(),
            peak_ring.clone(),
            output_format.clone(),
            device_lost.clone(),
        ),
        SampleFormat::U16 => build_output_stream::<u16>(
            &device,
            config.into(),
            playback.clone(),
            airplay_audio_tap.clone(),
            airplay_local_output_suppressed.clone(),
            peak_ring.clone(),
            output_format.clone(),
            device_lost.clone(),
        ),
        sample_format => Err(PlaybackError::AudioOutputUnavailable(format!(
            "unsupported audio output sample format: {sample_format:?}"
        ))),
    }
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
        let owned = std::mem::replace(
            airplay_scratch,
            Vec::with_capacity(airplay_scratch.capacity()),
        );
        airplay_audio_tap.push_interleaved(sample_rate, 2, owned);
    }
}

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
}
