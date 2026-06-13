use crate::{
    audio::decode::DecodedAudio,
    separator::{checkpoint, model::LoadedModel, preprocess},
};
use anyhow::{bail, Context, Result};
use ort::value::Tensor;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

pub const DEMUCS_STEM_NAMES: [&str; 4] = ["drums", "bass", "other", "vocals"];

#[derive(Debug, Clone, PartialEq)]
pub struct SeparatedStem {
    pub name: String,
    pub audio: DecodedAudio,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeparationResult {
    pub stems: Vec<SeparatedStem>,
}

/// Item 3: Takes `&LoadedModel` (not `&mut`) so the model cache lock can be
/// released before calling this function. The ONNX session is thread-safe and
/// only requires shared access for `session.run()`.
/// Item 3: Takes `&LoadedModel` (not `&mut`) so the model cache lock can be
/// released before calling this function. The ONNX session is thread-safe and
/// only requires shared access for `session.run()`.
///
/// Item 4: Takes ownership of `decoded_audio` so the original buffer is
/// consumed during normalization, preventing two full-song PCM copies.
pub fn separate_audio(
    model: &LoadedModel,
    decoded_audio: DecodedAudio,
    mut on_chunk_complete: impl FnMut(usize, usize),
    checkpoint_dir: Option<&Path>,
    song_hash: &str,
) -> Result<SeparationResult> {
    let normalized_audio = preprocess::normalize_audio_for_model(decoded_audio)?;
    let input_frame_count = normalized_audio.samples.len() / normalized_audio.channels;
    let target_frame_count = preprocess::target_frame_count(model, input_frame_count)?;

    if input_frame_count > target_frame_count {
        return separate_chunked_audio(
            model,
            &normalized_audio,
            target_frame_count,
            &mut on_chunk_complete,
            checkpoint_dir,
            song_hash,
        );
    }

    let result = separate_window_audio(model, &normalized_audio, input_frame_count)?;
    on_chunk_complete(1, 1);
    Ok(result)
}

fn separate_window_audio(
    model: &LoadedModel,
    decoded_audio: &DecodedAudio,
    trim_frame_count: usize,
) -> Result<SeparationResult> {
    let prepared_input = preprocess::prepare_model_input_from_normalized(model, decoded_audio)?;
    let session_inputs = build_session_inputs(model, decoded_audio, prepared_input)
        .context("failed to prepare Demucs inputs")?;
    let mut session_guard = model
        .session
        .lock()
        .map_err(|_| anyhow::anyhow!("ONNX session lock was poisoned"))?;
    let outputs = session_guard
        .run(session_inputs)
        .context("failed to run Demucs inference")?;

    if outputs.len() == 0 {
        bail!("Demucs inference returned no output tensors");
    }

    for (_, output_value) in outputs.iter() {
        let dims = tensor_dims(&output_value)?;
        if looks_like_stacked_stem_output(&dims, decoded_audio.channels) {
            let stems = stems_from_stacked_output(&output_value, decoded_audio, trim_frame_count)?;
            return Ok(SeparationResult { stems });
        }
    }

    if outputs.len() >= DEMUCS_STEM_NAMES.len() {
        let matching_outputs = outputs
            .iter()
            .filter(|(_, output_value)| {
                tensor_dims(output_value)
                    .map(|dims| looks_like_single_stem_output(&dims, decoded_audio.channels))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();

        if matching_outputs.len() == DEMUCS_STEM_NAMES.len() {
            let mut stems = Vec::with_capacity(DEMUCS_STEM_NAMES.len());
            for (stem_name, (_, output_value)) in DEMUCS_STEM_NAMES.iter().zip(matching_outputs) {
                stems.push(stem_from_single_output(
                    stem_name,
                    &output_value,
                    decoded_audio,
                    trim_frame_count,
                )?);
            }
            return Ok(SeparationResult { stems });
        }
    }

    let output_shapes = outputs
        .iter()
        .map(|(name, output_value)| {
            let dims = tensor_dims(&output_value)
                .map(|dims| format!("{dims:?}"))
                .unwrap_or_else(|error| format!("unreadable ({error:#})"));
            format!("{name}: {dims}")
        })
        .collect::<Vec<_>>();
    bail!(
        "Demucs inference did not expose a final stem output; saw {}",
        output_shapes.join(", ")
    )
}

/// Item 5: Generate a Hann window of the given size for overlap-add processing.
/// Sine window satisfying the squared constant-overlap-add constraint at 50%
/// overlap: w[n]^2 + w[n + N/2]^2 = 1.
///
/// This is equivalent to `sqrt(hann)` and is the standard choice for
/// overlap-add processing where chunks are windowed, processed, then
/// overlap-added with the same window (squared normalization).
fn hann_window(size: usize) -> Vec<f32> {
    (0..size)
        .map(|i| {
            let phase = std::f64::consts::TAU * i as f64 / size as f64;
            (0.5 * (1.0 - phase.cos())).sqrt() as f32
        })
        .collect()
}

fn separate_chunked_audio(
    model: &LoadedModel,
    decoded_audio: &DecodedAudio,
    target_frame_count: usize,
    on_chunk_complete: &mut impl FnMut(usize, usize),
    checkpoint_dir: Option<&Path>,
    song_hash: &str,
) -> Result<SeparationResult> {
    let channels = decoded_audio.channels;
    let input_frame_count = decoded_audio.samples.len() / channels;

    // Item 5: 50% overlap with Hann window for smooth chunk boundaries.
    let hop_size = target_frame_count / 2;
    let total_chunks = if input_frame_count <= target_frame_count {
        1
    } else {
        (input_frame_count - target_frame_count).div_ceil(hop_size) + 1
    };

    // Write checkpoint manifest and discover already-completed chunks.
    let completed_set: HashSet<usize> = if let Some(dir) = checkpoint_dir {
        let manifest = checkpoint::CheckpointManifest {
            song_hash: song_hash.to_string(),
            total_chunks,
            target_frame_count,
            input_frame_count,
            channels,
            sample_rate: decoded_audio.sample_rate,
            stem_count: DEMUCS_STEM_NAMES.len(),
        };
        checkpoint::write_manifest(dir, &manifest)?;
        checkpoint::list_completed_chunks(dir)?
            .into_iter()
            .collect()
    } else {
        HashSet::new()
    };

    let output_sample_count = decoded_audio.samples.len();
    let mut merged_stems = DEMUCS_STEM_NAMES
        .iter()
        .map(|stem_name| SeparatedStem {
            name: (*stem_name).to_string(),
            audio: DecodedAudio {
                sample_rate: decoded_audio.sample_rate,
                channels,
                duration_ms: decoded_audio.duration_ms,
                samples: vec![0.0_f32; output_sample_count],
            },
        })
        .collect::<Vec<_>>();

    // Overlap-add normalization buffer: tracks the sum of squared Hann
    // windows at each sample position for proper normalization.
    let mut overlap_norm = vec![0.0_f32; output_sample_count];

    // Restore already-completed chunks from checkpoint.
    if let Some(dir) = checkpoint_dir {
        for &completed_idx in &completed_set {
            let chunk_start_frame = completed_idx * hop_size;
            if chunk_start_frame >= input_frame_count {
                continue;
            }
            let chunk_frame_count = (input_frame_count - chunk_start_frame).min(target_frame_count);
            let chunk_data = checkpoint::read_chunk(dir, completed_idx)?;
            let window = hann_window(target_frame_count);
            let samples_per_stem = chunk_frame_count * channels;
            for (stem_idx, stem) in merged_stems.iter_mut().enumerate() {
                let src_offset = stem_idx * samples_per_stem;
                let dst_start = chunk_start_frame * channels;
                for (frame, &w) in window.iter().take(chunk_frame_count).enumerate() {
                    for ch in 0..channels {
                        let src_idx = src_offset + frame * channels + ch;
                        let dst_idx = dst_start + frame * channels + ch;
                        stem.audio.samples[dst_idx] += chunk_data[src_idx] * w * w;
                    }
                }
            }
            // Update normalization.
            for (frame, &w) in window.iter().take(chunk_frame_count).enumerate() {
                let w2 = w * w;
                let base = (chunk_start_frame + frame) * channels;
                for ch in 0..channels {
                    overlap_norm[base + ch] += w2;
                }
            }
        }
    }

    // Item 5: Process chunks with 50% overlap and Hann windowing.
    let window = hann_window(target_frame_count);
    let mut chunk_index = 0_usize;
    for chunk_start_frame in (0..input_frame_count).step_by(hop_size) {
        let chunk_frame_count = (input_frame_count - chunk_start_frame).min(target_frame_count);

        // Skip chunks that were already completed in a previous run.
        if completed_set.contains(&chunk_index) {
            chunk_index += 1;
            on_chunk_complete(chunk_index, total_chunks);
            continue;
        }

        let chunk_audio = build_chunk_audio(
            decoded_audio,
            chunk_start_frame,
            chunk_frame_count,
            target_frame_count,
        );
        let chunk_result = separate_window_audio(model, &chunk_audio, chunk_frame_count)
            .with_context(|| {
                format!("failed to separate chunk starting at frame {chunk_start_frame}")
            })?;

        // Item 5: Apply Hann window and overlap-add into merged output.
        for (stem_index, chunk_stem) in chunk_result.stems.iter().enumerate() {
            let destination = &mut merged_stems[stem_index].audio.samples;
            let dst_start = chunk_start_frame * channels;
            for (frame, &w) in window.iter().take(chunk_frame_count).enumerate() {
                for ch in 0..channels {
                    let src_idx = frame * channels + ch;
                    let dst_idx = dst_start + frame * channels + ch;
                    destination[dst_idx] += chunk_stem.audio.samples[src_idx] * w * w;
                }
            }
        }

        // Update normalization buffer.
        for (frame, &w) in window.iter().take(chunk_frame_count).enumerate() {
            let w2 = w * w;
            let base = (chunk_start_frame + frame) * channels;
            for ch in 0..channels {
                overlap_norm[base + ch] += w2;
            }
        }

        // Persist the chunk to disk for crash recovery.
        if let Some(dir) = checkpoint_dir {
            let chunk_sample_count = chunk_frame_count * channels;
            let mut chunk_data = Vec::with_capacity(chunk_sample_count * DEMUCS_STEM_NAMES.len());
            for stem in &chunk_result.stems {
                chunk_data.extend_from_slice(&stem.audio.samples[..chunk_sample_count]);
            }
            checkpoint::write_chunk(dir, chunk_index, &chunk_data)?;
        }

        chunk_index += 1;
        on_chunk_complete(chunk_index, total_chunks);
    }

    // Normalize the overlap-add output to compensate for Hann window weighting.
    for stem in merged_stems.iter_mut() {
        for (i, sample) in stem.audio.samples.iter_mut().enumerate() {
            let norm = overlap_norm[i];
            if norm > 1e-8 {
                *sample /= norm;
            }
        }
    }

    Ok(SeparationResult {
        stems: merged_stems,
    })
}

pub fn write_stems_to_directory(
    separation: &SeparationResult,
    output_directory: &Path,
) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(output_directory).with_context(|| {
        format!(
            "failed to create stem output directory at {}",
            output_directory.display()
        )
    })?;

    let mut written_paths = Vec::with_capacity(separation.stems.len());
    for stem in &separation.stems {
        let output_path = output_directory.join(format!("{}.ogg", stem.name));
        crate::audio::encode::write_ogg_file(&output_path, &stem.audio)?;
        written_paths.push(output_path);
    }

    Ok(written_paths)
}

fn stems_from_stacked_output(
    output_value: &ort::value::DynValue,
    decoded_audio: &DecodedAudio,
    input_frame_count: usize,
) -> Result<Vec<SeparatedStem>> {
    let (shape, data) = output_value
        .try_extract_tensor::<f32>()
        .context("Demucs stacked output tensor was not f32")?;
    let dims: Vec<i64> = shape.iter().copied().collect();
    let (stem_count, channel_count, output_frame_count) = match dims.as_slice() {
        [stem_count, channel_count, frame_count] => (
            usize_from_dim(*stem_count, "stem count")?,
            usize_from_dim(*channel_count, "channel count")?,
            usize_from_dim(*frame_count, "frame count")?,
        ),
        [batch_size, stem_count, channel_count, frame_count] => {
            let batch_size = usize_from_dim(*batch_size, "batch size")?;
            if batch_size != 1 {
                bail!("Demucs stacked output batch size must be 1, got {batch_size}");
            }
            (
                usize_from_dim(*stem_count, "stem count")?,
                usize_from_dim(*channel_count, "channel count")?,
                usize_from_dim(*frame_count, "frame count")?,
            )
        }
        _ => {
            bail!(
                "Demucs stacked output rank must be 3 or 4, got {} dimensions",
                dims.len()
            )
        }
    };

    if stem_count != DEMUCS_STEM_NAMES.len() {
        bail!(
            "Demucs stacked output must contain {} stems, got {stem_count}",
            DEMUCS_STEM_NAMES.len()
        );
    }

    if channel_count != decoded_audio.channels {
        bail!(
            "Demucs stacked output must contain {} channels, got {channel_count}",
            decoded_audio.channels
        );
    }

    if output_frame_count < input_frame_count {
        bail!(
            "Demucs stacked output contained {output_frame_count} frames, fewer than the input {input_frame_count}"
        );
    }

    let source_stride = channel_count * output_frame_count;
    let mut stems = Vec::with_capacity(stem_count);

    // Demucs exports the four standard source stems in fixed order.
    for (stem_index, stem_name) in DEMUCS_STEM_NAMES.iter().enumerate() {
        let source_offset = stem_index * source_stride;
        let source_data = &data[source_offset..source_offset + source_stride];
        stems.push(build_stem_from_channels_first(
            stem_name,
            source_data,
            output_frame_count,
            decoded_audio,
            input_frame_count,
        )?);
    }

    Ok(stems)
}

fn stem_from_single_output(
    stem_name: &str,
    output_value: &ort::value::DynValue,
    decoded_audio: &DecodedAudio,
    input_frame_count: usize,
) -> Result<SeparatedStem> {
    let (shape, data) = output_value
        .try_extract_tensor::<f32>()
        .with_context(|| format!("Demucs output tensor for {stem_name} was not f32"))?;
    let dims: Vec<i64> = shape.iter().copied().collect();
    let (channel_count, output_frame_count) = match dims.as_slice() {
        [channel_count, frame_count] => (
            usize_from_dim(*channel_count, "channel count")?,
            usize_from_dim(*frame_count, "frame count")?,
        ),
        [batch_size, channel_count, frame_count] => {
            let batch_size = usize_from_dim(*batch_size, "batch size")?;
            if batch_size != 1 {
                bail!(
                    "Demucs output tensor for {stem_name} must have batch size 1, got {batch_size}"
                );
            }
            (
                usize_from_dim(*channel_count, "channel count")?,
                usize_from_dim(*frame_count, "frame count")?,
            )
        }
        _ => {
            bail!(
                "Demucs output tensor for {stem_name} must have rank 2 or 3, got {} dimensions",
                dims.len()
            )
        }
    };

    if channel_count != decoded_audio.channels {
        bail!(
            "Demucs output tensor for {stem_name} must contain {} channels, got {channel_count}",
            decoded_audio.channels
        );
    }

    if output_frame_count < input_frame_count {
        bail!(
            "Demucs output tensor for {stem_name} contained {output_frame_count} frames, fewer than the input {input_frame_count}"
        );
    }

    build_stem_from_channels_first(
        stem_name,
        data,
        output_frame_count,
        decoded_audio,
        input_frame_count,
    )
}

fn build_session_inputs(
    model: &LoadedModel,
    decoded_audio: &DecodedAudio,
    prepared_input: preprocess::PreparedModelInput,
) -> Result<Vec<(String, Tensor<f32>)>> {
    let session = model
        .session
        .lock()
        .map_err(|_| anyhow::anyhow!("ONNX session lock was poisoned"))?;
    let mut session_inputs = Vec::with_capacity(session.inputs().len());
    let expected_audio_shape = prepared_input.shape.clone();
    let preprocess::PreparedModelInput {
        shape: audio_shape,
        samples: audio_samples,
    } = prepared_input;

    for input in session.inputs() {
        let input_shape = input
            .dtype()
            .tensor_shape()
            .with_context(|| format!("Demucs input {} is not a tensor", input.name()))?;
        let dims: Vec<i64> = input_shape.iter().copied().collect();
        let tensor = if looks_like_audio_input(&dims, decoded_audio.channels) {
            if dims != expected_audio_shape {
                bail!(
                    "Demucs audio input {} expected shape {:?}, prepared shape was {:?}",
                    input.name(),
                    dims,
                    expected_audio_shape
                );
            }

            Tensor::<f32>::from_array((audio_shape.clone(), audio_samples.clone())).with_context(
                || {
                    format!(
                        "failed to build Demucs audio input tensor for {}",
                        input.name()
                    )
                },
            )?
        } else {
            let zero_count = num_elements_for_dims(&dims).with_context(|| {
                format!(
                    "Demucs auxiliary input {} has unsupported shape {:?}",
                    input.name(),
                    dims
                )
            })?;
            let zero_tensor = vec![0.0_f32; zero_count];
            Tensor::<f32>::from_array((dims.clone(), zero_tensor)).with_context(|| {
                format!(
                    "failed to build zero tensor for Demucs auxiliary input {}",
                    input.name()
                )
            })?
        };

        session_inputs.push((input.name().to_owned(), tensor));
    }

    Ok(session_inputs)
}

fn build_stem_from_channels_first(
    stem_name: &str,
    channels_first_samples: &[f32],
    output_frame_count: usize,
    decoded_audio: &DecodedAudio,
    input_frame_count: usize,
) -> Result<SeparatedStem> {
    let channel_count = decoded_audio.channels;
    let expected_sample_count = channel_count * output_frame_count;
    if channels_first_samples.len() < expected_sample_count {
        bail!(
            "Demucs output for {stem_name} contained {} samples, fewer than expected {expected_sample_count}",
            channels_first_samples.len()
        );
    }

    let mut interleaved_samples = vec![0.0_f32; channel_count * input_frame_count];
    for frame_index in 0..input_frame_count {
        for channel_index in 0..channel_count {
            let source_offset = channel_index * output_frame_count + frame_index;
            let interleaved_offset = frame_index * channel_count + channel_index;
            interleaved_samples[interleaved_offset] = channels_first_samples[source_offset];
        }
    }

    Ok(SeparatedStem {
        name: stem_name.to_owned(),
        audio: DecodedAudio {
            sample_rate: decoded_audio.sample_rate,
            channels: channel_count,
            duration_ms: decoded_audio.duration_ms,
            samples: interleaved_samples,
        },
    })
}

fn build_chunk_audio(
    decoded_audio: &DecodedAudio,
    chunk_start_frame: usize,
    chunk_frame_count: usize,
    target_frame_count: usize,
) -> DecodedAudio {
    let channels = decoded_audio.channels;
    let chunk_start_sample = chunk_start_frame * channels;
    let chunk_end_sample = chunk_start_sample + chunk_frame_count * channels;
    let mut padded_samples = vec![0.0_f32; target_frame_count * channels];
    padded_samples[..chunk_frame_count * channels]
        .copy_from_slice(&decoded_audio.samples[chunk_start_sample..chunk_end_sample]);

    DecodedAudio {
        sample_rate: decoded_audio.sample_rate,
        channels,
        duration_ms: ((chunk_frame_count as f64 / decoded_audio.sample_rate as f64) * 1000.0)
            .round() as u64,
        samples: padded_samples,
    }
}

fn usize_from_dim(value: i64, label: &str) -> Result<usize> {
    usize::try_from(value)
        .with_context(|| format!("Demucs {label} dimension must be non-negative, got {value}"))
}

fn num_elements_for_dims(dims: &[i64]) -> Result<usize> {
    dims.iter().try_fold(1_usize, |accumulator, dim| {
        let dimension = usize_from_dim(*dim, "tensor")?;
        accumulator
            .checked_mul(dimension)
            .context("Demucs tensor element count overflowed usize")
    })
}

fn looks_like_audio_input(dims: &[i64], channel_count: usize) -> bool {
    matches!(dims, [1, channels, frame_count] if *channels == channel_count as i64 && *frame_count > 0)
}

fn looks_like_stacked_stem_output(dims: &[i64], channel_count: usize) -> bool {
    matches!(dims, [stem_count, channels, frame_count] if *stem_count == DEMUCS_STEM_NAMES.len() as i64 && *channels == channel_count as i64 && *frame_count > 0)
        || matches!(dims, [1, stem_count, channels, frame_count] if *stem_count == DEMUCS_STEM_NAMES.len() as i64 && *channels == channel_count as i64 && *frame_count > 0)
}

fn looks_like_single_stem_output(dims: &[i64], channel_count: usize) -> bool {
    matches!(dims, [channels, frame_count] if *channels == channel_count as i64 && *frame_count > 0)
        || matches!(dims, [1, channels, frame_count] if *channels == channel_count as i64 && *frame_count > 0)
}

fn tensor_dims(output_value: &ort::value::DynValue) -> Result<Vec<i64>> {
    let (shape, _) = output_value
        .try_extract_tensor::<f32>()
        .context("Demucs output tensor was not readable as f32")?;
    Ok(shape.iter().copied().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Item 5: Hann window should have correct size and boundary values.
    #[test]
    fn hann_window_has_correct_properties() {
        let n = 1024;
        let w = hann_window(n);

        assert_eq!(w.len(), n);
        // First value is exactly 0; last value is small but non-zero
        // for the periodic form (≈π/N for large N).
        assert!(w[0] < 1e-6, "Hann window should start at 0");
        assert!(w[n - 1] < 0.01, "Hann window should end near 0");
        assert!(
            w[n - 1] > 1e-6,
            "Hann window end should be non-zero (periodic form)"
        );
        // Middle value should be 1.0.
        assert!(
            (w[n / 2] - 1.0).abs() < 1e-6,
            "Hann window should peak at 1.0"
        );
    }

    /// Item 5: Hann window squared + shifted by 50% should sum to 1.0
    /// (constant overlap-add constraint).
    #[test]
    fn hann_window_constant_overlap_add_constraint() {
        let n = 1024;
        let w = hann_window(n);
        let hop = n / 2;

        for i in 0..hop {
            let sum = w[i] * w[i] + w[i + hop] * w[i + hop];
            assert!(
                (sum - 1.0).abs() < 1e-5,
                "w[{i}]^2 + w[{}]^2 = {sum}, expected 1.0",
                i + hop,
            );
        }
    }

    /// Item 5: Verify that overlap-add with Hann window produces a smooth
    /// reconstruction of a constant signal.
    #[test]
    fn overlap_add_reconstructs_constant_signal() {
        let channels = 2;
        let target_frames = 256;
        let input_frames = target_frames * 3; // 3 non-overlapping would have seams

        // Create a constant input signal.
        let input_samples = vec![1.0_f32; input_frames * channels];
        let decoded = DecodedAudio {
            sample_rate: 44100,
            channels,
            duration_ms: (input_frames as f64 / 44.1) as u64,
            samples: input_samples,
        };

        let hop_size = target_frames / 2;
        let window = hann_window(target_frames);
        let mut output = vec![0.0_f32; input_frames * channels];
        let mut norm = vec![0.0_f32; input_frames * channels];

        // Simulate overlap-add with 50% overlap and Hann window.
        for chunk_start in (0..input_frames).step_by(hop_size) {
            let chunk_frames = (input_frames - chunk_start).min(target_frames);
            for (frame, &w) in window.iter().take(chunk_frames).enumerate() {
                let w2 = w * w;
                for ch in 0..channels {
                    let idx = (chunk_start + frame) * channels + ch;
                    // "Inference" just passes through the input.
                    output[idx] += decoded.samples[idx] * w2;
                    norm[idx] += w2;
                }
            }
        }

        // Normalize.
        for (i, sample) in output.iter_mut().enumerate() {
            if norm[i] > 1e-8 {
                *sample /= norm[i];
            }
        }

        // Interior samples (where window norm > 0) should reconstruct to ~1.0.
        // Boundary samples at the very start/end where the Hann window is 0
        // cannot be reconstructed — that's expected, not a bug.
        for (i, (&sample, &n)) in output.iter().zip(norm.iter()).enumerate() {
            if n > 0.1 {
                assert!(
                    (sample - 1.0).abs() < 0.01,
                    "sample[{i}] = {sample}, expected ~1.0 (overlap-add should reconstruct constant signal)"
                );
            }
        }
    }
}
