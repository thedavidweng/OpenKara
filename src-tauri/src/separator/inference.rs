//! Streaming stem separation with bounded memory.
//!
//! The separation path uses a reusable workspace (`SeparationWorkspace`),
//! OLA ring buffers (`OlaRing`), and streaming OGG writers
//! (`StreamingOggWriter`) so that application memory is independent of
//! song duration for the additional output working set. One normalized
//! full-song input PCM buffer remains by design; finalized output frames
//! are flushed as soon as future overlap chunks can no longer modify them.
//!
//! TwoStem mode produces vocals and accompaniment only. When the model
//! bundle provides a verified `karaoke_2stem` output contract, vocals
//! and accompaniment are read directly. Otherwise, a four-output bundle
//! is used and drums+bass+other are summed per-sample into the
//! accompaniment scratch buffer — no full-song drums/bass/other buffers
//! are retained.
//!
//! FourStem mode produces four independent stems (drums, bass, other,
//! vocals) with no accompaniment computation.

use crate::{
    audio::decode::DecodedAudio,
    audio::encode::StreamingOggWriter,
    config::StemMode,
    separator::{model::LoadedModel, preprocess, workspace::SeparationWorkspace},
};
use anyhow::{bail, Context, Result};
use ort::{session::SessionInputValue, value::TensorRef};

pub const DEMUCS_STEM_NAMES: [&str; 4] = ["drums", "bass", "other", "vocals"];
/// Stem name order for two-stem bundles that provide vocals + accompaniment
/// directly. The model output contract must name these outputs explicitly.
pub const TWO_STEM_OUTPUT_NAMES: [&str; 2] = ["vocals", "accompaniment"];

/// The model's output contract, detected from the ORT session outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelOutputContract {
    /// Four stems in a single stacked tensor: `[4, channels, frames]`.
    FourStemStacked,
    /// Four separate stem tensors, each `[channels, frames]`.
    FourStemSeparate,
    /// Two-stem bundle: vocals + accompaniment as two separate tensors.
    TwoStemBundle,
}

/// Streaming OGG writers for one separation run. The number of writers
/// matches the stem mode: 2 for TwoStem, 4 for FourStem.
pub struct StemWriters {
    pub mode: StemMode,
    pub vocals: StreamingOggWriter,
    pub accompaniment: Option<StreamingOggWriter>,
    pub drums: Option<StreamingOggWriter>,
    pub bass: Option<StreamingOggWriter>,
    pub other: Option<StreamingOggWriter>,
}

impl StemWriters {
    fn write_vocals(&mut self, pcm: &[f32]) -> Result<()> {
        self.vocals
            .accept_frames(pcm)
            .context("failed to write vocals frames")
    }

    fn write_accompaniment(&mut self, pcm: &[f32]) -> Result<()> {
        self.accompaniment
            .as_mut()
            .context("TwoStem requires accompaniment writer")?
            .accept_frames(pcm)
            .context("failed to write accompaniment frames")
    }

    fn write_drums(&mut self, pcm: &[f32]) -> Result<()> {
        self.drums
            .as_mut()
            .context("FourStem requires drums writer")?
            .accept_frames(pcm)
            .context("failed to write drums frames")
    }

    fn write_bass(&mut self, pcm: &[f32]) -> Result<()> {
        self.bass
            .as_mut()
            .context("FourStem requires bass writer")?
            .accept_frames(pcm)
            .context("failed to write bass frames")
    }

    fn write_other(&mut self, pcm: &[f32]) -> Result<()> {
        self.other
            .as_mut()
            .context("FourStem requires other writer")?
            .accept_frames(pcm)
            .context("failed to write other frames")
    }

    /// Finalize all writers. If a later writer fails after an earlier file
    /// was promoted, remove every final path from this run before returning
    /// the error so the filesystem cannot expose a partial stem set.
    pub fn finish_all(self) -> Result<()> {
        let StemWriters {
            mode,
            vocals,
            accompaniment,
            drums,
            bass,
            other,
        } = self;

        let mut final_paths = vec![vocals.final_path().to_path_buf()];
        for writer in [
            accompaniment.as_ref(),
            drums.as_ref(),
            bass.as_ref(),
            other.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            final_paths.push(writer.final_path().to_path_buf());
        }

        let result: Result<()> = match mode {
            StemMode::TwoStem => (|| {
                vocals
                    .finish()
                    .context("failed to finalize vocals writer")?;
                accompaniment
                    .context("TwoStem must have accompaniment writer")?
                    .finish()
                    .context("failed to finalize accompaniment writer")?;
                Ok(())
            })(),
            StemMode::FourStem => (|| {
                vocals
                    .finish()
                    .context("failed to finalize vocals writer")?;
                drums
                    .context("FourStem must have drums writer")?
                    .finish()
                    .context("failed to finalize drums writer")?;
                bass.context("FourStem must have bass writer")?
                    .finish()
                    .context("failed to finalize bass writer")?;
                other
                    .context("FourStem must have other writer")?
                    .finish()
                    .context("failed to finalize other writer")?;
                Ok(())
            })(),
        };

        if result.is_err() {
            for path in final_paths {
                let _ = std::fs::remove_file(path);
            }
        }
        result
    }
}

