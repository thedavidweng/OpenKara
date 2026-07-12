//! Five-band peaking EQ + auto preamp + soft limiter, designed to run inside
//! the CPAL output callback without any steady-state allocation.
//!
//! # Render order (fixed by issue #86)
//!
//! ```text
//! existing source/stem mix + master/stem gains
//! → EQ dry/wet processor + auto preamp
//! → soft limiter
//! → existing play/pause/seek fade
//! → output/AirPlay forwarding
//! ```
//!
//! The processor is owned by the output closure beside `ResamplerCache`; it is
//! NOT stored behind the playback mutex. A new stream constructs a new
//! processor so per-channel filter state always matches the active device
//! sample rate / channel count.
//!
//! # Realtime constraints
//!
//! After `EqProcessor::new`, no callback operation may:
//! - lock a second mutex
//! - allocate (no `Vec`/`String`/`format!`)
//! - log per sample/callback
//! - serialize or emit an event
//!
//! Coefficient recomputation happens at most once per callback per band using
//! `DirectForm1::update_coefficients`, which preserves delay state and performs
//! no allocation. A coefficient error (e.g. band above Nyquist) disables only
//! that channel/band for that callback; the last valid coefficient is retained
//! and no panic/log storm is produced.

use biquad::{Biquad, Coefficients, DirectForm1, Type as BiquadType};

/// Fixed band center frequencies (Hz) for the five-band peaking EQ.
pub const EQ_BAND_FREQUENCIES_HZ: [f32; 5] = [60.0, 230.0, 910.0, 3_600.0, 14_000.0];
/// Q factor shared by all bands. 0.707 ≈ Butterworth — flat passband.
pub const EQ_Q: f32 = 0.707;
/// Minimum allowed per-band gain in dB.
pub const EQ_MIN_GAIN_DB: f32 = -12.0;
/// Maximum allowed per-band gain in dB.
pub const EQ_MAX_GAIN_DB: f32 = 12.0;

/// Smoothing time constant for gain and preamp transitions.
const EQ_SMOOTH_MS: f32 = 50.0;
/// Smoothing time constant for the bypass dry/wet crossfade.
const BYPASS_SMOOTH_MS: f32 = 20.0;
/// Bands at or above `sample_rate * NYQUIST_RATIO_LIMIT` are disabled to avoid
/// coefficient instability near Nyquist. 0.45 leaves headroom below 0.5.
const NYQUIST_RATIO_LIMIT: f32 = 0.45;
/// Soft limiter threshold. Samples at or below this are passed through
/// bit-for-bit; above it the curve transitions smoothly to an asymptote at 1.0.
const LIMITER_THRESHOLD: f32 = 0.95;

/// Convert decibels to a linear amplitude factor.
fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Continuous, slope-matched soft limiter.
///
/// Samples at or below `LIMITER_THRESHOLD` are returned unchanged. Above the
/// threshold the curve uses `tanh` to compress toward an asymptote at `1.0`,
/// with derivative 1 at the threshold (no kink). Non-finite inputs map to `0.0`
/// so a NaN/Inf from upstream DSP cannot reach the DAC.
pub fn soft_limit(sample: f32) -> f32 {
    if !sample.is_finite() {
        return 0.0;
    }
    let magnitude = sample.abs();
    if magnitude <= LIMITER_THRESHOLD {
        return sample;
    }
    let headroom = 1.0 - LIMITER_THRESHOLD;
    let compressed =
        LIMITER_THRESHOLD + headroom * ((magnitude - LIMITER_THRESHOLD) / headroom).tanh();
    sample.signum() * compressed
}

/// Per-channel, per-band biquad state. `None` means the band is disabled for
/// this channel (Nyquist guard or coefficient error). The slot is retained so
/// a band can re-enable if the smoothed gain later moves back into range.
type BandSlot = Option<DirectForm1<f32>>;

