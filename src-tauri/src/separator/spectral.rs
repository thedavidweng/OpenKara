//! Native STFT/ISTFT for the OpenKara spectral contract
//! (`openkara.spectral-contract/v1`).
//!
//! This module is a pure, dependency-light port of the float64 numpy
//! reference (`spectral_reference.py`) that defines the exact
//! waveform<->spectral transform semantics of the shipped HTDemucs graphs.
//! The shipped ONNX models implement those transforms with dense
//! conv1d/conv_transpose1d DFT matrices (~134 MB of constants per model);
//! implementing them natively (FFT) lets the spectral-core models of
//! `openkara-models#23` drop those constants.
//!
//! # Scope (issue #172 PR 1)
//!
//! This is the *pure DSP* layer only: the forward transform ([`SpectralPlans::spec`]),
//! the inverse transform ([`SpectralPlans::ispec`]), and the neural-core
//! [`magnitude`] view helper, all validated against the pinned golden vectors
//! at every intermediate stage. The production model / ORT session path that
//! consumes these transforms lives in the sibling [`spectral_session`] module
//! (issue #172 PR 2, once `openkara-models#23` published spectral-core
//! artifacts).
//!
//! [`spectral_session`]: super::spectral_session
//!
//! # Contract semantics (mirrored operation-by-operation from the reference)
//!
//! * periodic Hann window `sin²(π·n/4096)` (torch default, NOT symmetric);
//! * `n_fft = 4096`, `hop = 1024`, one-sided output with the Nyquist bin
//!   dropped (2049 → 2048 carried bins);
//! * normalization `1/√4096` applied in BOTH directions (`normalized=True`);
//! * imaginary convention `e^{−i2πkn/N}` — the imaginary part uses `−sin`
//!   (matches `torch.stft`), which is exactly what a real→complex FFT produces;
//! * Demucs outer reflect padding of `1536` samples, plus centered STFT
//!   reflect padding of `n_fft/2` inside the transform;
//! * forward frame crop `[2 : 2+le]`; inverse zero-Nyquist re-append, two-frame
//!   re-pad on each side, one-sided inverse DFT with hermitian doubling, and
//!   an overlap-add envelope clamped at `1e-8`.
//!
//! # Precision
//!
//! All transforms run in `f64` internally (f64 FFT plans, f64 window and
//! accumulators) and cast to `f32` only at the input/output boundary. The
//! contract gate for an fp32 implementation is `1e-3` max-abs versus the
//! golden vectors; computing internally in f64 lands the achieved error near
//! f32 round-off (~`1e-6`), well inside that gate, and keeps a clean
//! validation story. Selecting a faster f32 FFT is deferred to issue #172 PR 3
//! (runtime-measured tuning) and must not change any contract semantics.
//!
//! # Segment stitching (informational — no OLA integration in this PR)
//!
//! The Nyquist bin (22050 Hz) is discarded by the forward transform and
//! reconstructed as zero, so only band-limited content round-trips exactly;
//! broadband content leaks ~−80 dB of Hann-windowed energy into that bin,
//! bounding broadband round-trip error near `1e-4` max-abs. Independently, the
//! first and last `n_fft` (4096) samples of a reconstructed window lose
//! overlap-add contributions from the cropped frames (interior error ~`1e-10`,
//! transition band up to ~`3e-6`). Any application that stitches segments MUST
//! overlap at least `4096` samples per side and cross-fade so only interior
//! samples are used. This matches the shipped waveform models identically.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// The contract this module implements. Session/cache identities that depend on
/// the transform semantics must carry this string (issue #172 PR 2+).
pub const SPECTRAL_CONTRACT_VERSION: &str = "openkara.spectral-contract/v1";