/// The outcome of a streaming separation run.
#[derive(Debug, Clone)]
pub struct SeparationOutcome {
    pub stem_mode: StemMode,
    pub vocals_path: String,
    pub accomp_path: String,
    pub drums_path: Option<String>,
    pub bass_path: Option<String>,
    pub other_path: Option<String>,
    pub frames_written: usize,
}

/// Detect the model's output contract from the ORT session outputs.
///
/// The detection logic checks output shapes against the expected stem
/// layouts. It does NOT guess the stem mode from the output — the stem
/// mode is a user decision. It only determines how to read the tensors.
fn detect_output_contract(model: &LoadedModel, channels: usize) -> Result<ModelOutputContract> {
    // Collect output metadata into owned data so the session guard can be
    // dropped before we process the shapes.
    let output_infos: Vec<(String, Vec<i64>)> = {
        let session = model
            .session
            .lock()
            .map_err(|_| anyhow::anyhow!("ONNX session lock was poisoned"))?;
        session
            .outputs()
            .iter()
            .map(|output| {
                let name = output.name().to_owned();
                let dims = output
                    .dtype()
                    .tensor_shape()
                    .map(|s| s.iter().copied().collect())
                    .unwrap_or_default();
                (name, dims)
            })
            .collect()
    };

    classify_output_contract(&output_infos, channels)
}

/// Classify the model output contract from owned output metadata.
///
/// Separated from `detect_output_contract` so the classification rules can
/// be unit-tested without an ORT session.
fn classify_output_contract(
    output_infos: &[(String, Vec<i64>)],
    channels: usize,
) -> Result<ModelOutputContract> {
    if output_infos.is_empty() {
        bail!("model has no outputs");
    }

    // A two-stem bundle is valid only when both outputs have stem-like
    // shapes and their names unambiguously identify vocals and accompaniment.
    // Generic names such as output_0/output_1 cannot be routed safely later.
    if output_infos.len() == 2 {
        let both_stem_like = output_infos
            .iter()
            .all(|(_, dims)| looks_like_single_stem_output(dims, channels));
        let has_vocals = output_infos
            .iter()
            .any(|(name, _)| name == "vocals" || name.contains("vocal"));
        let has_accompaniment = output_infos
            .iter()
            .any(|(name, _)| name == "accompaniment" || name.contains("accomp"));
        if both_stem_like && has_vocals && has_accompaniment {
            return Ok(ModelOutputContract::TwoStemBundle);
        }
    }

    // Check for stacked four-stem output: 1 output with [4, channels, frames].
    if output_infos.len() == 1 && looks_like_stacked_stem_output(&output_infos[0].1, channels) {
        return Ok(ModelOutputContract::FourStemStacked);
    }

    // Check for separate four-stem output: 4+ outputs with stem-like shapes.
    let stem_like_count = output_infos
        .iter()
        .filter(|(_, dims)| looks_like_single_stem_output(dims, channels))
        .count();

    if stem_like_count >= DEMUCS_STEM_NAMES.len() {
        return Ok(ModelOutputContract::FourStemSeparate);
    }

    let shapes = output_infos
        .iter()
        .map(|(name, dims)| format!("{name}: {dims:?}"))
        .collect::<Vec<_>>();
    bail!(
        "could not detect model output contract; saw: {}",
        shapes.join(", ")
    );
}

/// Compute the hop size and total chunk count for the streaming loop.
///
/// A short input is one inference window even when it exceeds half of the
/// model window. Longer inputs retain the 50% overlap schedule. The returned
/// hop drives `(0..input_frame_count).step_by(hop_size)`, and the chunk
/// count equals the number of loop iterations so progress never exceeds
/// the reported total.
fn chunk_schedule(input_frame_count: usize, chunk_size: usize) -> (usize, usize) {
    let hop_size = if input_frame_count <= chunk_size {
        input_frame_count.max(1)
    } else {
        (chunk_size / 2).max(1)
    };
    let total_chunks = input_frame_count.div_ceil(hop_size);
    (hop_size, total_chunks)
}

