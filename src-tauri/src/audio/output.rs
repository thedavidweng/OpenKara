use crate::audio::decode::DecodedAudio;
use crate::audio::playback::{monotonic_now_ms, LoadedStems, PlaybackController};
use anyhow::{Context, Result};
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
) -> Result<()> {
    if started.load(Ordering::SeqCst) {
        return Ok(());
    }

    let _guard = start_lock
        .lock()
        .map_err(|_| anyhow::anyhow!("audio output start lock was poisoned"))?;
    if started.load(Ordering::SeqCst) {
        return Ok(());
    }

    let (startup_tx, startup_rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        if let Err(error) = start_output_thread(playback, startup_tx) {
            eprintln!("audio output thread failed to start: {error:#}");
        }
    });

    startup_rx
        .recv_timeout(Duration::from_secs(5))
        .context("timed out while waiting for audio output thread startup")??;
    started.store(true, Ordering::SeqCst);

    Ok(())
}

pub fn render_output_buffer(
    playback: &mut PlaybackController,
    now_ms: u64,
    output: &mut [f32],
) -> usize {
    output.fill(0.0);

    let snapshot = playback.snapshot(now_ms);
    if !snapshot.is_playing {
        return 0;
    }

    let Some(track) = &playback.current_track else {
        return 0;
    };

    let master = snapshot.volume;
    let sv = snapshot.stem_volumes;

    if let Some(loaded_stems) = &track.stems {
        let rendered = match loaded_stems {
            LoadedStems::TwoStem { vocals, accompaniment } => {
                let sample_rate = vocals.sample_rate as u64;
                let start_frame = (snapshot.position_ms * sample_rate / 1000) as usize;
                // In 2-stem mode, use drums volume as the accompaniment volume
                // (the frontend sets drums/bass/other to the same value when collapsed)
                let accomp_gain = sv.drums;
                let mut rendered = 0;
                rendered = rendered.max(mix_stem_into(output, vocals, start_frame, master * sv.vocals));
                rendered = rendered.max(mix_stem_into(output, accompaniment, start_frame, master * accomp_gain));
                rendered
            }
            LoadedStems::FourStem(stems) => {
                let sample_rate = stems.vocals.sample_rate as u64;
                let start_frame = (snapshot.position_ms * sample_rate / 1000) as usize;
                let mut rendered = 0;
                rendered = rendered.max(mix_stem_into(output, &stems.vocals, start_frame, master * sv.vocals));
                rendered = rendered.max(mix_stem_into(output, &stems.drums, start_frame, master * sv.drums));
                rendered = rendered.max(mix_stem_into(output, &stems.bass, start_frame, master * sv.bass));
                rendered = rendered.max(mix_stem_into(output, &stems.other, start_frame, master * sv.other));
                rendered
            }
        };

        // Clamp to prevent clipping
        for sample in output.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }

        rendered
    } else {
        // Fallback: play original audio with master volume
        let original = &track.original_audio;
        let start_frame =
            (snapshot.position_ms * original.sample_rate as u64 / 1000) as usize;
        let start_sample = start_frame * original.channels;
        let available = original.samples.len().saturating_sub(start_sample);
        let count = available.min(output.len());

        for i in 0..count {
            output[i] = original.samples[start_sample + i] * master;
        }

        count
    }
}

fn mix_stem_into(
    output: &mut [f32],
    audio: &DecodedAudio,
    start_frame: usize,
    gain: f32,
) -> usize {
    if gain == 0.0 {
        return 0;
    }
    let start_sample = start_frame * audio.channels;
    let available = audio.samples.len().saturating_sub(start_sample);
    let count = available.min(output.len());

    for i in 0..count {
        output[i] += audio.samples[start_sample + i] * gain;
    }

    count
}

fn build_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    playback: Arc<Mutex<PlaybackController>>,
) -> Result<Stream>
where
    T: SizedSample + Sample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _info| {
            let mut scratch = vec![0.0_f32; data.len()];

            if let Ok(mut controller) = playback.lock() {
                let rendered_samples =
                    render_output_buffer(&mut controller, monotonic_now_ms(), &mut scratch);

                if rendered_samples < scratch.len() {
                    scratch[rendered_samples..].fill(0.0);
                }
            } else {
                scratch.fill(0.0);
            }

            for frame in scratch.chunks(channels).zip(data.chunks_mut(channels)) {
                let (input_frame, output_frame) = frame;
                for (input_sample, output_sample) in input_frame.iter().zip(output_frame.iter_mut())
                {
                    *output_sample = T::from_sample(*input_sample);
                }
            }
        },
        move |error| {
            eprintln!("audio output stream error: {error}");
        },
        None,
    )?;

    Ok(stream)
}

fn start_output_thread(
    playback: Arc<Mutex<PlaybackController>>,
    startup_tx: mpsc::SyncSender<Result<()>>,
) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("no default output audio device is available")?;
    let config = device
        .default_output_config()
        .context("failed to read default audio output config")?;
    let stream = match config.sample_format() {
        SampleFormat::F32 => build_output_stream::<f32>(&device, &config.into(), playback)?,
        SampleFormat::I16 => build_output_stream::<i16>(&device, &config.into(), playback)?,
        SampleFormat::U16 => build_output_stream::<u16>(&device, &config.into(), playback)?,
        sample_format => {
            anyhow::bail!("unsupported audio output sample format: {sample_format:?}");
        }
    };

    stream
        .play()
        .context("failed to start audio output stream")?;
    let _ = startup_tx.send(Ok(()));

    loop {
        thread::sleep(Duration::from_secs(60));
        let _keep_alive = &stream;
    }
}