/// Model sample rate (Hz).
pub const SAMPLE_RATE: u32 = 44_100;
/// Stereo channel count carried by the contract tensors.
pub const CHANNELS: usize = 2;
/// FFT size.
pub const N_FFT: usize = 4096;
/// Hop length (`n_fft / 4`).
pub const HOP: usize = 1024;
/// One-sided bin count before the Nyquist drop (`n_fft/2 + 1`).
pub const N_BINS: usize = N_FFT / 2 + 1;
/// One-sided bins carried by the contract tensor (Nyquist bin dropped).
pub const CONTRACT_FREQS: usize = N_FFT / 2;
/// Demucs outer reflect padding (`hop/2 · 3`).
pub const OUTER_PAD: usize = HOP / 2 * 3;
/// ISTFT overlap-add envelope floor.
pub const ENVELOPE_CLAMP: f64 = 1e-8;
/// Number of edge samples per side that are not reconstruction-exact and must
/// be covered by application-level segment overlap (see module docs).
pub const EDGE_INVALID_SAMPLES: usize = N_FFT;

/// Periodic Hann window used by the contract: `w[n] = sin²(π·n/N_FFT)`,
/// `n = 0..N_FFT` (torch `hann_window` default; NOT the symmetric variant).
///
/// Note `w[0] == 0` and `w[N_FFT/2] == 1`; the window is symmetric about
/// `N_FFT/2` for `n = 1..N_FFT`.
pub fn periodic_hann() -> Vec<f64> {
    (0..N_FFT)
        .map(|n| {
            let s = (std::f64::consts::PI * n as f64 / N_FFT as f64).sin();
            s * s
        })
        .collect()
}

/// Reflect one index `p` (which may be negative or `>= len`) into `[0, len)`
/// using torch/numpy `reflect` semantics: reflect about the edge samples
/// WITHOUT repeating them. Padding larger than the segment reflects
/// repeatedly (numpy behavior); see [`SpectralPlans::spec`] for the minimum
/// input length that keeps this to a single reflection.
fn reflect_index(p: isize, len: usize) -> usize {
    debug_assert!(len > 0);
    if len == 1 {
        return 0;
    }
    let period = 2 * (len as isize - 1);
    let mut q = p % period;
    if q < 0 {
        q += period;
    }
    if q >= len as isize {
        q = period - q;
    }
    q as usize
}

/// Reflect-pad an `f64` slice on its single axis into `out`
/// (torch/numpy `reflect`). `out` is cleared and rewritten.
fn reflect_pad_into(src: &[f64], left: usize, right: usize, out: &mut Vec<f64>) {
    let n = src.len();
    out.clear();
    out.reserve(left + n + right);
    for i in 0..left {
        let p = i as isize - left as isize;
        out.push(src[reflect_index(p, n)]);
    }
    out.extend_from_slice(src);
    for i in 0..right {
        let p = (n + i) as isize;
        out.push(src[reflect_index(p, n)]);
    }
}

/// Reflect-pad an `f32` slice into an `f64` `out` buffer (torch/numpy
/// `reflect`), converting to `f64` in the same pass. `out` is cleared.
fn reflect_pad_from_f32(src: &[f32], left: usize, right: usize, out: &mut Vec<f64>) {
    let n = src.len();
    out.clear();
    out.reserve(left + n + right);
    for i in 0..left {
        let p = i as isize - left as isize;
        out.push(src[reflect_index(p, n)] as f64);
    }
    out.extend(src.iter().map(|&v| v as f64));
    for i in 0..right {
        let p = (n + i) as isize;
        out.push(src[reflect_index(p, n)] as f64);
    }
}

/// `ceil(a / b)` for positive `b`.
#[inline]
fn div_ceil(a: usize, b: usize) -> usize {
    a.div_ceil(b)
}

/// Number of forward frames (`= le`) produced by [`SpectralPlans::spec`] for a
/// signal of `samples` length: `le = ceil(samples / hop)`.
pub fn forward_frames(samples: usize) -> usize {
    div_ceil(samples, HOP)
}