/// Run streaming separation, writing finalized PCM directly to OGG writers.
///
/// This function does NOT return a `SeparationResult` with full-song PCM.
/// Finalized frames are streamed to the writers as chunks complete, so
/// memory is bounded by the workspace size (chunk_size * channels).
///
/// The caller is responsible for creating the `StemWriters` with the
/// correct output paths and for finalizing them after this function
/// returns successfully. On error, the writers are dropped and their
/// temp files are cleaned up automatically.
#[allow(clippy::too_many_arguments)]
pub fn separate_streaming(
    model: &LoadedModel,
    normalized_audio: &DecodedAudio,
    stem_mode: StemMode,
    writers: &mut StemWriters,
    workspace: &mut SeparationWorkspace,
    mut on_chunk_complete: impl FnMut(usize, usize),
) -> Result<SeparationOutcome> {
    let channels = normalized_audio.channels;
    let input_frame_count = normalized_audio.samples.len() / channels;
    let chunk_size = preprocess::target_frame_count(model, input_frame_count)?;
    let (hop_size, total_chunks) = chunk_schedule(input_frame_count, chunk_size);

    let output_contract = detect_output_contract(model, channels)?;
    configure_model_inputs(model, workspace)?;

    // Verify the output contract is compatible with the requested stem mode.
    match (&output_contract, stem_mode) {
        (ModelOutputContract::TwoStemBundle, StemMode::TwoStem) => {}
        (ModelOutputContract::FourStemStacked, _) => {}
        (ModelOutputContract::FourStemSeparate, _) => {}
        (ModelOutputContract::TwoStemBundle, StemMode::FourStem) => {
            bail!(
                "cannot use FourStem mode with a two-stem model bundle; \
                 select TwoStem mode or install a four-output bundle"
            );
        }
    }

    // Interrupted runs always restart from chunk 0. Vorbis encoder and OLA
    // state are intentionally not serialized.
    let window = workspace.window().to_vec();

    let mut chunk_index = 0usize;
    for chunk_start_frame in (0..input_frame_count).step_by(hop_size) {
        let chunk_frame_count = (input_frame_count - chunk_start_frame).min(chunk_size);

        // 1. Fill planar input directly from the normalized source.
        workspace.fill_planar_input(
            &normalized_audio.samples,
            chunk_start_frame,
            chunk_frame_count,
        );

        // 2. Build borrowed ORT inputs and run inference.
        // The session guard must stay alive while we read the output tensors
        // because SessionOutputs borrows from the session. We process the
        // outputs (reading tensor data into workspace buffers) within this
        // scope so the guard can be released before I/O.
        let session_inputs = build_session_inputs(workspace)?;
        {
            let mut session_guard = model
                .session
                .lock()
                .map_err(|_| anyhow::anyhow!("ONNX session lock was poisoned"))?;
            let outputs = session_guard
                .run(session_inputs)
                .context("failed to run Demucs inference")?;

            // 4. Read output tensor views and feed to OLA rings.
            // The writers are passed so add_chunk can flush to them when
            // it auto-shifts the OLA ring.
            match stem_mode {
                StemMode::TwoStem => {
                    process_two_stem_chunk(
                        &outputs,
                        output_contract,
                        workspace,
                        writers,
                        chunk_frame_count,
                        &window,
                        chunk_start_frame,
                    )?;
                }
                StemMode::FourStem => {
                    process_four_stem_chunk(
                        &outputs,
                        output_contract,
                        workspace,
                        writers,
                        chunk_frame_count,
                        &window,
                        chunk_start_frame,
                    )?;
                }
            }
        }
        // Session guard and outputs are both dropped here.

        // 5. Flush finalized frames to writers.
        // After processing chunk N (starting at chunk_start_frame), frames
        // before chunk_start_frame are safe because the next chunk starts
        // at chunk_start_frame + hop_size and only covers from there.
        let safe_through = chunk_start_frame;
        flush_finalized_to_writers(workspace, stem_mode, safe_through, writers)?;

        chunk_index += 1;
        on_chunk_complete(chunk_index, total_chunks);
    }

    // Flush all remaining frames.
    flush_finalized_to_writers(workspace, stem_mode, input_frame_count, writers)?;
    let finalized_frames = input_frame_count;

    let vocals_path = writers
        .vocals
        .final_path()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("vocals.ogg")
        .to_string();
    let accomp_path = writers
        .accompaniment
        .as_ref()
        .map(|w| {
            w.final_path()
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("accompaniment.ogg")
                .to_string()
        })
        .unwrap_or_default();
    let drums_path = writers.drums.as_ref().map(|w| {
        w.final_path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("drums.ogg")
            .to_string()
    });
    let bass_path = writers.bass.as_ref().map(|w| {
        w.final_path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("bass.ogg")
            .to_string()
    });
    let other_path = writers.other.as_ref().map(|w| {
        w.final_path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("other.ogg")
            .to_string()
    });

    Ok(SeparationOutcome {
        stem_mode,
        vocals_path,
        accomp_path,
        drums_path,
        bass_path,
        other_path,
        frames_written: finalized_frames,
    })
}