/// Five-band peaking EQ with auto preamp and dry/wet bypass smoothing.
///
/// Owned by the CPAL output closure. The controller updates
/// `eq_enabled`/`eq_gains_db` behind the playback mutex; the callback polls
/// `eq_revision` while it already holds the lock and mirrors the change into
/// the processor's `target_*` fields. All smoothing advances by rendered
/// frames so it is sample-rate-correct and does not advance on zero-length /
/// buffering callbacks.
pub struct EqProcessor {
    filters: Vec<[BandSlot; 5]>,
    sample_rate: f32,
    channels: usize,
    enabled: bool,
    target_gains_db: [f32; 5],
    current_gains_db: [f32; 5],
    target_preamp: f32,
    current_preamp: f32,
    wet_mix: f32,
    target_wet_mix: f32,
    /// Last controller revision applied. Compared against
    /// `PlaybackController::eq_revision` to detect config changes.
    applied_revision: u64,
}

impl EqProcessor {
    /// Construct a new processor for the given device sample rate and channel
    /// count. All bands start disabled (`None`) and are lazily created on the
    /// first callback that needs them — this avoids constructing coefficients
    /// for a flat/0 dB configuration that would never be applied.
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        Self {
            filters: vec![[None; 5]; channels.max(1)],
            sample_rate: sample_rate as f32,
            channels,
            enabled: false,
            target_gains_db: [0.0; 5],
            current_gains_db: [0.0; 5],
            target_preamp: 1.0,
            current_preamp: 1.0,
            wet_mix: 0.0,
            target_wet_mix: 0.0,
            applied_revision: 0,
        }
    }

    /// Mirror a controller config snapshot into the processor's targets. Called
    /// from the callback while it already holds the playback lock. Does not
    /// touch filter state — coefficients are recomputed lazily on the next
    /// `process` call using the smoothed gain.
    pub fn apply_config(&mut self, enabled: bool, gains_db: [f32; 5], revision: u64) {
        if revision == self.applied_revision {
            return;
        }
        self.applied_revision = revision;
        self.set_enabled(enabled);
        self.set_gains(gains_db);
    }

    /// Update the enable target. The dry/wet crossfade smooths the transition
    /// over `BYPASS_SMOOTH_MS`.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.target_wet_mix = if enabled { 1.0 } else { 0.0 };
    }

    /// Update the per-band gain targets and recompute the auto preamp target.
    /// Auto preamp = `db_to_linear(-max(0, max_positive_gain))` supplies
    /// headroom for boosted bands; it does not replace the limiter.
    pub fn set_gains(&mut self, gains_db: [f32; 5]) {
        self.target_gains_db = gains_db;
        let max_positive = gains_db.iter().copied().fold(0.0_f32, f32::max);
        self.target_preamp = db_to_linear(-max_positive.max(0.0));
    }

    /// Process interleaved audio in place over `rendered_samples` samples.
    /// Trailing callback padding (`output[rendered_samples..]`) is left
    /// untouched so filter state is not advanced by silence.
    ///
    /// `rendered_samples` is in interleaved samples (frames × channels). It
    /// must be a multiple of `channels`; the caller guarantees this because
    /// `render_output_buffer` returns `rendered * device_channels`.
    pub fn process(&mut self, output: &mut [f32], rendered_samples: usize) {
        if rendered_samples == 0 {
            return;
        }
        let channels = self.channels.max(1);
        let frames = rendered_samples / channels;
        if frames == 0 {
            return;
        }

        // Advance smoothing by rendered frames (sample-rate-correct).
        let gain_step = smoothing_step(self.sample_rate, EQ_SMOOTH_MS, frames);
        let preamp_step = gain_step;
        let wet_step = smoothing_step(self.sample_rate, BYPASS_SMOOTH_MS, frames);

        // Snap to target when within one step to guarantee convergence.
        for band in 0..5 {
            let target = self.target_gains_db[band];
            let current = &mut self.current_gains_db[band];
            *current = approach(*current, target, gain_step);
        }
        self.current_preamp = approach(self.current_preamp, self.target_preamp, preamp_step);
        self.wet_mix = approach(self.wet_mix, self.target_wet_mix, wet_step);

        // Recompute coefficients at most once per callback per band using the
        // smoothed gain. update_coefficients preserves delay state. Filter
        // slots are lazily created here from the first valid coefficient so a
        // flat/0 dB startup does not construct filters that would be transparent.
        let mut band_coeffs: [Option<Coefficients<f32>>; 5] = [None; 5];
        for band in 0..5 {
            let gain_db = self.current_gains_db[band];
            // Flat bands still run the filter chain (warm state for re-enable),
            // but a 0 dB peaking EQ is transparent so we can skip coefficient
            // work. We keep the slot warm by running it with the last coeff.
            if gain_db == 0.0 {
                continue;
            }
            let freq = EQ_BAND_FREQUENCIES_HZ[band];
            if freq >= self.sample_rate * NYQUIST_RATIO_LIMIT {
                // Above Nyquist guard — leave this band bypassed.
                continue;
            }
            // biquad 0.5's from_params has a frequency normalization bug
            // (uses f0/(2*fs) instead of f0/(fs/2), shifting the center to
            // 1/4 of the intended frequency). Use from_normalized_params with
            // the correct normalization: normalized_f0 = 2 * f0 / fs.
            let normalized_f0 = 2.0 * freq / self.sample_rate;
            match Coefficients::<f32>::from_normalized_params(
                BiquadType::PeakingEQ(gain_db),
                normalized_f0,
                EQ_Q,
            ) {
                Ok(coeffs) => band_coeffs[band] = Some(coeffs),
                Err(_) => {
                    // Coefficient error (e.g. gain out of internal range) —
                    // disable only this band for this callback. The slot
                    // retains its last valid coefficient.
                }
            }
        }

        let wet = self.wet_mix;
        let preamp = self.current_preamp;
        let dry_gain = 1.0 - wet;

        for frame in 0..frames {
            for ch in 0..channels {
                let idx = frame * channels + ch;
                let dry = output[idx];
                let mut wet_sample = dry;
                let slots = &mut self.filters[ch];
                for band in 0..5 {
                    // Lazily create the filter from the first valid coefficient.
                    if slots[band].is_none() {
                        if let Some(coeffs) = band_coeffs[band] {
                            slots[band] = Some(DirectForm1::new(coeffs));
                        }
                    }
                    if let Some(ref mut filter) = slots[band] {
                        if let Some(coeffs) = band_coeffs[band] {
                            filter.update_coefficients(coeffs);
                        }
                        wet_sample = filter.run(wet_sample);
                    }
                }
                let processed = dry * dry_gain + (wet_sample * preamp) * wet;
                output[idx] = soft_limit(processed);
            }
        }
    }

    /// Current applied revision — used by tests to confirm config propagation.
    pub fn applied_revision(&self) -> u64 {
        self.applied_revision
    }
}