/// Neural-core magnitude view (`cac = true`): reshape `[B, C, 2, F, T]` into
/// `[B, C·2, F, T]`, channel-major `[L_re, L_im, R_re, R_im]`.
///
/// The spectral tensor produced by [`SpectralPlans::spec`] is already laid out
/// `[C, 2, F, T]` contiguously, so this reshape is a pure reinterpretation of
/// the same bytes — the returned slice aliases the input. (Accordingly the
/// golden `magnitude` and `spectral` arrays share one digest.)
pub fn magnitude(spectral: &[f32]) -> &[f32] {
    spectral
}

/// Reusable FFT plans, windows, and scratch keyed by the contract shape
/// (`n_fft = 4096`). Create once and reuse across calls: no per-call heap
/// allocation happens on the hot path except the freshly returned output
/// buffer and (for variable-length inputs) growth of the internal padding /
/// overlap-add scratch, which is amortized for repeated same-size calls.
pub struct SpectralPlans {
    forward: Arc<dyn RealToComplex<f64>>,
    inverse: Arc<dyn ComplexToReal<f64>>,
    /// Periodic Hann window, `N_FFT` samples.
    window: Vec<f64>,
    /// `window[n]²`, precomputed for the ISTFT overlap-add envelope.
    window_sq: Vec<f64>,
    /// FFT scratch for the forward (R2C) plan.
    fwd_scratch: Vec<Complex<f64>>,
    /// FFT scratch for the inverse (C2R) plan.
    inv_scratch: Vec<Complex<f64>>,
    /// Time-domain frame buffer (`N_FFT`): R2C input / C2R output.
    frame_time: Vec<f64>,
    /// Frequency-domain frame buffer (`N_BINS`): R2C output / C2R input.
    frame_freq: Vec<Complex<f64>>,
    /// Outer-padded signal scratch (forward).
    pad_a: Vec<f64>,
    /// Center-padded signal scratch (forward).
    pad_b: Vec<f64>,
    /// Overlap-add accumulator (inverse).
    signal: Vec<f64>,
    /// Overlap-add window-square envelope (inverse).
    envelope: Vec<f64>,
}

impl Default for SpectralPlans {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectralPlans {
    /// Build the reusable plans/windows/scratch for the contract shape.
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f64>::new();
        let forward = planner.plan_fft_forward(N_FFT);
        let inverse = planner.plan_fft_inverse(N_FFT);
        let window = periodic_hann();
        let window_sq = window.iter().map(|w| w * w).collect();
        let fwd_scratch = forward.make_scratch_vec();
        let inv_scratch = inverse.make_scratch_vec();
        let frame_time = forward.make_input_vec();
        let frame_freq = forward.make_output_vec();
        Self {
            forward,
            inverse,
            window,
            window_sq,
            fwd_scratch,
            inv_scratch,
            frame_time,
            frame_freq,
            pad_a: Vec::new(),
            pad_b: Vec::new(),
            signal: Vec::new(),
            envelope: Vec::new(),
        }
    }

    /// The contract normalization factor `1/√N_FFT`, applied in both directions.
    #[inline]
    fn norm() -> f64 {
        1.0 / (N_FFT as f64).sqrt()
    }

    /// Forward transform: waveform `[C, samples]` → spectral tensor
    /// `[C, 2, CONTRACT_FREQS, le]` (contiguous, `{real, imag}` interleaved on
    /// the size-2 axis), matching the reference `spec`.
    ///
    /// `x` is the channel-major waveform of length `channels * samples`
    /// (`B = 1` is implied by the contract). Returns a fresh
    /// `channels · 2 · 2048 · le` buffer with `le = ceil(samples / hop)`.
    ///
    /// # Minimum input length
    ///
    /// The transform reflect-pads the raw signal by `OUTER_PAD` on the left and
    /// `OUTER_PAD + le·hop − samples` (≤ `OUTER_PAD + hop − 1 = 2559`) on the
    /// right, then the centered STFT reflect-pads the result by `N_FFT/2` more.
    /// `reflect_index` reproduces numpy's repeated reflection for very short
    /// inputs, but the contract's production window (343980 samples) and all
    /// golden fixtures (≥ 10000 samples) keep every reflection to a single
    /// bounce. Inputs shorter than ~`2559` samples still transform, but the
    /// left/right reflections wrap and are outside the validated regime.
    ///
    /// # Panics
    ///
    /// Panics if `x.len() != channels * samples`.
    pub fn spec(&mut self, x: &[f32], channels: usize, samples: usize) -> Vec<f32> {
        let mut out = Vec::new();
        self.spec_into(x, channels, samples, &mut out);
        out
    }