/// Process a TwoStem chunk: read vocals and accompaniment from the model
/// output and feed them to the OLA rings.
///
/// If the model provides a `karaoke_2stem` bundle, vocals and accompaniment
/// are read directly. Otherwise, drums+bass+other are summed per-sample
/// into the accompaniment scratch buffer.
fn process_two_stem_chunk(
    outputs: &ort::session::SessionOutputs,
    contract: ModelOutputContract,
    workspace: &mut SeparationWorkspace,
    writers: &mut StemWriters,
    chunk_frame_count: usize,
    window: &[f32],
    chunk_start_frame: usize,
) -> Result<()> {
    let channels = workspace.channels;

    match contract {
        ModelOutputContract::TwoStemBundle => {
            // Read vocals and accompaniment directly from two output tensors.
            // Phase 1: read into reusable stem buffers.
            for buf in workspace.stem_output_buffers.iter_mut() {
                buf[..chunk_frame_count * channels].fill(0.0);
            }

            let mut vocals_idx = None;
            let mut accomp_idx = None;
            for (name, output_value) in outputs.iter() {
                let (shape, data) = output_value
                    .try_extract_tensor::<f32>()
                    .context("two-stem output tensor was not f32")?;
                let dims: Vec<i64> = shape.iter().copied().collect();
                let (ch, frames) = extract_stem_dims(&dims, name, channels)?;

                if name == "vocals" || name.contains("vocal") {
                    deinterleave_to_interleaved(
                        data,
                        ch,
                        frames,
                        channels,
                        &mut workspace.stem_output_buffers[0],
                        chunk_frame_count,
                    );
                    vocals_idx = Some(0);
                } else if name == "accompaniment" || name.contains("accomp") {
                    deinterleave_to_interleaved(
                        data,
                        ch,
                        frames,
                        channels,
                        &mut workspace.stem_output_buffers[1],
                        chunk_frame_count,
                    );
                    accomp_idx = Some(1);
                }
            }

            let v_idx = vocals_idx.context("two-stem bundle missing vocals output")?;
            let a_idx = accomp_idx.context("two-stem bundle missing accompaniment output")?;

            // Phase 2: feed to OLA rings. The rings and buffers are disjoint
            // fields in the workspace, so NLL allows simultaneous borrows.
            // The sink closures write to the streaming writers when the ring
            // auto-shifts.
            let sample_count = chunk_frame_count * channels;
            let rings = workspace
                .two_stem_rings
                .as_mut()
                .context("TwoStem mode requires two_stem_rings in workspace")?;
            let vocals_buf = &workspace.stem_output_buffers[v_idx][..sample_count];
            rings.vocals.add_chunk(
                chunk_start_frame,
                chunk_frame_count,
                vocals_buf,
                window,
                |pcm| writers.write_vocals(pcm),
            )?;
            let accomp_buf = &workspace.stem_output_buffers[a_idx][..sample_count];
            rings.accompaniment.add_chunk(
                chunk_start_frame,
                chunk_frame_count,
                accomp_buf,
                window,
                |pcm| writers.write_accompaniment(pcm),
            )?;
        }
        ModelOutputContract::FourStemStacked | ModelOutputContract::FourStemSeparate => {
            // Phase 1: read four stems into reusable buffers.
            read_four_stems_into(outputs, contract, channels, chunk_frame_count, workspace)?;

            // Phase 2: sum drums+bass+other into accompaniment scratch.
            let sample_count = chunk_frame_count * channels;
            let scratch = &mut workspace.accompaniment_scratch[..sample_count];
            scratch.fill(0.0);
            for stem_idx in 0..3 {
                let stem = &workspace.stem_output_buffers[stem_idx][..sample_count];
                for (dst, &src) in scratch.iter_mut().zip(stem) {
                    *dst += src;
                }
            }

            // Phase 3: feed vocals and accompaniment to OLA rings.
            let rings = workspace
                .two_stem_rings
                .as_mut()
                .context("TwoStem mode requires two_stem_rings in workspace")?;
            let vocals_buf = &workspace.stem_output_buffers[3][..sample_count];
            rings.vocals.add_chunk(
                chunk_start_frame,
                chunk_frame_count,
                vocals_buf,
                window,
                |pcm| writers.write_vocals(pcm),
            )?;
            let accomp_buf = &workspace.accompaniment_scratch[..sample_count];
            rings.accompaniment.add_chunk(
                chunk_start_frame,
                chunk_frame_count,
                accomp_buf,
                window,
                |pcm| writers.write_accompaniment(pcm),
            )?;
        }
    }

    Ok(())
}

