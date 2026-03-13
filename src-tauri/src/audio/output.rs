use crate::audio::playback::{monotonic_now_ms, PlaybackController};
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

    let Some(decoded_audio) = playback.loaded_audio() else {
        return 0;
    };

    let start_frame = (snapshot.position_ms * decoded_audio.sample_rate as u64 / 1000) as usize;
    let start_sample = start_frame * decoded_audio.channels;
    let available_samples = decoded_audio.samples.len().saturating_sub(start_sample);
    let rendered_samples = available_samples.min(output.len());
    let volume = snapshot.volume;

    for (index, sample) in output.iter_mut().enumerate().take(rendered_samples) {
        *sample = decoded_audio.samples[start_sample + index] * volume;
    }

    rendered_samples
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