    /// [`Self::spec`] into a caller-owned buffer: `out` is cleared and
    /// resized to `channels · 2 · CONTRACT_FREQS · le`, so a buffer reused
    /// across same-size calls performs no steady-state allocation
    /// (issue #172 PR 3).
    pub fn spec_into(&mut self, x: &[f32], channels: usize, samples: usize, out: &mut Vec<f32>) {
        assert_eq!(
            x.len(),
            channels * samples,
            "waveform length must equal channels * samples"
        );
        let le = div_ceil(samples, HOP);
        let pad_r = OUTER_PAD + le * HOP - samples;
        let t = le;
        let norm = Self::norm();
        out.clear();
        out.resize(channels * 2 * CONTRACT_FREQS * t, 0.0f32);

        for c in 0..channels {
            let chan = &x[c * samples..(c + 1) * samples];
            // Outer Demucs reflect padding (f32 -> f64 in one pass) into pad_a,
            // then the centered STFT reflect padding of N_FFT/2 on each side
            // into pad_b. `pad_a` and `pad_b` are disjoint fields, so the
            // second call borrows one shared and one mutable without cloning.
            reflect_pad_from_f32(chan, OUTER_PAD, pad_r, &mut self.pad_a);
            reflect_pad_into(&self.pad_a, N_FFT / 2, N_FFT / 2, &mut self.pad_b);
            let padded = &self.pad_b;
            let n_frames = 1 + (padded.len() - N_FFT) / HOP;
            debug_assert_eq!(n_frames, le + 4, "padded STFT must have le+4 frames");

            for tf in 0..t {
                // Frame crop [2 : 2+le]: contract frame `tf` is STFT frame `tf+2`.
                let start = (tf + 2) * HOP;
                let src = &padded[start..start + N_FFT];
                for (dst, (&s, &w)) in self
                    .frame_time
                    .iter_mut()
                    .zip(src.iter().zip(self.window.iter()))
                {
                    *dst = s * w;
                }
                self.forward
                    .process_with_scratch(
                        &mut self.frame_time,
                        &mut self.frame_freq,
                        &mut self.fwd_scratch,
                    )
                    .expect("forward FFT length matches plan");

                let re_base = (c * 2) * CONTRACT_FREQS * t + tf;
                let im_base = (c * 2 + 1) * CONTRACT_FREQS * t + tf;
                for (f, bin) in self.frame_freq[..CONTRACT_FREQS].iter().enumerate() {
                    out[re_base + f * t] = (bin.re * norm) as f32;
                    out[im_base + f * t] = (bin.im * norm) as f32;
                }
            }
        }
    }

    /// Inverse transform: spectral tensor `[C, 2, CONTRACT_FREQS, T]` → waveform
    /// `[C, length]`, matching the reference `ispec` (single-source view; the
    /// source axis and stem composition arrive with issue #172 PR 2).
    ///
    /// Re-appends a zero Nyquist bin, re-pads two frames on each side, runs the
    /// one-sided inverse DFT (hermitian doubling on interior bins), performs
    /// windowed overlap-add with the envelope clamped at [`ENVELOPE_CLAMP`],
    /// crops `N_FFT/2` from the front, then crops `[OUTER_PAD : OUTER_PAD+length]`.
    ///
    /// # Panics
    ///
    /// Panics if `z.len()` is not a multiple of `channels · 2 · CONTRACT_FREQS`.
    pub fn ispec(&mut self, z: &[f32], channels: usize, length: usize) -> Vec<f32> {
        let mut out = Vec::new();
        self.ispec_into(z, channels, length, &mut out);
        out
    }