/// Process a FourStem chunk: read four stems and feed each to its OLA ring.
fn process_four_stem_chunk(
    outputs: &ort::session::SessionOutputs,
    contract: ModelOutputContract,
    workspace: &mut SeparationWorkspace,
    writers: &mut StemWriters,
    chunk_frame_count: usize,
    window: &[f32],
    chunk_start_frame: usize,
) -> Result<()> {
    let channels = workspace.channels;

    // Phase 1: read four stems into reusable buffers.
    read_four_stems_into(outputs, contract, channels, chunk_frame_count, workspace)?;

    // Phase 2: feed each stem to its OLA ring. The sink closures write to
    // the streaming writers when the ring auto-shifts.
    let sample_count = chunk_frame_count * channels;
    let rings = workspace
        .four_stem_rings
        .as_mut()
        .context("FourStem mode requires four_stem_rings in workspace")?;

    let drums_buf = &workspace.stem_output_buffers[0][..sample_count];
    rings.drums.add_chunk(
        chunk_start_frame,
        chunk_frame_count,
        drums_buf,
        window,
        |pcm| writers.write_drums(pcm),
    )?;

    let bass_buf = &workspace.stem_output_buffers[1][..sample_count];
    rings.bass.add_chunk(
        chunk_start_frame,
        chunk_frame_count,
        bass_buf,
        window,
        |pcm| writers.write_bass(pcm),
    )?;

    let other_buf = &workspace.stem_output_buffers[2][..sample_count];
    rings.other.add_chunk(
        chunk_start_frame,
        chunk_frame_count,
        other_buf,
        window,
        |pcm| writers.write_other(pcm),
    )?;

    let vocals_buf = &workspace.stem_output_buffers[3][..sample_count];
    rings.vocals.add_chunk(
        chunk_start_frame,
        chunk_frame_count,
        vocals_buf,
        window,
        |pcm| writers.write_vocals(pcm),
    )?;

    Ok(())
}

