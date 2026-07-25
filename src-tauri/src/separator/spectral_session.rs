//! Typed spectral-core session path (issue #172 PR 2).
//!
//! Runs models exported at the `openkara.spectral-contract/v1` boundary
//! (openkara-models#23): the application computes the forward transform
//! (`spectral::SpectralPlans::spec`) from the workspace's planar mix window,
//! feeds the core the contract spectral tensor plus the raw mix, and composes
//! output stems from the core's pre-ISTFT and time-branch tensors:
//!
//! ```text
//! stems[s] = ispec(spectral_out[:, s], segment_frames) + time_out[:, s]
//! ```
//!
//! The session interface is TYPED: tensor names and fixed shapes are verified
//! against the contract at model-load time (`verify_spectral_interface`), and
//! dispatch into this path happens exclusively via the model's declared
//! tensor-interface metadata. Output-rank and filename heuristics are
//! forbidden.
//!
//! Stem layouts are explicit per stem mode:
//! - FourStem composes each of drums/bass/other/vocals independently
//!   (four inverse transforms).
//! - TwoStem composes vocals directly and pre-mixes the accompaniment in the
//!   spectral domain — the contract's linearity guarantee — so ONE inverse
//!   transform replaces three, plus the summed time branches.
//!
//! Per-chunk output buffers are the shared workspace stem buffers; the
//! transform's own working buffers live in `SpectralPlans` and are reused
//! across chunks. Fixed-buffer optimization of the remaining per-chunk
//! allocations (the spec output and composition scratch) is issue #172 PR 3.

use crate::{
    config::StemMode,
    separator::{
        inference::StemWriters,
        model::LoadedModel,
        spectral::{SpectralPlans, CHANNELS, CONTRACT_FREQS},
        workspace::SeparationWorkspace,
    },
};
use anyhow::{bail, Context, Result};
use ort::{session::SessionInputValue, value::TensorRef};

/// Sources produced by the spectral core, in contract order.
pub const SPECTRAL_SOURCES: [&str; 4] = ["drums", "bass", "other", "vocals"];
const VOCALS: usize = 3;

/// Contract tensor names (fixed by `openkara.spectral-contract/v1`).
pub const SPECTRAL_INPUT_NAME: &str = "spectral";
pub const MIX_INPUT_NAME: &str = "mix";
pub const SPECTRAL_OUTPUT_NAME: &str = "spectral_out";
pub const TIME_OUTPUT_NAME: &str = "time_out";

/// Verified spectral-core session interface. Constructed only by
/// [`verify_spectral_interface`] at model-load time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpectralInterface {
    /// Fixed inference window in samples (`mix` input dim 2).
    pub segment_frames: usize,
    /// Spectral frames per window (`= ceil(segment_frames / hop)`).
    pub spectral_frames: usize,
}

impl SpectralInterface {
    /// Length of one source's spectral tensor `[C, 2, F, T]` in floats.
    pub fn spectral_stride(&self) -> usize {
        CHANNELS * 2 * CONTRACT_FREQS * self.spectral_frames
    }

    /// Length of one source's time tensor `[C, frames]` in floats.
    pub fn time_stride(&self) -> usize {
        CHANNELS * self.segment_frames
    }
}

