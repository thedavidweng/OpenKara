use crate::{audio::decode::DecodedAudio, separator::model::LoadedModel};
use anyhow::{bail, Result};

pub const DEMUCS_SAMPLE_RATE: u32 = 44_100;
pub const DEMUCS_CHANNELS: usize = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedModelInput {
    pub shape: Vec<i64>,
    pub samples: Vec<f32>,
}

pub fn prepare_model_input(
    model: &LoadedModel,
    decoded_audio: &DecodedAudio,
) -> Result<PreparedModelInput> {
    if decoded_audio.sample_rate != DEMUCS_SAMPLE_RATE {
        bail!(
            "Demucs preprocessing currently requires 44.1 kHz audio, got {} Hz",
            decoded_audio.sample_rate
        );
    }

    if decoded_audio.channels != DEMUCS_CHANNELS {
        bail!(
            "Demucs preprocessing currently requires stereo audio, got {} channels",
            decoded_audio.channels
        );
    }

    if model.input_shape.len() != 3 {
        bail!(
            "Demucs model input rank must be 3, got {} dimensions",
            model.input_shape.len()
        );
    }

    let frame_count = decoded_audio.samples.len() / decoded_audio.channels;
    let target_frame_count = match model.input_shape.get(2).copied() {
        Some(frame_count_hint) if frame_count_hint > 0 => frame_count_hint as usize,
        _ => frame_count,
    };

    if frame_count > target_frame_count {
        bail!(
            "Demucs preprocessing currently supports audio up to {target_frame_count} frames, got {frame_count}"
        );
    }

    let mut channels_first = vec![0.0_f32; decoded_audio.channels * target_frame_count];

    for frame_index in 0..frame_count {
        let interleaved_offset = frame_index * decoded_audio.channels;
        for channel_index in 0..decoded_audio.channels {
            let channels_first_offset = channel_index * target_frame_count + frame_index;
            channels_first[channels_first_offset] =
                decoded_audio.samples[interleaved_offset + channel_index];
        }
    }

    Ok(PreparedModelInput {
        shape: vec![1, decoded_audio.channels as i64, target_frame_count as i64],
        samples: channels_first,
    })
}