/// Read four stems from the model output into the workspace's reusable
/// stem output buffers. No per-chunk allocation — buffers are reused.
fn read_four_stems_into(
    outputs: &ort::session::SessionOutputs,
    contract: ModelOutputContract,
    channels: usize,
    chunk_frame_count: usize,
    workspace: &mut SeparationWorkspace,
) -> Result<()> {
    // Zero the active region of all stem buffers.
    for buf in workspace.stem_output_buffers.iter_mut() {
        buf[..chunk_frame_count * channels].fill(0.0);
    }

    match contract {
        ModelOutputContract::FourStemStacked => {
            for (_, output_value) in outputs.iter() {
                let (shape, data) = output_value
                    .try_extract_tensor::<f32>()
                    .context("stacked output tensor was not f32")?;
                let dims: Vec<i64> = shape.iter().copied().collect();
                if !looks_like_stacked_stem_output(&dims, channels) {
                    continue;
                }
                let (stem_count, ch, frames) = match dims.as_slice() {
                    [sc, c, f] => (
                        usize_from_dim(*sc, "stem count")?,
                        usize_from_dim(*c, "channel count")?,
                        usize_from_dim(*f, "frame count")?,
                    ),
                    [1, sc, c, f] => (
                        usize_from_dim(*sc, "stem count")?,
                        usize_from_dim(*c, "channel count")?,
                        usize_from_dim(*f, "frame count")?,
                    ),
                    _ => bail!("unexpected stacked output rank"),
                };
                if stem_count != DEMUCS_STEM_NAMES.len() {
                    bail!(
                        "stacked output must have {} stems, got {stem_count}",
                        DEMUCS_STEM_NAMES.len()
                    );
                }
                let stride = ch * frames;
                for (stem_idx, stem_buf) in workspace.stem_output_buffers.iter_mut().enumerate() {
                    let offset = stem_idx * stride;
                    deinterleave_to_interleaved(
                        &data[offset..offset + stride],
                        ch,
                        frames,
                        channels,
                        stem_buf,
                        chunk_frame_count,
                    );
                }
                return Ok(());
            }
            bail!("no stacked stem output found in model outputs");
        }
        ModelOutputContract::FourStemSeparate => {
            let matching: Vec<_> = outputs
                .iter()
                .filter(|(_, v)| {
                    v.try_extract_tensor::<f32>()
                        .map(|(shape, _)| {
                            let dims: Vec<i64> = shape.iter().copied().collect();
                            looks_like_single_stem_output(&dims, channels)
                        })
                        .unwrap_or(false)
                })
                .collect();

            if matching.len() < DEMUCS_STEM_NAMES.len() {
                bail!(
                    "expected {} separate stem outputs, found {}",
                    DEMUCS_STEM_NAMES.len(),
                    matching.len()
                );
            }

            for (stem_idx, (_, output_value)) in
                matching.iter().take(DEMUCS_STEM_NAMES.len()).enumerate()
            {
                let (shape, data) = output_value
                    .try_extract_tensor::<f32>()
                    .context("separate stem output was not f32")?;
                let dims: Vec<i64> = shape.iter().copied().collect();
                let (ch, frames) = match dims.as_slice() {
                    [c, f] => (
                        usize_from_dim(*c, "channel count")?,
                        usize_from_dim(*f, "frame count")?,
                    ),
                    [1, c, f] => (
                        usize_from_dim(*c, "channel count")?,
                        usize_from_dim(*f, "frame count")?,
                    ),
                    _ => bail!("unexpected separate output rank"),
                };
                deinterleave_to_interleaved(
                    data,
                    ch,
                    frames,
                    channels,
                    &mut workspace.stem_output_buffers[stem_idx],
                    chunk_frame_count,
                );
            }
            Ok(())
        }
        ModelOutputContract::TwoStemBundle => {
            bail!("cannot read four stems from a two-stem bundle");
        }
    }
}

/// Convert channels-first (planar) tensor data to interleaved PCM.
///
/// `planar_data` is `[channel, frame]` layout. The output is `[frame, channel]`
/// (interleaved). Only `chunk_frame_count` frames are copied; the rest are
/// ignored (the model may output more frames than the chunk needs).
fn deinterleave_to_interleaved(
    planar_data: &[f32],
    planar_channels: usize,
    planar_frames: usize,
    expected_channels: usize,
    output: &mut [f32],
    chunk_frame_count: usize,
) {
    let channels = planar_channels.min(expected_channels);
    let frames = planar_frames.min(chunk_frame_count);
    for frame in 0..frames {
        for ch in 0..channels {
            let src = ch * planar_frames + frame;
            let dst = frame * expected_channels + ch;
            if dst < output.len() && src < planar_data.len() {
                output[dst] = planar_data[src];
            }
        }
    }
}

/// Flush finalized frames from the OLA rings to the streaming writers.
fn flush_finalized_to_writers(
    workspace: &mut SeparationWorkspace,
    stem_mode: StemMode,
    safe_through: usize,
    writers: &mut StemWriters,
) -> Result<()> {
    match stem_mode {
        StemMode::TwoStem => {
            let rings = workspace
                .two_stem_rings
                .as_mut()
                .context("TwoStem requires two_stem_rings")?;
            rings
                .vocals
                .flush_finalized(safe_through, |pcm| writers.write_vocals(pcm))?;
            rings
                .accompaniment
                .flush_finalized(safe_through, |pcm| writers.write_accompaniment(pcm))?;
        }
        StemMode::FourStem => {
            let rings = workspace
                .four_stem_rings
                .as_mut()
                .context("FourStem requires four_stem_rings")?;
            rings
                .drums
                .flush_finalized(safe_through, |pcm| writers.write_drums(pcm))?;
            rings
                .bass
                .flush_finalized(safe_through, |pcm| writers.write_bass(pcm))?;
            rings
                .other
                .flush_finalized(safe_through, |pcm| writers.write_other(pcm))?;
            rings
                .vocals
                .flush_finalized(safe_through, |pcm| writers.write_vocals(pcm))?;
        }
    }
    Ok(())
}

