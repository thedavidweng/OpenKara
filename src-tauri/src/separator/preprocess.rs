use crate::{audio::decode::DecodedAudio, separator::model::LoadedModel};
use anyhow::{bail, Context, Result};
use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler, WindowFunction};

pub const DEMUCS_SAMPLE_RATE: u32 = 44_100;
pub const DEMUCS_CHANNELS: usize = 2;

/// The spectral-core inference window, fixed by the model's verified session
/// interface. The former waveform rank-3 input fallback was removed with the
/// waveform production path (issue #172); every loaded model is spectral-core,
/// so the window is always the contract segment length.
pub fn target_frame_count(model: &LoadedModel, _fallback_frame_count: usize) -> Result<usize> {
    Ok(model.spectral.segment_frames)
}

/// Takes ownership of the decoded audio buffer to avoid holding two
/// full-song PCM copies in memory simultaneously. If resampling is needed, the
/// original buffer is consumed and replaced; otherwise it is returned as-is.
pub fn normalize_audio_for_model(decoded_audio: DecodedAudio) -> Result<DecodedAudio> {
    if decoded_audio.channels != DEMUCS_CHANNELS {
        bail!(
            "Demucs preprocessing currently requires stereo audio, got {} channels",
            decoded_audio.channels
        );
    }

    if decoded_audio.sample_rate == DEMUCS_SAMPLE_RATE {
        return Ok(decoded_audio);
    }

    let frame_count = decoded_audio.samples.len() / decoded_audio.channels;
    let input_adapter =
        InterleavedSlice::new(&decoded_audio.samples, decoded_audio.channels, frame_count)
            .context("failed to wrap interleaved audio for resampling")?;
    let mut resampler = Fft::<f32>::new_custom(
        decoded_audio.sample_rate as usize,
        DEMUCS_SAMPLE_RATE as usize,
        1024,
        2,
        decoded_audio.channels,
        WindowFunction::BlackmanHarris2,
        FixedSync::Both,
    )
    .with_context(|| {
        format!(
            "failed to create resampler from {} Hz to {} Hz",
            decoded_audio.sample_rate, DEMUCS_SAMPLE_RATE
        )
    })?;
    let output_frame_capacity = resampler.process_all_needed_output_len(frame_count);
    let mut output_samples = vec![0.0_f32; output_frame_capacity * decoded_audio.channels];
    let mut output_adapter = InterleavedSlice::new_mut(
        &mut output_samples,
        decoded_audio.channels,
        output_frame_capacity,
    )
    .context("failed to prepare output buffer for resampling")?;
    let (_, output_frames) = resampler
        .process_all_into_buffer(&input_adapter, &mut output_adapter, frame_count, None)
        .context("failed while resampling audio for Demucs preprocessing")?;
    output_samples.truncate(output_frames * decoded_audio.channels);

    Ok(DecodedAudio {
        sample_rate: DEMUCS_SAMPLE_RATE,
        channels: decoded_audio.channels,
        duration_ms: ((output_frames as f64 / DEMUCS_SAMPLE_RATE as f64) * 1000.0).round() as u64,
        samples: output_samples,
    })
}
