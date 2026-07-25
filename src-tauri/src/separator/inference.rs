//! Streaming stem separation with bounded memory.
//!
//! The separation path uses a reusable workspace (`SeparationWorkspace`),
//! OLA ring buffers (`OlaRing`), and streaming OGG writers
//! (`StreamingOggWriter`) so that application memory is independent of
//! song duration for the additional output working set. One normalized
//! full-song input PCM buffer remains by design; finalized output frames
//! are flushed as soon as future overlap chunks can no longer modify them.
//!
//! The only production separation path is the spectral-core session
//! (`spectral_session`), dispatched from the model's verified spectral
//! interface (issue #172). Each chunk's stems are composed in the spectral
//! domain by the session and fed to the OLA rings here; the waveform graph
//! path (graph transform/layout adapters, output-contract detection) has
//! been removed.
//!
//! TwoStem mode produces vocals and accompaniment only: the spectral session
//! composes vocals directly and pre-mixes the accompaniment in the spectral
//! domain (one inverse transform instead of three), so no full-song
//! drums/bass/other buffers are retained. FourStem mode produces four
//! independent stems (drums, bass, other, vocals).

use crate::{
    audio::decode::DecodedAudio,
    audio::encode::StreamingOggWriter,
    config::StemMode,
    separator::{
        error::SeparationError, model::LoadedModel, preprocess, spectral_session,
        workspace::SeparationWorkspace,
    },
};
use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};

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
///
/// `cancel` is checked at the top of every chunk iteration. When it is set,
/// the loop returns `SeparationError::Cancelled` without finalizing any
/// writer, so an aborted run leaves no partial stem set (matching the
/// interrupted-run restart-from-chunk-0 guarantee).
#[allow(clippy::too_many_arguments)]
pub fn separate_streaming(
    model: &LoadedModel,
    normalized_audio: &DecodedAudio,
    stem_mode: StemMode,
    writers: &mut StemWriters,
    workspace: &mut SeparationWorkspace,
    cancel: &AtomicBool,
    mut on_chunk_complete: impl FnMut(usize, usize),
) -> Result<SeparationOutcome> {
    let channels = normalized_audio.channels;
    let input_frame_count = normalized_audio.samples.len() / channels;
    let chunk_size = preprocess::target_frame_count(model, input_frame_count)?;
    let (hop_size, total_chunks) = chunk_schedule(input_frame_count, chunk_size);

    // The only production path is the spectral-core session, dispatched from
    // the model's verified spectral interface (issue #172). The inference
    // window is fixed by the contract, so the schedule's chunk size must
    // equal the interface segment.
    let iface = &model.spectral;
    anyhow::ensure!(
        chunk_size == iface.segment_frames,
        "spectral-core window is fixed at {} frames, chunk size was {}",
        iface.segment_frames,
        chunk_size
    );
    // The transform plans and every chunk-loop buffer are created once per run
    // and reused across every chunk (fixed working memory per chunk). Both
    // stem modes are supported: the core always exposes four sources; TwoStem
    // pre-mixes the accompaniment in the spectral domain.
    let mut spectral_state = spectral_session::SpectralSessionState::new();

    // Interrupted runs always restart from chunk 0. Vorbis encoder and OLA
    // state are intentionally not serialized.
    let window = workspace.window().to_vec();

    let mut chunk_index = 0usize;
    for chunk_start_frame in (0..input_frame_count).step_by(hop_size) {
        // Cancellation checkpoint: bail before doing any work for this chunk.
        // The writers are dropped on the error path, cleaning up temp files.
        if cancel.load(Ordering::Relaxed) {
            return Err(SeparationError::Cancelled.into());
        }

        let chunk_frame_count = (input_frame_count - chunk_start_frame).min(chunk_size);

        // 1. Fill planar input directly from the normalized source.
        workspace.fill_planar_input(
            &normalized_audio.samples,
            chunk_start_frame,
            chunk_frame_count,
        );

        // 2-4. Forward transform, spectral-core inference, stem composition,
        // and OLA ring feeding all happen in the spectral session.
        spectral_session::process_spectral_chunk(
            model,
            iface,
            &mut spectral_state,
            workspace,
            writers,
            stem_mode,
            chunk_frame_count,
            &window,
            chunk_start_frame,
        )?;

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

/// Feed vocals + accompaniment OLA rings from two workspace stem buffers.
///
/// Shared by every path whose vocals/accompaniment already sit in stem
/// output buffers (two-stem bundles, dual-stacked reads, and the spectral
/// session's composed stems).
pub(crate) fn feed_two_stem_rings(
    workspace: &mut SeparationWorkspace,
    writers: &mut StemWriters,
    vocals_idx: usize,
    accomp_idx: usize,
    chunk_frame_count: usize,
    window: &[f32],
    chunk_start_frame: usize,
) -> Result<()> {
    let channels = workspace.channels;
    let sample_count = chunk_frame_count * channels;
    let rings = workspace
        .two_stem_rings
        .as_mut()
        .context("TwoStem mode requires two_stem_rings in workspace")?;
    let vocals_buf = &workspace.stem_output_buffers[vocals_idx][..sample_count];
    rings.vocals.add_chunk(
        chunk_start_frame,
        chunk_frame_count,
        vocals_buf,
        window,
        |pcm| writers.write_vocals(pcm),
    )?;
    let accomp_buf = &workspace.stem_output_buffers[accomp_idx][..sample_count];
    rings.accompaniment.add_chunk(
        chunk_start_frame,
        chunk_frame_count,
        accomp_buf,
        window,
        |pcm| writers.write_accompaniment(pcm),
    )?;
    Ok(())
}

/// Feed the four stem OLA rings from the workspace stem buffers
/// (drums, bass, other, vocals in buffer order 0..3).
pub(crate) fn feed_four_stem_rings(
    workspace: &mut SeparationWorkspace,
    writers: &mut StemWriters,
    chunk_frame_count: usize,
    window: &[f32],
    chunk_start_frame: usize,
) -> Result<()> {
    let channels = workspace.channels;
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