/// Resolve and cache the model input contract once per separation run.
fn configure_model_inputs(model: &LoadedModel, workspace: &mut SeparationWorkspace) -> Result<()> {
    let session = model
        .session
        .lock()
        .map_err(|_| anyhow::anyhow!("ONNX session lock was poisoned"))?;
    let expected_shape = workspace.input_shape().to_vec();
    let mut audio_input_name = None;
    let mut auxiliary_inputs = Vec::new();

    for input in session.inputs() {
        let input_shape = input
            .dtype()
            .tensor_shape()
            .with_context(|| format!("Demucs input {} is not a tensor", input.name()))?;
        let dims: Vec<i64> = input_shape.iter().copied().collect();
        if looks_like_audio_input(&dims, workspace.channels) {
            if dims != expected_shape {
                bail!(
                    "Demucs audio input {} expected shape {:?}, prepared shape was {:?}",
                    input.name(),
                    dims,
                    expected_shape
                );
            }
            if audio_input_name.replace(input.name().to_owned()).is_some() {
                bail!("Demucs model has more than one audio input");
            }
        } else {
            let zero_count = num_elements_for_dims(&dims).with_context(|| {
                format!(
                    "Demucs auxiliary input {} has unsupported shape {:?}",
                    input.name(),
                    dims
                )
            })?;
            auxiliary_inputs.push((input.name().to_owned(), dims, vec![0.0; zero_count]));
        }
    }

    workspace.configure_model_inputs(
        audio_input_name.context("Demucs model has no stereo audio input")?,
        auxiliary_inputs,
    );
    Ok(())
}

/// Build zero-copy session inputs from the workspace's persistent backing.
fn build_session_inputs<'a>(
    workspace: &'a SeparationWorkspace,
) -> Result<Vec<(&'a str, SessionInputValue<'a>)>> {
    let mut session_inputs = Vec::with_capacity(1 + workspace.auxiliary_inputs().len());
    let audio_name = workspace
        .audio_input_name()
        .context("Demucs audio input contract was not configured")?;
    let audio_tensor =
        TensorRef::from_array_view((workspace.input_shape(), workspace.tensor_input()))
            .context("failed to build borrowed Demucs audio input tensor")?;
    session_inputs.push((audio_name, audio_tensor.into()));

    for (name, dims, zero_data) in workspace.auxiliary_inputs() {
        let tensor = TensorRef::from_array_view((dims.as_slice(), zero_data.as_slice()))
            .with_context(|| format!("failed to build borrowed zero tensor for {name}"))?;
        session_inputs.push((name.as_str(), tensor.into()));
    }
    Ok(session_inputs)
}