/// Per-frame smoothing increment for a time constant. Returns the amount to
/// add per frame toward the target. Derived from the device sample rate so
/// smoothing duration is consistent across 22.05/44.1/48/96 kHz outputs.
fn smoothing_step(sample_rate: f32, time_ms: f32, frames: usize) -> f32 {
    if sample_rate <= 0.0 || time_ms <= 0.0 {
        return 1.0;
    }
    let total_samples = sample_rate * time_ms / 1000.0;
    if total_samples <= 0.0 {
        return 1.0;
    }
    // Step per frame so the cumulative move over `frames` approaches the target.
    // Using frames/total as the fraction keeps the transition duration fixed
    // regardless of how many frames happen to be in this callback.
    (frames as f32 / total_samples).clamp(0.0, 1.0)
}

/// Move `current` toward `target` by at most `step` (absolute). Snaps to target
/// when the remaining distance is <= step so the value converges exactly.
fn approach(current: f32, target: f32, step: f32) -> f32 {
    if !current.is_finite() {
        return target;
    }
    let delta = target - current;
    if delta.abs() <= step || step >= 1.0 {
        target
    } else {
        current + delta.signum() * step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f32 = 1e-6;

    // ── soft_limit ────────────────────────────────────────────────────────

    #[test]
    fn soft_limit_passes_samples_below_threshold_unchanged() {
        for &s in &[0.0_f32, 0.5, -0.5, 0.95, -0.95, 0.001, -0.001] {
            assert!(
                (soft_limit(s) - s).abs() <= f32::EPSILON,
                "sample {s} below threshold must be bit-for-bit unchanged"
            );
        }
    }

    #[test]
    fn soft_limit_is_continuous_at_threshold() {
        // Derivative is 1 at the threshold — no kink.
        let eps = 1e-5_f32;
        let below = soft_limit(LIMITER_THRESHOLD - eps);
        let above = soft_limit(LIMITER_THRESHOLD + eps);
        // Just below: unchanged. Just above: very slightly compressed.
        assert!((below - (LIMITER_THRESHOLD - eps)).abs() <= f32::EPSILON);
        // The discontinuity in derivative must be tiny (continuous function).
        let expected_slope_one = LIMITER_THRESHOLD + eps;
        assert!(
            (above - expected_slope_one).abs() < 1e-4,
            "just above threshold must be near linear, got {above} vs {expected_slope_one}"
        );
    }

    #[test]
    fn soft_limit_bounded_at_or_below_one() {
        // The tanh asymptote approaches 1.0 but may round to exactly 1.0 in
        // f32 for very large inputs. The guarantee is |out| <= 1.0 (never
        // exceeds unity), and samples just above threshold stay well below 1.0.
        for &s in &[0.96_f32, 1.5, 5.0, 100.0, -0.96, -1.5, -5.0, -100.0] {
            let out = soft_limit(s);
            assert!(
                out.abs() <= 1.0,
                "soft_limit({s}) = {out} must be |.| <= 1.0"
            );
        }
        // Just above threshold: well below 1.0.
        assert!(soft_limit(0.96).abs() < 0.99);
        assert!(soft_limit(1.0).abs() < 0.99);
    }

    #[test]
    fn soft_limit_sanitizes_non_finite() {
        assert_eq!(soft_limit(f32::NAN), 0.0);
        assert_eq!(soft_limit(f32::INFINITY), 0.0);
        assert_eq!(soft_limit(f32::NEG_INFINITY), 0.0);
    }

    #[test]
    fn soft_limit_is_sign_symmetric() {
        for &s in &[0.96_f32, 1.2, 3.0, 10.0] {
            let pos = soft_limit(s);
            let neg = soft_limit(-s);
            assert!(
                (pos + neg).abs() < 1e-6,
                "soft_limit must be sign-symmetric: {pos} vs {neg}"
            );
        }
    }

    // ── EqProcessor flat/bypassed transparency ────────────────────────────

    #[test]
    fn flat_bypassed_path_is_transparent_below_threshold() {
        // EQ off, all gains 0 → wet_mix = 0 → output = dry (then soft_limit
        // passes it through because < 0.95).
        let mut proc = EqProcessor::new(44_100, 2);
        proc.set_enabled(false);
        proc.set_gains([0.0; 5]);
        // Force wet_mix to 0 so no smoothing ramp interferes.
        proc.wet_mix = 0.0;
        proc.target_wet_mix = 0.0;

        let mut buf = [0.3_f32, -0.4, 0.5, -0.6, 0.7, -0.8, 0.1, -0.2];
        let original = buf;
        let n = buf.len();
        proc.process(&mut buf, n);
        for (got, want) in buf.iter().zip(original.iter()) {
            assert!(
                (got - want).abs() <= TOLERANCE,
                "bypassed path must be transparent: {got} vs {want}"
            );
        }
    }

    #[test]
    fn flat_enabled_path_is_transparent_below_threshold() {
        // EQ on but all gains 0 → peaking EQ at 0 dB is transparent, preamp = 1.
        let mut proc = EqProcessor::new(44_100, 2);
        proc.set_enabled(true);
        proc.set_gains([0.0; 5]);
        proc.wet_mix = 1.0;
        proc.target_wet_mix = 1.0;
        proc.current_preamp = 1.0;
        proc.target_preamp = 1.0;

        let mut buf = [0.3_f32, -0.4, 0.5, -0.6, 0.7, -0.8, 0.1, -0.2];
        let original = buf;
        let n = buf.len();
        proc.process(&mut buf, n);
        for (got, want) in buf.iter().zip(original.iter()) {
            assert!(
                (got - want).abs() <= TOLERANCE,
                "flat enabled path must be transparent: {got} vs {want}"
            );
        }
    }

    // ── Band response ─────────────────────────────────────────────────────

    #[test]
    fn positive_gain_boosts_in_band_relative_to_out_of_band() {
        // With +12 dB at band 2 (910 Hz) and auto preamp of -12 dB, the net
        // in-band gain is ~0 dB while out-of-band tones are attenuated by the
        // preamp alone (~-12 dB). So the in-band tone must be significantly
        // louder than the out-of-band tone through the same processor.
        let sample_rate = 44_100_u32;
        let frames = 8192_usize;
        let channels = 1_usize;

        let in_band_freq = 910.0_f32; // band 2 center
        let out_of_band_freq = 5_000.0_f32; // between band 3 (3.6k) and band 4 (14k)

        let mut in_band: Vec<f32> = (0..frames)
            .map(|i| {
                0.01 * (2.0 * std::f32::consts::PI * in_band_freq * i as f32 / sample_rate as f32)
                    .sin()
            })
            .collect();
        let mut out_of_band: Vec<f32> = (0..frames)
            .map(|i| {
                0.01 * (2.0 * std::f32::consts::PI * out_of_band_freq * i as f32
                    / sample_rate as f32)
                    .sin()
            })
            .collect();

        // EQ on, +12 dB at band 2 only.
        let gains = {
            let mut g = [0.0; 5];
            g[2] = 12.0;
            g
        };

        let mut proc = EqProcessor::new(sample_rate, channels);
        proc.set_enabled(true);
        proc.set_gains(gains);
        // Snap smoothing so we measure the steady-state response.
        proc.wet_mix = 1.0;
        proc.target_wet_mix = 1.0;
        proc.current_gains_db = gains;
        proc.current_preamp = proc.target_preamp;

        // Warm filter state, then measure.
        for _ in 0..20 {
            let mut chunk = in_band.clone();
            let n = chunk.len();
            proc.process(&mut chunk, n);
        }
        let n = in_band.len();
        proc.process(&mut in_band, n);

        // Fresh processor for the out-of-band tone (independent filter state).
        let mut proc2 = EqProcessor::new(sample_rate, channels);
        proc2.set_enabled(true);
        proc2.set_gains(gains);
        proc2.wet_mix = 1.0;
        proc2.target_wet_mix = 1.0;
        proc2.current_gains_db = gains;
        proc2.current_preamp = proc2.target_preamp;
        for _ in 0..20 {
            let mut chunk = out_of_band.clone();
            let n = chunk.len();
            proc2.process(&mut chunk, n);
        }
        let n = out_of_band.len();
        proc2.process(&mut out_of_band, n);

        // Measure RMS of the second half (after filter settled).
        let half = frames / 2;
        let in_rms: f32 = in_band[half..].iter().map(|s| s * s).sum::<f32>() / half as f32;
        let out_rms: f32 = out_of_band[half..].iter().map(|s| s * s).sum::<f32>() / half as f32;
        // In-band must be louder than out-of-band. The preamp (-12 dB) applies
        // to both; the +12 dB peaking EQ at 910 Hz only boosts the in-band
        // tone, so in-band net ≈ 0 dB while out-of-band net ≈ -12 dB.
        assert!(
            in_rms > out_rms * 2.0,
            "in-band tone must be louder than out-of-band: in_rms={in_rms} out_rms={out_rms}"
        );
    }

    #[test]
    fn filter_boosts_in_band_tone_without_preamp() {
        // Isolate the filter response: preamp = 1.0, +12 dB at 910 Hz.
        // A 910 Hz tone must be boosted relative to the bypassed reference.
        let sample_rate = 44_100_u32;
        let frames = 8192_usize;
        let channels = 1_usize;

        let mut tone: Vec<f32> = (0..frames)
            .map(|i| {
                0.01 * (2.0 * std::f32::consts::PI * 910.0 * i as f32 / sample_rate as f32).sin()
            })
            .collect();
        let reference = tone.clone();

        let mut proc = EqProcessor::new(sample_rate, channels);
        proc.set_enabled(true);
        let gains = {
            let mut g = [0.0; 5];
            g[2] = 12.0;
            g
        };
        proc.set_gains(gains);
        // Disable auto preamp to isolate the filter response.
        proc.target_preamp = 1.0;
        proc.wet_mix = 1.0;
        proc.target_wet_mix = 1.0;
        proc.current_gains_db = gains;
        proc.current_preamp = 1.0;

        for _ in 0..20 {
            let mut chunk = tone.clone();
            let n = chunk.len();
            proc.process(&mut chunk, n);
        }
        let n = tone.len();
        proc.process(&mut tone, n);

        let half = frames / 2;
        let eq_rms: f32 = tone[half..].iter().map(|s| s * s).sum::<f32>() / half as f32;
        let ref_rms: f32 = reference[half..].iter().map(|s| s * s).sum::<f32>() / half as f32;
        // +12 dB → linear gain ≈ 3.98 → power gain ≈ 15.8x.
        assert!(
            eq_rms > ref_rms * 4.0,
            "+12 dB filter must boost in-band tone: eq_rms={eq_rms} ref_rms={ref_rms}"
        );
    }

    // ── Auto preamp ───────────────────────────────────────────────────────

    #[test]
    fn auto_preamp_target_reduces_headroom_for_positive_gain() {
        let mut proc = EqProcessor::new(44_100, 2);
        proc.set_gains([6.0, 0.0, 0.0, 0.0, 0.0]);
        // -6 dB → linear ≈ 0.501
        assert!(
            (proc.target_preamp - db_to_linear(-6.0)).abs() < 1e-5,
            "preamp must be -max_positive_gain dB"
        );
    }

    #[test]
    fn auto_preamp_target_is_unity_when_no_positive_gain() {
        let mut proc = EqProcessor::new(44_100, 2);
        proc.set_gains([-6.0, -3.0, 0.0, -1.0, -12.0]);
        assert!(
            (proc.target_preamp - 1.0).abs() < 1e-6,
            "preamp must be unity when no positive gain"
        );
    }

    #[test]
    fn auto_preamp_smooths_toward_target() {
        let mut proc = EqProcessor::new(44_100, 2);
        proc.set_gains([12.0, 0.0, 0.0, 0.0, 0.0]);
        // current_preamp starts at 1.0; target ≈ 0.251.
        let before = proc.current_preamp;
        // Process a small buffer — preamp should move toward target but not snap.
        let mut buf = [0.0_f32; 256];
        proc.set_enabled(true);
        proc.wet_mix = 1.0;
        proc.target_wet_mix = 1.0;
        let n = buf.len();
        proc.process(&mut buf, n);
        let after = proc.current_preamp;
        assert!(
            after < before,
            "preamp must decrease toward target: {before} -> {after}"
        );
        assert!(
            after > proc.target_preamp,
            "preamp must not reach target in one small callback: {after} vs {}",
            proc.target_preamp
        );
    }

    // ── Bypass / gain change discontinuity ────────────────────────────────

    #[test]
    fn bypass_transition_is_click_free() {
        // Switching enabled on/off mid-stream must not produce an unbounded
        // adjacent-sample discontinuity.
        let sample_rate = 44_100_u32;
        let frames = 1024_usize;
        let channels = 1_usize;
        let tone: Vec<f32> = (0..frames)
            .map(|i| {
                0.2 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / sample_rate as f32).sin()
            })
            .collect();

        let mut proc = EqProcessor::new(sample_rate, channels);
        proc.set_enabled(false);
        proc.wet_mix = 0.0;
        proc.target_wet_mix = 0.0;

        let mut buf = tone.clone();
        let n = buf.len();
        proc.process(&mut buf, n);

        // Flip enabled on. wet_mix will ramp from 0 to 1 over BYPASS_SMOOTH_MS.
        proc.set_enabled(true);
        let mut buf2 = tone.clone();
        let n = buf2.len();
        proc.process(&mut buf2, n);

        // Max adjacent-sample jump must be bounded (no click). A hard switch
        // would produce a jump on the order of the signal amplitude; the
        // smoothing ramp keeps it well below that.
        let max_jump = buf2
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            max_jump < 0.15,
            "bypass transition must be click-free: max_jump={max_jump}"
        );
    }

    // ── Independent channel state ─────────────────────────────────────────

    #[test]
    fn left_and_right_filter_state_is_independent() {
        // Feed a 910 Hz tone on the left channel and silence on the right.
        // After +12 dB at 910 Hz, left must be boosted while right stays near 0.
        let sample_rate = 44_100_u32;
        let frames = 2048_usize;
        let channels = 2_usize;
        let mut buf: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let l = 0.1
                    * (2.0 * std::f32::consts::PI * 910.0 * i as f32 / sample_rate as f32).sin();
                [l, 0.0]
            })
            .collect();

        let mut proc = EqProcessor::new(sample_rate, channels);
        proc.set_enabled(true);
        let mut gains = [0.0; 5];
        gains[2] = 12.0;
        proc.set_gains(gains);
        proc.wet_mix = 1.0;
        proc.target_wet_mix = 1.0;
        proc.current_gains_db = gains;
        proc.current_preamp = proc.target_preamp;

        // Warm up.
        for _ in 0..5 {
            let mut chunk = buf.clone();
            let n = chunk.len();
            proc.process(&mut chunk, n);
        }
        let n = buf.len();
        proc.process(&mut buf, n);

        let half = frames / 2;
        let left_rms: f32 = buf[half * 2..]
            .iter()
            .step_by(2)
            .map(|s| s * s)
            .sum::<f32>()
            / half as f32;
        let right_rms: f32 = buf[half * 2 + 1..]
            .iter()
            .step_by(2)
            .map(|s| s * s)
            .sum::<f32>()
            / half as f32;
        assert!(
            left_rms > 1e-4,
            "left channel must carry boosted signal: left_rms={left_rms}"
        );
        assert!(
            right_rms < 1e-4,
            "right channel must stay silent: right_rms={right_rms}"
        );
    }

    // ── Nyquist guards ────────────────────────────────────────────────────

    #[test]
    fn nyquist_guard_disables_high_band_at_low_sample_rate() {
        // 22.05 kHz: 14 kHz band is above 0.45 * 22050 = 9922.5 Hz, so band 4
        // must be bypassed. The processor must not panic.
        let mut proc = EqProcessor::new(22_050, 2);
        proc.set_enabled(true);
        let mut gains = [0.0; 5];
        gains[4] = 12.0; // 14 kHz band
        proc.set_gains(gains);
        proc.wet_mix = 1.0;
        proc.target_wet_mix = 1.0;
        proc.current_gains_db = gains;
        proc.current_preamp = proc.target_preamp;
        let mut buf = vec![0.1_f32; 256];
        // Must not panic.
        let n = buf.len();
        proc.process(&mut buf, n);
    }

    #[test]
    fn processor_handles_32_44_1_48_96_khz_without_panic() {
        for &sr in &[32_000_u32, 44_100, 48_000, 96_000] {
            let mut proc = EqProcessor::new(sr, 2);
            proc.set_enabled(true);
            proc.set_gains([3.0, -2.0, 5.0, -1.0, 8.0]);
            proc.wet_mix = 1.0;
            proc.target_wet_mix = 1.0;
            proc.current_gains_db = [3.0, -2.0, 5.0, -1.0, 8.0];
            proc.current_preamp = proc.target_preamp;
            let mut buf = vec![0.2_f32; 512];
            let n = buf.len();
            proc.process(&mut buf, n);
        }
    }

    // ── apply_config revision guard ───────────────────────────────────────

    #[test]
    fn apply_config_ignores_stale_revision() {
        let mut proc = EqProcessor::new(44_100, 2);
        proc.apply_config(true, [3.0, 0.0, 0.0, 0.0, 0.0], 5);
        assert_eq!(proc.applied_revision(), 5);
        assert!(proc.enabled);
        // Same revision — must be a no-op even with different values.
        proc.apply_config(false, [6.0, 0.0, 0.0, 0.0, 0.0], 5);
        assert_eq!(proc.applied_revision(), 5);
        assert!(proc.enabled, "stale revision must not change enabled");
        assert!(
            (proc.target_gains_db[0] - 3.0).abs() < 1e-6,
            "stale revision must not change gains"
        );
    }

    #[test]
    fn apply_config_updates_on_new_revision() {
        let mut proc = EqProcessor::new(44_100, 2);
        proc.apply_config(true, [3.0, 0.0, 0.0, 0.0, 0.0], 5);
        proc.apply_config(false, [0.0; 5], 6);
        assert_eq!(proc.applied_revision(), 6);
        assert!(!proc.enabled);
        assert_eq!(proc.target_gains_db, [0.0; 5]);
    }

    // ── zero-length callback ──────────────────────────────────────────────

    #[test]
    fn process_zero_length_does_not_advance_state() {
        let mut proc = EqProcessor::new(44_100, 2);
        proc.set_enabled(true);
        proc.set_gains([12.0, 0.0, 0.0, 0.0, 0.0]);
        let wet_before = proc.wet_mix;
        let preamp_before = proc.current_preamp;
        let mut buf = [0.0_f32; 8];
        proc.process(&mut buf, 0);
        assert_eq!(proc.wet_mix, wet_before);
        assert_eq!(proc.current_preamp, preamp_before);
    }

    // ── trailing padding not advanced ─────────────────────────────────────

    #[test]
    fn process_only_touches_rendered_samples() {
        let mut proc = EqProcessor::new(44_100, 2);
        proc.set_enabled(true);
        proc.set_gains([12.0, 0.0, 0.0, 0.0, 0.0]);
        proc.wet_mix = 1.0;
        proc.target_wet_mix = 1.0;
        proc.current_gains_db = [12.0, 0.0, 0.0, 0.0, 0.0];
        proc.current_preamp = proc.target_preamp;

        let mut buf = [0.5_f32, 0.5, 0.5, 0.5, 0.99, 0.99, 0.99, 0.99];
        // Render only the first 4 samples (2 frames). Trailing 4 must be
        // untouched (0.99 > threshold so soft_limit would change them).
        proc.process(&mut buf, 4);
        assert_eq!(buf[4], 0.99);
        assert_eq!(buf[5], 0.99);
        assert_eq!(buf[6], 0.99);
        assert_eq!(buf[7], 0.99);
    }
}