    /// [`Self::ispec`] into a caller-owned buffer: `out` is cleared and
    /// resized to `channels · length`, so a buffer reused across same-size
    /// calls performs no steady-state allocation (issue #172 PR 3).
    pub fn ispec_into(&mut self, z: &[f32], channels: usize, length: usize, out: &mut Vec<f32>) {
        let per_chan = 2 * CONTRACT_FREQS;
        assert_eq!(
            z.len() % (channels * per_chan),
            0,
            "spectral tensor length must be a multiple of channels * 2 * CONTRACT_FREQS"
        );
        let tin = z.len() / (channels * per_chan);
        let n_frames = tin + 4; // two zero frames re-padded on each side
        let out_len_full = N_FFT + HOP * (n_frames - 1);
        let norm = Self::norm();
        let front = N_FFT / 2;
        out.clear();
        out.resize(channels * length, 0.0f32);

        for c in 0..channels {
            self.signal.clear();
            self.signal.resize(out_len_full, 0.0);
            self.envelope.clear();
            self.envelope.resize(out_len_full, 0.0);

            for jf in 0..n_frames {
                // Build the 2049-bin one-sided spectrum for this frame.
                if jf >= 2 && jf < 2 + tin {
                    let df = jf - 2;
                    let re_base = (c * 2) * CONTRACT_FREQS * tin + df;
                    let im_base = (c * 2 + 1) * CONTRACT_FREQS * tin + df;
                    for (f, bin) in self.frame_freq[..CONTRACT_FREQS].iter_mut().enumerate() {
                        *bin =
                            Complex::new(z[re_base + f * tin] as f64, z[im_base + f * tin] as f64);
                    }
                } else {
                    for bin in self.frame_freq[..CONTRACT_FREQS].iter_mut() {
                        *bin = Complex::new(0.0, 0.0);
                    }
                }
                // Zero Nyquist bin, and a zero DC imaginary part (both are
                // required to be real for the C2R inverse; the reference's
                // sin(0)/sin(πn) terms make them contribute nothing regardless).
                self.frame_freq[CONTRACT_FREQS] = Complex::new(0.0, 0.0);
                self.frame_freq[0].im = 0.0;

                self.inverse
                    .process_with_scratch(
                        &mut self.frame_freq,
                        &mut self.frame_time,
                        &mut self.inv_scratch,
                    )
                    .expect("inverse FFT length matches plan");

                let start = jf * HOP;
                let sig = &mut self.signal[start..start + N_FFT];
                let env = &mut self.envelope[start..start + N_FFT];
                for (((s, e), &ft), (&w, &wsq)) in sig
                    .iter_mut()
                    .zip(env.iter_mut())
                    .zip(self.frame_time.iter())
                    .zip(self.window.iter().zip(self.window_sq.iter()))
                {
                    *s += ft * w * norm;
                    *e += wsq;
                }
            }

            // Envelope division (clamped), front crop of N_FFT/2, outer crop of
            // OUTER_PAD, and final crop to `length` — folded into one indexing.
            for k in 0..length {
                let idx = front + OUTER_PAD + k;
                let env = self.envelope[idx].max(ENVELOPE_CLAMP);
                out[c * length + k] = (self.signal[idx] / env) as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_variants_match_allocating_apis_and_reuse_buffers() {
        let mut plans = SpectralPlans::new();
        let samples = 10_240;
        let x: Vec<f32> = (0..CHANNELS * samples)
            .map(|i| ((i as f32 * 0.37).sin()) * 0.1)
            .collect();

        let z = plans.spec(&x, CHANNELS, samples);
        let y = plans.ispec(&z, CHANNELS, samples);

        let mut z_buf = Vec::new();
        let mut y_buf = Vec::new();
        plans.spec_into(&x, CHANNELS, samples, &mut z_buf);
        plans.ispec_into(&z_buf, CHANNELS, samples, &mut y_buf);
        assert_eq!(z, z_buf, "spec_into must match spec exactly");
        assert_eq!(y, y_buf, "ispec_into must match ispec exactly");

        // Same-size reuse performs no reallocation (issue #172 PR 3).
        let z_ptr = z_buf.as_ptr();
        let y_ptr = y_buf.as_ptr();
        plans.spec_into(&x, CHANNELS, samples, &mut z_buf);
        plans.ispec_into(&z_buf, CHANNELS, samples, &mut y_buf);
        assert_eq!(z_buf.as_ptr(), z_ptr, "spec_into reallocated its buffer");
        assert_eq!(y_buf.as_ptr(), y_ptr, "ispec_into reallocated its buffer");
        assert_eq!(z, z_buf);
        assert_eq!(y, y_buf);
    }

    #[test]
    fn window_is_periodic_hann() {
        let w = periodic_hann();
        assert_eq!(w.len(), N_FFT);
        // Periodic Hann starts at zero (torch default, not the symmetric variant).
        assert_eq!(w[0], 0.0);
        // Center sample is exactly 1.0 (sin(π/2)² = 1).
        assert!((w[N_FFT / 2] - 1.0).abs() < 1e-12);
        // sin² identity: sin²(x) == (1 − cos 2x)/2.
        for (n, &wn) in w.iter().enumerate() {
            let x = std::f64::consts::PI * n as f64 / N_FFT as f64;
            let cosine_form = 0.5 - 0.5 * (2.0 * x).cos();
            assert!((wn - cosine_form).abs() < 1e-12, "n={n}");
        }
        // Symmetric about the center for n = 1..N_FFT (w[n] == w[N_FFT-n]).
        for n in 1..N_FFT {
            assert!((w[n] - w[N_FFT - n]).abs() < 1e-12, "n={n}");
        }
    }

    #[test]
    fn cola_envelope_is_constant_in_the_interior() {
        // Hann² with 75% overlap (hop = N_FFT/4) satisfies the COLA condition:
        // the sum of window² over hop-spaced frames is constant in the interior.
        let w = periodic_hann();
        let wsq: Vec<f64> = w.iter().map(|v| v * v).collect();
        let frames = 12usize;
        let len = N_FFT + HOP * (frames - 1);
        let mut env = vec![0.0f64; len];
        for t in 0..frames {
            for (i, &s) in wsq.iter().enumerate() {
                env[t * HOP + i] += s;
            }
        }
        // Interior (fully overlapped) region: at least N_FFT past the start and
        // before the end. The COLA constant for Hann² at 75% overlap is 1.5.
        let interior = &env[N_FFT..len - N_FFT];
        let first = interior[0];
        assert!((first - 1.5).abs() < 1e-9, "COLA constant = {first}");
        for &v in interior {
            assert!((v - first).abs() < 1e-9);
        }
        // Every accumulated envelope value stays well above the clamp floor in
        // the interior, so the ENVELOPE_CLAMP path only guards the edges.
        assert!(first > ENVELOPE_CLAMP);
    }

    #[test]
    fn reflect_index_matches_numpy_single_and_repeated() {
        // [1,2,3,4] pad(2,2) -> [3,2,1,2,3,4,3,2]
        let src = [1, 2, 3, 4];
        let left: Vec<i32> = (0..2)
            .map(|i| src[reflect_index(i as isize - 2, 4)])
            .collect();
        let right: Vec<i32> = (0..2)
            .map(|i| src[reflect_index((4 + i) as isize, 4)])
            .collect();
        assert_eq!(left, vec![3, 2]);
        assert_eq!(right, vec![3, 2]);

        // Repeated reflection when pad > len: [1,2,3] pad(4,0) -> [1,2,3,2,1,2,3]
        let src3 = [1, 2, 3];
        let left4: Vec<i32> = (0..4)
            .map(|i| src3[reflect_index(i as isize - 4, 3)])
            .collect();
        assert_eq!(left4, vec![1, 2, 3, 2]);
        // [1,2,3] pad(0,5) -> [1,2,3,2,1,2,3,2]
        let right5: Vec<i32> = (0..5)
            .map(|i| src3[reflect_index((3 + i) as isize, 3)])
            .collect();
        assert_eq!(right5, vec![2, 1, 2, 3, 2]);

        // len == 1 always maps to index 0.
        assert_eq!(reflect_index(-3, 1), 0);
        assert_eq!(reflect_index(7, 1), 0);
    }

    #[test]
    fn reflect_pad_into_matches_numpy() {
        let mut out = Vec::new();
        reflect_pad_into(&[1.0, 2.0, 3.0, 4.0], 2, 2, &mut out);
        assert_eq!(out, vec![3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0]);
    }

    #[test]
    fn forward_frame_count_math() {
        assert_eq!(forward_frames(10240), 10); // exact multiple of hop
        assert_eq!(forward_frames(10000), 10); // ceil(10000/1024) = 10
        assert_eq!(forward_frames(1024), 1);
        assert_eq!(forward_frames(1025), 2);
        assert_eq!(forward_frames(343_980), 336); // contract fixed window
    }

    #[test]
    fn spec_output_shape_math() {
        let mut plans = SpectralPlans::new();
        let samples = 4096usize;
        let x = vec![0.0f32; CHANNELS * samples];
        let z = plans.spec(&x, CHANNELS, samples);
        let le = forward_frames(samples);
        assert_eq!(z.len(), CHANNELS * 2 * CONTRACT_FREQS * le);
        // Silence in -> zeros out.
        assert!(z.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn ispec_of_zeros_is_finite_silence() {
        // Envelope clamp guards the division: an all-zero spectral tensor must
        // round-trip to finite zeros, never NaN/inf from a zero envelope.
        let mut plans = SpectralPlans::new();
        let length = 10240usize;
        let tin = forward_frames(length);
        let z = vec![0.0f32; CHANNELS * 2 * CONTRACT_FREQS * tin];
        let y = plans.ispec(&z, CHANNELS, length);
        assert_eq!(y.len(), CHANNELS * length);
        assert!(y.iter().all(|&v| v == 0.0 && v.is_finite()));
    }

    #[test]
    fn plan_reuse_is_deterministic() {
        // Two calls on one reused plan must produce identical output, proving
        // scratch buffers are fully overwritten between calls.
        let mut plans = SpectralPlans::new();
        let samples = 8192usize;
        let mut x = vec![0.0f32; CHANNELS * samples];
        for (n, v) in x.iter_mut().enumerate() {
            *v = ((n % 97) as f32 / 97.0) - 0.5;
        }
        let a = plans.spec(&x, CHANNELS, samples);
        let b = plans.spec(&x, CHANNELS, samples);
        assert_eq!(a, b);

        // And a differently-sized call in between must not corrupt a repeat.
        let _ = plans.spec(&vec![0.1f32; CHANNELS * 2048], CHANNELS, 2048);
        let cc = plans.spec(&x, CHANNELS, samples);
        assert_eq!(a, cc);
    }

    #[test]
    fn tone_round_trips_in_the_interior() {
        // Band-limited (single-tone) content reconstructs exactly in the
        // interior; only the documented edge regions are lossy.
        let mut plans = SpectralPlans::new();
        let samples = 44_100usize;
        let mut x = vec![0.0f32; CHANNELS * samples];
        for c in 0..CHANNELS {
            for n in 0..samples {
                x[c * samples + n] =
                    (2.0 * std::f64::consts::PI * 440.0 * n as f64 / samples as f64).sin() as f32;
            }
        }
        let z = plans.spec(&x, CHANNELS, samples);
        let y = plans.ispec(&z, CHANNELS, samples);
        let mut max_interior = 0.0f32;
        for c in 0..CHANNELS {
            for n in EDGE_INVALID_SAMPLES..samples - EDGE_INVALID_SAMPLES {
                let d = (y[c * samples + n] - x[c * samples + n]).abs();
                max_interior = max_interior.max(d);
            }
        }
        assert!(
            max_interior < 1e-5,
            "tone interior max abs = {max_interior}"
        );
    }
}