fn extract_stem_dims(
    dims: &[i64],
    name: &str,
    _expected_channels: usize,
) -> Result<(usize, usize)> {
    match dims {
        [ch, frames] => Ok((
            usize_from_dim(*ch, "channel count")?,
            usize_from_dim(*frames, "frame count")?,
        )),
        [1, ch, frames] => Ok((
            usize_from_dim(*ch, "channel count")?,
            usize_from_dim(*frames, "frame count")?,
        )),
        _ => bail!("stem output {} has unexpected rank {}", name, dims.len()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_window_constant_overlap_add_constraint() {
        let n = 1024;
        let w = crate::separator::workspace::hann_window(n);
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

    #[test]
    fn deinterleave_converts_channels_first_to_interleaved() {
        // channels-first: [ch0: [1, 2, 3], ch1: [4, 5, 6]]
        let planar = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let mut output = vec![0.0; 6];
        deinterleave_to_interleaved(&planar, 2, 3, 2, &mut output, 3);

        // interleaved: [1, 4, 2, 5, 3, 6]
        assert_eq!(output, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }

    #[test]
    fn deinterleave_truncates_to_chunk_frame_count() {
        let planar = vec![1.0, 2.0, 3.0, 4.0, 4.0, 5.0, 6.0, 6.0];
        let mut output = vec![0.0; 4]; // 2 frames * 2 channels
        deinterleave_to_interleaved(&planar, 2, 4, 2, &mut output, 2);

        // Only first 2 frames: [1, 4, 2, 5]
        assert_eq!(output, vec![1.0, 4.0, 2.0, 5.0]);
    }

    fn owned_outputs(outputs: &[(&str, &[i64])]) -> Vec<(String, Vec<i64>)> {
        outputs
            .iter()
            .map(|(name, dims)| (name.to_string(), dims.to_vec()))
            .collect()
    }

    #[test]
    fn classify_rejects_generic_two_output_model() {
        // Regression: generic output_0/output_1 must not be accepted as a
        // two-stem bundle even when both shapes are stem-like, because the
        // processing path cannot route unnamed outputs.
        let outputs = owned_outputs(&[("output_0", &[2, 44_100]), ("output_1", &[2, 44_100])]);
        let error = classify_output_contract(&outputs, 2)
            .expect_err("generic two-output model must be rejected");
        assert!(error.to_string().contains("could not detect"));
    }

    #[test]
    fn classify_rejects_two_output_model_missing_accompaniment_name() {
        let outputs = owned_outputs(&[("vocals", &[2, 44_100]), ("output_1", &[2, 44_100])]);
        classify_output_contract(&outputs, 2)
            .expect_err("two-output model without accompaniment name must be rejected");
    }

    #[test]
    fn classify_accepts_named_two_stem_bundle() {
        let outputs = owned_outputs(&[("vocals", &[2, 44_100]), ("accompaniment", &[2, 44_100])]);
        let contract = classify_output_contract(&outputs, 2).expect("named bundle");
        assert_eq!(contract, ModelOutputContract::TwoStemBundle);
    }

    #[test]
    fn classify_detects_stacked_four_stem_output() {
        let outputs = owned_outputs(&[("output", &[4, 2, 44_100])]);
        let contract = classify_output_contract(&outputs, 2).expect("stacked output");
        assert_eq!(contract, ModelOutputContract::FourStemStacked);
    }

    #[test]
    fn classify_detects_separate_four_stem_outputs() {
        let outputs = owned_outputs(&[
            ("drums", &[2, 44_100]),
            ("bass", &[2, 44_100]),
            ("other", &[2, 44_100]),
            ("vocals", &[2, 44_100]),
        ]);
        let contract = classify_output_contract(&outputs, 2).expect("separate outputs");
        assert_eq!(contract, ModelOutputContract::FourStemSeparate);
    }

    fn loop_iterations(input_frame_count: usize, hop_size: usize) -> usize {
        (0..input_frame_count).step_by(hop_size).count()
    }

    #[test]
    fn chunk_schedule_keeps_short_audio_on_one_window() {
        let chunk_size = 343_980;

        // Regression: audio longer than half a window but shorter than a
        // full window must run as a single inference window.
        for input in [1, chunk_size / 2, chunk_size / 2 + 1, chunk_size] {
            let (hop, total) = chunk_schedule(input, chunk_size);
            assert_eq!(total, 1, "input {input} should be one chunk");
            assert_eq!(loop_iterations(input, hop), total);
        }
    }

    #[test]
    fn chunk_schedule_total_matches_loop_iterations_for_long_audio() {
        let chunk_size = 343_980;

        for input in [
            chunk_size + 1,
            chunk_size * 3 / 2,
            chunk_size * 2,
            chunk_size * 23 + 17,
        ] {
            let (hop, total) = chunk_schedule(input, chunk_size);
            assert_eq!(hop, chunk_size / 2);
            assert_eq!(
                loop_iterations(input, hop),
                total,
                "input {input} chunk count must match loop iterations"
            );
        }
    }

    #[test]
    fn chunk_schedule_covers_every_frame() {
        let chunk_size = 64;

        for input in 1..=chunk_size * 4 {
            let (hop, total) = chunk_schedule(input, chunk_size);
            // Consecutive chunk starts are `hop <= chunk_size` apart, so the
            // schedule is gapless as long as the final chunk reaches the end.
            assert!(
                hop <= chunk_size,
                "input {input}: hop must not exceed chunk"
            );
            let last_start = (total - 1) * hop;
            assert!(
                last_start + chunk_size >= input,
                "input {input}: final chunk starting at {last_start} must reach the end"
            );
        }
    }

    #[test]
    fn finish_all_removes_already_promoted_files_after_late_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vocals_path = dir.path().join("vocals.ogg");
        let mut vocals =
            StreamingOggWriter::new(&vocals_path, 44_100, 2, None, None).expect("writer");
        vocals
            .accept_frames(&vec![0.0_f32; 2 * 2_048])
            .expect("write frames");

        let writers = StemWriters {
            mode: StemMode::TwoStem,
            vocals,
            accompaniment: None,
            drums: None,
            bass: None,
            other: None,
        };

        let error = writers
            .finish_all()
            .expect_err("missing accompaniment must fail");
        assert!(error.to_string().contains("accompaniment writer"));
        assert!(
            !vocals_path.exists(),
            "promoted vocals file must be cleaned up"
        );
    }
}