/// Verify a session's tensor interface against the spectral contract.
///
/// Pure over owned I/O metadata so the rules are unit-testable without an
/// ORT session. Names, order, and every dimension are pinned; any deviation
/// is a load-time error, never a runtime guess.
pub fn verify_spectral_interface(
    inputs: &[(String, Vec<i64>)],
    outputs: &[(String, Vec<i64>)],
) -> Result<SpectralInterface> {
    let [(spec_name, spec_dims), (mix_name, mix_dims)] = inputs else {
        bail!(
            "spectral-core model must have exactly two inputs \
             [{SPECTRAL_INPUT_NAME}, {MIX_INPUT_NAME}], got {:?}",
            inputs.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
    };
    let [(sout_name, sout_dims), (tout_name, tout_dims)] = outputs else {
        bail!(
            "spectral-core model must have exactly two outputs \
             [{SPECTRAL_OUTPUT_NAME}, {TIME_OUTPUT_NAME}], got {:?}",
            outputs.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
    };
    if spec_name != SPECTRAL_INPUT_NAME
        || mix_name != MIX_INPUT_NAME
        || sout_name != SPECTRAL_OUTPUT_NAME
        || tout_name != TIME_OUTPUT_NAME
    {
        bail!(
            "spectral-core tensor names must be \
             [{SPECTRAL_INPUT_NAME}, {MIX_INPUT_NAME}] -> \
             [{SPECTRAL_OUTPUT_NAME}, {TIME_OUTPUT_NAME}], \
             got [{spec_name}, {mix_name}] -> [{sout_name}, {tout_name}]"
        );
    }

    let channels = CHANNELS as i64;
    let freqs = CONTRACT_FREQS as i64;
    let sources = SPECTRAL_SOURCES.len() as i64;

    let (segment_frames, spectral_frames) = match (mix_dims.as_slice(), spec_dims.as_slice()) {
        ([1, mc, frames], [1, sc, 2, f, t])
            if *mc == channels && *sc == channels && *f == freqs && *frames > 0 && *t > 0 =>
        {
            (*frames as usize, *t as usize)
        }
        _ => bail!(
            "spectral-core input shapes must be \
             {SPECTRAL_INPUT_NAME} [1, {channels}, 2, {freqs}, T] and \
             {MIX_INPUT_NAME} [1, {channels}, frames], \
             got {spec_dims:?} and {mix_dims:?}"
        ),
    };
    if spectral_frames != crate::separator::spectral::forward_frames(segment_frames) {
        bail!(
            "spectral frame count {} does not match ceil({} / hop) = {}",
            spectral_frames,
            segment_frames,
            crate::separator::spectral::forward_frames(segment_frames)
        );
    }

    let expected_sout: Vec<i64> = vec![1, sources, channels, 2, freqs, spectral_frames as i64];
    let expected_tout: Vec<i64> = vec![1, sources, channels, segment_frames as i64];
    if sout_dims != &expected_sout {
        bail!(
            "{SPECTRAL_OUTPUT_NAME} shape {sout_dims:?} does not match the \
             contract {expected_sout:?}"
        );
    }
    if tout_dims != &expected_tout {
        bail!(
            "{TIME_OUTPUT_NAME} shape {tout_dims:?} does not match the \
             contract {expected_tout:?}"
        );
    }

    Ok(SpectralInterface {
        segment_frames,
        spectral_frames,
    })
}

/// Compose one stem: `ispec(spectral_slice) + time_slice`, planar `[C, len]`.
fn compose_stem(
    plans: &mut SpectralPlans,
    spectral_slice: &[f32],
    time_slice: &[f32],
    length: usize,
) -> Vec<f32> {
    let mut wave = plans.ispec(spectral_slice, CHANNELS, length);
    for (w, &t) in wave.iter_mut().zip(time_slice.iter()) {
        *w += t;
    }
    wave
}

/// Run one spectral-core inference window and feed the OLA rings.
///
/// The mix window is the workspace's planar input backing (already filled
/// for this chunk); the forward transform, session run, and stem composition
/// all happen here. Ring feeding and finalized-frame flushing match the
/// waveform path exactly.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_spectral_chunk(
    model: &LoadedModel,
    iface: &SpectralInterface,
    plans: &mut SpectralPlans,
    workspace: &mut SeparationWorkspace,
    writers: &mut StemWriters,
    stem_mode: StemMode,
    chunk_frame_count: usize,
    window: &[f32],
    chunk_start_frame: usize,
) -> Result<()> {
    let channels = workspace.channels;
    anyhow::ensure!(
        channels == CHANNELS,
        "spectral contract requires {CHANNELS} channels, workspace has {channels}"
    );
    let segment = iface.segment_frames;

    // 1. Forward transform of the full (zero-tail-padded) window.
    let spectral = plans.spec(workspace.tensor_input(), channels, segment);

    // 2. Typed session run: named contract tensors, borrowed input views.
    let spectral_shape: [i64; 5] = [
        1,
        channels as i64,
        2,
        CONTRACT_FREQS as i64,
        iface.spectral_frames as i64,
    ];
    let mix_shape: [i64; 3] = [1, channels as i64, segment as i64];

    let spectral_stride = iface.spectral_stride();
    let time_stride = iface.time_stride();

    // Composition happens inside the session-output scope; the composed
    // stems land in the workspace stem buffers (interleaved) before the
    // guard drops.
    {
        let spectral_tensor =
            TensorRef::from_array_view((spectral_shape.as_slice(), spectral.as_slice()))
                .context("failed to build borrowed spectral input tensor")?;
        let mix_tensor =
            TensorRef::from_array_view((mix_shape.as_slice(), workspace.tensor_input()))
                .context("failed to build borrowed mix input tensor")?;
        let session_inputs: Vec<(&str, SessionInputValue<'_>)> = vec![
            (SPECTRAL_INPUT_NAME, spectral_tensor.into()),
            (MIX_INPUT_NAME, mix_tensor.into()),
        ];

        let mut session_guard = model
            .session
            .lock()
            .map_err(|_| anyhow::anyhow!("ONNX session lock was poisoned"))?;
        let outputs = session_guard
            .run(session_inputs)
            .context("failed to run spectral-core inference")?;

        let (_, spectral_out) = outputs
            .get(SPECTRAL_OUTPUT_NAME)
            .context("spectral-core did not produce its spectral output")?
            .try_extract_tensor::<f32>()
            .context("spectral-core spectral output was not f32")?;
        let (_, time_out) = outputs
            .get(TIME_OUTPUT_NAME)
            .context("spectral-core did not produce its time output")?
            .try_extract_tensor::<f32>()
            .context("spectral-core time output was not f32")?;
        anyhow::ensure!(
            spectral_out.len() == SPECTRAL_SOURCES.len() * spectral_stride
                && time_out.len() == SPECTRAL_SOURCES.len() * time_stride,
            "spectral-core output sizes do not match the verified interface"
        );

        let source_spectral =
            |s: usize| &spectral_out[s * spectral_stride..(s + 1) * spectral_stride];
        let source_time = |s: usize| &time_out[s * time_stride..(s + 1) * time_stride];

        match stem_mode {
            StemMode::FourStem => {
                // Explicit FourStem layout: one composition per source, in
                // contract order (drums, bass, other, vocals).
                for s in 0..SPECTRAL_SOURCES.len() {
                    let wave = compose_stem(plans, source_spectral(s), source_time(s), segment);
                    planar_to_buffer(
                        &wave,
                        segment,
                        channels,
                        &mut workspace.stem_output_buffers[s],
                        chunk_frame_count,
                    );
                }
            }
            StemMode::TwoStem => {
                // Explicit TwoStem layout: vocals composed directly;
                // accompaniment pre-mixed in the spectral domain (contract
                // linearity: ispec(Σ drums/bass/other) == Σ ispec(each)),
                // so one inverse transform replaces three.
                let vocals =
                    compose_stem(plans, source_spectral(VOCALS), source_time(VOCALS), segment);

                let mut accomp_spectral = source_spectral(0).to_vec();
                let mut accomp_time = source_time(0).to_vec();
                for s in 1..VOCALS {
                    for (dst, &src) in accomp_spectral.iter_mut().zip(source_spectral(s)) {
                        *dst += src;
                    }
                    for (dst, &src) in accomp_time.iter_mut().zip(source_time(s)) {
                        *dst += src;
                    }
                }
                let accompaniment = compose_stem(plans, &accomp_spectral, &accomp_time, segment);

                planar_to_buffer(
                    &vocals,
                    segment,
                    channels,
                    &mut workspace.stem_output_buffers[0],
                    chunk_frame_count,
                );
                planar_to_buffer(
                    &accompaniment,
                    segment,
                    channels,
                    &mut workspace.stem_output_buffers[1],
                    chunk_frame_count,
                );
            }
        }
    }

    // 3. Feed the OLA rings from the composed stem buffers.
    match stem_mode {
        StemMode::FourStem => {
            crate::separator::inference::feed_four_stem_rings(
                workspace,
                writers,
                chunk_frame_count,
                window,
                chunk_start_frame,
            )?;
        }
        StemMode::TwoStem => {
            crate::separator::inference::feed_two_stem_rings(
                workspace,
                writers,
                0,
                1,
                chunk_frame_count,
                window,
                chunk_start_frame,
            )?;
        }
    }
    Ok(())
}

/// Copy a planar `[C, frames]` stem into an interleaved workspace buffer,
/// truncated to `chunk_frame_count` frames.
fn planar_to_buffer(
    planar: &[f32],
    planar_frames: usize,
    channels: usize,
    buffer: &mut [f32],
    chunk_frame_count: usize,
) {
    buffer[..chunk_frame_count * channels].fill(0.0);
    for frame in 0..chunk_frame_count.min(planar_frames) {
        for ch in 0..channels {
            buffer[frame * channels + ch] = planar[ch * planar_frames + frame];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEGMENT: i64 = 343_980;
    const T: i64 = 336;

    fn contract_inputs() -> Vec<(String, Vec<i64>)> {
        vec![
            ("spectral".into(), vec![1, 2, 2, 2048, T]),
            ("mix".into(), vec![1, 2, SEGMENT]),
        ]
    }

    fn contract_outputs() -> Vec<(String, Vec<i64>)> {
        vec![
            ("spectral_out".into(), vec![1, 4, 2, 2, 2048, T]),
            ("time_out".into(), vec![1, 4, 2, SEGMENT]),
        ]
    }

    #[test]
    fn accepts_the_contract_interface() {
        let iface = verify_spectral_interface(&contract_inputs(), &contract_outputs())
            .expect("contract interface must verify");
        assert_eq!(iface.segment_frames, 343_980);
        assert_eq!(iface.spectral_frames, 336);
        assert_eq!(iface.spectral_stride(), 2 * 2 * 2048 * 336);
        assert_eq!(iface.time_stride(), 2 * 343_980);
    }

    #[test]
    fn rejects_wrong_input_names() {
        let mut inputs = contract_inputs();
        inputs[1].0 = "audio".into();
        verify_spectral_interface(&inputs, &contract_outputs())
            .expect_err("wrong input name must be rejected");
    }

    #[test]
    fn rejects_swapped_output_order() {
        let mut outputs = contract_outputs();
        outputs.swap(0, 1);
        verify_spectral_interface(&contract_inputs(), &outputs)
            .expect_err("swapped output order must be rejected");
    }

    #[test]
    fn rejects_waveform_interface() {
        let inputs = vec![("audio".to_string(), vec![1, 2, SEGMENT])];
        let outputs = vec![("stems".to_string(), vec![1, 4, 2, SEGMENT])];
        verify_spectral_interface(&inputs, &outputs)
            .expect_err("a waveform interface must never verify as spectral");
    }

    #[test]
    fn rejects_mismatched_spectral_frames() {
        let mut inputs = contract_inputs();
        inputs[0].1 = vec![1, 2, 2, 2048, 335];
        verify_spectral_interface(&inputs, &contract_outputs())
            .expect_err("T != ceil(frames/hop) must be rejected");
    }

    #[test]
    fn rejects_wrong_source_count() {
        let mut outputs = contract_outputs();
        outputs[0].1 = vec![1, 2, 2, 2, 2048, T];
        verify_spectral_interface(&contract_inputs(), &outputs)
            .expect_err("wrong source count must be rejected");
    }

    #[test]
    fn rejects_symbolic_dims() {
        let mut inputs = contract_inputs();
        inputs[1].1 = vec![1, 2, -1];
        verify_spectral_interface(&inputs, &contract_outputs())
            .expect_err("symbolic mix frames must be rejected");
    }

    #[test]
    fn planar_to_buffer_interleaves_and_truncates() {
        // planar [C=2, frames=3]: ch0 = [1,2,3], ch1 = [4,5,6]
        let planar = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let mut buffer = vec![9.0; 6];
        planar_to_buffer(&planar, 3, 2, &mut buffer, 2);
        assert_eq!(buffer, vec![1.0, 4.0, 2.0, 5.0, 9.0, 9.0]);
    }

    /// Direct numeric equivalence against the shipped waveform model on one
    /// full inference window. Gated on `OPENKARA_SPECTRAL_MODEL` (the
    /// spectral-core artifact is not in the stable catalog yet, so CI cannot
    /// provision it; run locally against a candidate export). The waveform
    /// reference is the dev model at `model::default_model_path()`.
    #[test]
    fn spectral_core_matches_waveform_model_on_one_window() {
        use crate::config::ExecutionProviderPreference;
        use crate::separator::model;

        let Some(spectral_model) = std::env::var_os("OPENKARA_SPECTRAL_MODEL") else {
            eprintln!("skipping spectral/waveform equivalence: OPENKARA_SPECTRAL_MODEL is not set");
            return;
        };
        let spectral_model = std::path::PathBuf::from(spectral_model);
        let waveform_model = model::default_model_path();
        if !waveform_model.is_file() {
            eprintln!("skipping spectral/waveform equivalence: no dev waveform model");
            return;
        }

        let runtime_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("generated")
            .join("onnxruntime")
            .join(model::ORT_RUNTIME_FILENAME);
        model::ensure_runtime_loaded_from_path(&runtime_path)
            .expect("dev runtime should initialize");

        let spectral = model::load_from_path(&spectral_model, ExecutionProviderPreference::Cpu)
            .expect("spectral-core model should load");
        let iface = spectral
            .spectral
            .clone()
            .expect("spectral-core model must carry a verified interface");
        let waveform = model::load_from_path(&waveform_model, ExecutionProviderPreference::Cpu)
            .expect("waveform model should load");

        let segment = iface.segment_frames;

        // Deterministic band-limited-ish noise, planar [C, segment].
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (((state >> 33) as f32 / (1u64 << 31) as f32) - 1.0) * 0.1
        };
        let mix: Vec<f32> = (0..CHANNELS * segment).map(|_| next()).collect();

        // Waveform reference: one session run, read the stacked four-stem
        // output and (dual models) the stacked two-stem output.
        let mix_shape: [i64; 3] = [1, CHANNELS as i64, segment as i64];
        let (four_ref, two_ref) = {
            let audio_tensor = TensorRef::from_array_view((mix_shape.as_slice(), mix.as_slice()))
                .expect("borrowed audio tensor");
            let input_name = waveform.inputs[0].clone();
            let inputs: Vec<(&str, SessionInputValue<'_>)> =
                vec![(input_name.as_str(), audio_tensor.into())];
            let mut guard = waveform.session.lock().expect("session lock");
            let outputs = guard.run(inputs).expect("waveform inference");
            let mut four: Option<Vec<f32>> = None;
            let mut two: Option<Vec<f32>> = None;
            for (_, value) in outputs.iter() {
                let (shape, data) = value
                    .try_extract_tensor::<f32>()
                    .expect("waveform output f32");
                let dims: Vec<i64> = shape.iter().copied().collect();
                match dims.as_slice() {
                    [1, 4, c, f] | [4, c, f] if *c == CHANNELS as i64 && *f == segment as i64 => {
                        four = Some(data.to_vec())
                    }
                    [1, 2, c, f] | [2, c, f] if *c == CHANNELS as i64 && *f == segment as i64 => {
                        two = Some(data.to_vec())
                    }
                    _ => {}
                }
            }
            (
                four.expect("waveform model must expose a stacked four-stem output"),
                two,
            )
        };

        // Spectral path: forward transform, core run, contract composition.
        let mut plans = SpectralPlans::new();
        let spec_in = plans.spec(&mix, CHANNELS, segment);
        let spectral_shape: [i64; 5] = [
            1,
            CHANNELS as i64,
            2,
            CONTRACT_FREQS as i64,
            iface.spectral_frames as i64,
        ];
        let (spectral_out, time_out) = {
            let spec_tensor =
                TensorRef::from_array_view((spectral_shape.as_slice(), spec_in.as_slice()))
                    .expect("borrowed spectral tensor");
            let mix_tensor = TensorRef::from_array_view((mix_shape.as_slice(), mix.as_slice()))
                .expect("borrowed mix tensor");
            let inputs: Vec<(&str, SessionInputValue<'_>)> = vec![
                (SPECTRAL_INPUT_NAME, spec_tensor.into()),
                (MIX_INPUT_NAME, mix_tensor.into()),
            ];
            let mut guard = spectral.session.lock().expect("session lock");
            let outputs = guard.run(inputs).expect("spectral-core inference");
            let (_, sout) = outputs[SPECTRAL_OUTPUT_NAME]
                .try_extract_tensor::<f32>()
                .expect("spectral output f32");
            let (_, tout) = outputs[TIME_OUTPUT_NAME]
                .try_extract_tensor::<f32>()
                .expect("time output f32");
            (sout.to_vec(), tout.to_vec())
        };

        let spectral_stride = iface.spectral_stride();
        let time_stride = iface.time_stride();

        let compare = |label: &str, reference: &[f32], actual: &[f32]| {
            assert_eq!(reference.len(), actual.len(), "{label} length");
            let mut max_abs = 0.0f32;
            let mut sq = 0.0f64;
            for (r, a) in reference.iter().zip(actual) {
                let d = (r - a).abs();
                max_abs = max_abs.max(d);
                sq += (d as f64) * (d as f64);
            }
            let rms = (sq / reference.len() as f64).sqrt();
            eprintln!("{label}: max-abs {max_abs:.3e}, rms {rms:.3e}");
            assert!(
                max_abs < 1e-2 && rms < 1e-3,
                "{label} diverges from the waveform reference \
                 (max-abs {max_abs:.3e}, rms {rms:.3e})"
            );
        };

        // FourStem layout: every source, contract order.
        for (s, name) in SPECTRAL_SOURCES.iter().enumerate() {
            let composed = compose_stem(
                &mut plans,
                &spectral_out[s * spectral_stride..(s + 1) * spectral_stride],
                &time_out[s * time_stride..(s + 1) * time_stride],
                segment,
            );
            compare(
                &format!("four-stem {name}"),
                &four_ref[s * time_stride..(s + 1) * time_stride],
                &composed,
            );
        }

        // TwoStem layout: vocals + spectral-domain accompaniment premix,
        // against the dual model's stacked two-stem output when present.
        if let Some(two_ref) = two_ref {
            let vocals = compose_stem(
                &mut plans,
                &spectral_out[VOCALS * spectral_stride..(VOCALS + 1) * spectral_stride],
                &time_out[VOCALS * time_stride..(VOCALS + 1) * time_stride],
                segment,
            );
            let mut premix_spec = spectral_out[..spectral_stride].to_vec();
            let mut premix_time = time_out[..time_stride].to_vec();
            for s in 1..VOCALS {
                for (dst, &src) in premix_spec
                    .iter_mut()
                    .zip(&spectral_out[s * spectral_stride..(s + 1) * spectral_stride])
                {
                    *dst += src;
                }
                for (dst, &src) in premix_time
                    .iter_mut()
                    .zip(&time_out[s * time_stride..(s + 1) * time_stride])
                {
                    *dst += src;
                }
            }
            let accompaniment = compose_stem(&mut plans, &premix_spec, &premix_time, segment);
            compare("two-stem vocals", &two_ref[..time_stride], &vocals);
            compare(
                "two-stem accompaniment",
                &two_ref[time_stride..2 * time_stride],
                &accompaniment,
            );
        } else {
            eprintln!("waveform model has no stacked two-stem output; premix compared only via linearity test");
        }
    }

    #[test]
    fn spectral_premix_matches_summed_compositions() {
        // Contract linearity: composing the accompaniment from summed
        // spectral+time tensors must equal summing the three composed stems.
        let mut plans = SpectralPlans::new();
        let length = 10_240;
        let t = crate::separator::spectral::forward_frames(length);
        let stride = CHANNELS * 2 * CONTRACT_FREQS * t;

        // Three deterministic pseudo-source tensors.
        let mut state = 0x243F_6A88_85A3_08D3_u64;
        let mut next = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as f32 / (1u64 << 31) as f32) - 1.0
        };
        let sources: Vec<Vec<f32>> = (0..3)
            .map(|_| (0..stride).map(|_| next() * 0.1).collect())
            .collect();
        let times: Vec<Vec<f32>> = (0..3)
            .map(|_| (0..CHANNELS * length).map(|_| next() * 0.1).collect())
            .collect();

        let mut summed = vec![0.0f32; CHANNELS * length];
        for s in 0..3 {
            let composed = compose_stem(&mut plans, &sources[s], &times[s], length);
            for (dst, &src) in summed.iter_mut().zip(&composed) {
                *dst += src;
            }
        }

        let mut premix_spec = sources[0].clone();
        let mut premix_time = times[0].clone();
        for s in 1..3 {
            for (dst, &src) in premix_spec.iter_mut().zip(&sources[s]) {
                *dst += src;
            }
            for (dst, &src) in premix_time.iter_mut().zip(&times[s]) {
                *dst += src;
            }
        }
        let premixed = compose_stem(&mut plans, &premix_spec, &premix_time, length);

        let max_abs = summed
            .iter()
            .zip(&premixed)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_abs < 1e-5,
            "premix must match summed compositions, max abs diff {max_abs}"
        );
    }
}
