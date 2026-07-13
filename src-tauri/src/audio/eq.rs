//! Five-band peaking EQ with auto preamp and a bounded soft limiter.
//!
//! The `EqProcessor` is owned by the CPAL output closure beside
//! `ResamplerCache`; it is **not** stored behind the playback mutex. A new
//! stream constructs a new processor. The controller publishes an
//! `EqConfig` snapshot (enabled + gains + monotonically increasing
//! revision); the realtime callback compares revisions while it already holds
//! the controller lock and pushes the update into its local processor.
//!
//! Render order (see `docs/references/contracts/playback.md`):
//!
//! ```text
//! existing source/stem mix + master/stem gains
//! → EQ dry/wet processor + auto preamp
//! → soft limiter
//! → existing play/pause/seek fade
//! → output/AirPlay forwarding
//! ```
//!
//! No callback operation in this module allocates after steady-state
//! initialization, blocks on a second mutex, logs per sample/callback, or
//! serializes/emits an event.

use biquad::{Biquad, Coefficients, DirectForm1, Hertz, Type};

/// Band center frequencies for the five-band peaking EQ.
pub const EQ_BAND_FREQUENCIES_HZ: [f32; 5] = [60.0, 230.0, 910.0, 3_600.0, 14_000.0];
/// Q factor shared by all bands (Butterworth-ish 0.707).
pub const EQ_Q: f32 = 0.707;
/// Inclusive lower bound for per-band gain in dB.
pub const EQ_MIN_GAIN_DB: f32 = -12.0;
/// Inclusive upper bound for per-band gain in dB.
pub const EQ_MAX_GAIN_DB: f32 = 12.0;
/// Smooth duration for gain and preamp transitions.
const EQ_SMOOTH_MS: f32 = 50.0;
/// Smooth duration for the bypass dry/wet crossfade.
const BYPASS_SMOOTH_MS: f32 = 20.0;
/// Bands at or above `sample_rate * NYQUIST_RATIO_LIMIT` are skipped to avoid
/// coefficient instability near Nyquist.
const NYQUIST_RATIO_LIMIT: f32 = 0.45;
/// Soft-limiter threshold; samples at or below this pass through unchanged.
pub const LIMITER_THRESHOLD: f32 = 0.95;

/// Convert decibels to linear amplitude.
fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// Continuous, slope-matched soft limiter.
///
/// Samples at or below `LIMITER_THRESHOLD` are returned bit-for-bit unchanged.
/// Above the threshold the magnitude is compressed by a `tanh` curve that is
/// continuous with derivative 1 at the threshold and asymptotically stays
/// below `1.0`. Non-finite samples are sanitized to `0.0`.
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
    // Preserve the sign of the input: `compressed.copysign(sample)` keeps the
    // magnitude of `compressed` and the sign of `sample`.
    compressed.copysign(sample)
}

/// Per-channel, per-band peaking EQ filter state.
type ChannelBands = [Option<DirectForm1<f32>>; 5];

/// Realtime EQ processor owned by the CPAL output closure.
pub struct EqProcessor {
    filters: Vec<ChannelBands>,
    sample_rate: f32,
    channels: usize,
    enabled: bool,
    target_gains_db: [f32; 5],
    current_gains_db: [f32; 5],
    target_preamp: f32,
    current_preamp: f32,
    wet_mix: f32,
    target_wet_mix: f32,
    /// Last controller `eq_revision` applied to this processor. The callback
    /// compares this with the controller's current revision to detect config
    /// changes without polling on every callback.
    last_eq_revision: u64,
}

impl EqProcessor {
    /// Construct a processor for the given device sample rate and channel
    /// count. Bands above the Nyquist guard are left as `None` for the
    /// lifetime of this processor (i.e. until a new stream is built).
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        let sr = sample_rate as f32;
        let nyquist_limit = sr * NYQUIST_RATIO_LIMIT;
        let filters = (0..channels)
            .map(|_| {
                let mut bands: ChannelBands = [None, None, None, None, None];
                for (i, &freq) in EQ_BAND_FREQUENCIES_HZ.iter().enumerate() {
                    if freq >= nyquist_limit {
                        continue;
                    }
                    // Initial coefficients use 0 dB gain (flat). The callback
                    // updates coefficients from the smoothed gain each tick.
                    if let Ok(coeffs) = Coefficients::<f32>::from_params(
                        Type::PeakingEQ(0.0),
                        Hertz::from_hz(sr).unwrap_or_else(|_| Hertz::from_hz(1.0).unwrap()),
                        Hertz::from_hz(freq).unwrap_or_else(|_| Hertz::from_hz(1.0).unwrap()),
                        EQ_Q,
                    ) {
                        bands[i] = Some(DirectForm1::new(coeffs));
                    }
                }
                bands
            })
            .collect();

        EqProcessor {
            filters,
            sample_rate: sr,
            channels,
            enabled: false,
            target_gains_db: [0.0; 5],
            current_gains_db: [0.0; 5],
            target_preamp: 1.0,
            current_preamp: 1.0,
            wet_mix: 0.0,
            target_wet_mix: 0.0,
            last_eq_revision: 0,
        }
    }

    /// Update the enabled/bypass target. Filters keep running while bypassed
    /// so re-enable uses warm delay state; the dry/wet crossfade masks the
    /// transition.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.target_wet_mix = if enabled { 1.0 } else { 0.0 };
        self.update_preamp_target();
    }

    /// Update the per-band gain target (dB). Coefficients are recomputed on
    /// the next callback from the smoothed gain.
    pub fn set_gains(&mut self, gains_db: [f32; 5]) {
        self.target_gains_db = gains_db;
        self.update_preamp_target();
    }

    /// Auto preamp target supplies headroom for positive gain: it is
    /// `db_to_linear(-max(0, max_positive_gain))`. Smoothed over the same
    /// 50 ms interval as the gains.
    fn update_preamp_target(&mut self) {
        let max_positive =
            self.target_gains_db
                .iter()
                .copied()
                .fold(0.0f32, |acc, g| if g > acc { g } else { acc });
        self.target_preamp = db_to_linear(-max_positive.max(0.0));
    }

    /// Last controller revision applied to this processor.
    pub fn last_eq_revision(&self) -> u64 {
        self.last_eq_revision
    }

    /// Record that the processor has consumed the given revision.
    pub fn set_last_eq_revision(&mut self, revision: u64) {
        self.last_eq_revision = revision;
    }

    /// Process `output[..rendered_samples]` in place. Trailing callback
    /// padding must not be passed here — filter state must not advance on
    /// unrendered samples. Zero-length callbacks (buffering) return without
    /// advancing any smoothing.
    pub fn process(&mut self, output: &mut [f32], rendered_samples: usize) {
        if rendered_samples == 0 {
            return;
        }
        let channels = self.channels.max(1);
        let frames = rendered_samples / channels;
        if frames == 0 {
            return;
        }

        // Recompute coefficients at most once per callback per band using the
        // current smoothed gain. `update_coefficients` preserves delay state.
        // A coefficient error disables only that band for this callback; the
        // last valid coefficients are retained (no update, no panic).
        for ch in 0..self.channels {
            let Some(bands) = self.filters.get_mut(ch) else {
                continue;
            };
            for band in 0..5 {
                let Some(filter) = bands[band].as_mut() else {
                    continue;
                };
                let gain = self.current_gains_db[band];
                let Ok(coeffs) = Coefficients::<f32>::from_params(
                    Type::PeakingEQ(gain),
                    Hertz::from_hz(self.sample_rate)
                        .unwrap_or_else(|_| Hertz::from_hz(1.0).unwrap()),
                    Hertz::from_hz(EQ_BAND_FREQUENCIES_HZ[band])
                        .unwrap_or_else(|_| Hertz::from_hz(1.0).unwrap()),
                    EQ_Q,
                ) else {
                    continue;
                };
                filter.update_coefficients(coeffs);
            }
        }

        let smooth_samples = (EQ_SMOOTH_MS * self.sample_rate / 1000.0).max(1.0);
        let bypass_samples = (BYPASS_SMOOTH_MS * self.sample_rate / 1000.0).max(1.0);

        // Linear smoothing: compute a fixed per-frame step from the current
        // value to the target at the start of this callback. The step is
        // capped so the final frame lands exactly on the target without
        // overshoot. This avoids the exponential-decay behavior of
        // `diff * (1/N)` smoothing, which never actually reaches the target.
        let gain_steps: [f32; 5] = {
            let mut steps = [0.0f32; 5];
            for (band, step) in steps.iter_mut().enumerate() {
                let diff = self.target_gains_db[band] - self.current_gains_db[band];
                *step = diff / smooth_samples;
            }
            steps
        };
        let preamp_step = (self.target_preamp - self.current_preamp) / smooth_samples;
        let wet_step = (self.target_wet_mix - self.wet_mix) / bypass_samples;

        for frame in 0..frames {
            // Advance smoothing by one rendered frame. Each scalar moves
            // linearly toward its target; clamp to the target so the final
            // step does not overshoot.
            for (band, &step) in gain_steps.iter().enumerate() {
                let diff = self.target_gains_db[band] - self.current_gains_db[band];
                if step.abs() >= diff.abs() {
                    self.current_gains_db[band] = self.target_gains_db[band];
                } else {
                    self.current_gains_db[band] += step;
                }
            }
            let preamp_diff = self.target_preamp - self.current_preamp;
            if preamp_step.abs() >= preamp_diff.abs() {
                self.current_preamp = self.target_preamp;
            } else {
                self.current_preamp += preamp_step;
            }
            let wet_diff = self.target_wet_mix - self.wet_mix;
            if wet_step.abs() >= wet_diff.abs() {
                self.wet_mix = self.target_wet_mix;
            } else {
                self.wet_mix += wet_step;
            }

            // Snapshot the per-frame scalar gains so the inner channel loop
            // can borrow `self.filters` mutably without aliasing.
            let preamp = self.current_preamp;
            let wet_mix = self.wet_mix;
            let dry_gain = 1.0 - wet_mix;

            for ch in 0..self.channels {
                let idx = frame * channels + ch;
                if idx >= output.len() {
                    break;
                }
                let dry = output[idx];
                let mut wet = dry;
                if let Some(bands) = self.filters.get_mut(ch) {
                    for filter in bands.iter_mut().take(5).flatten() {
                        wet = filter.run(wet);
                    }
                }
                wet *= preamp;
                output[idx] = dry * dry_gain + wet * wet_mix;
            }
        }
    }
}

/// Validated EQ config snapshot published by `PlaybackController`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqConfig {
    pub enabled: bool,
    pub gains_db: [f32; 5],
    /// Monotonically increasing revision; bumped on every successful setter.
    pub revision: u64,
}

impl EqConfig {
    pub fn flat() -> Self {
        EqConfig {
            enabled: false,
            gains_db: [0.0; 5],
            revision: 0,
        }
    }
}

/// Validate a per-band gain array. Returns `Ok(())` only when every value is
/// finite and within `EQ_MIN_GAIN_DB..=EQ_MAX_GAIN_DB`. The caller is expected
/// to reject the whole request on `Err` rather than clamping.
pub fn validate_gains_db(gains_db: &[f32; 5]) -> Result<(), &'static str> {
    for &g in gains_db {
        if !g.is_finite() {
            return Err("eq gain must be finite");
        }
        if !(EQ_MIN_GAIN_DB..=EQ_MAX_GAIN_DB).contains(&g) {
            return Err("eq gain out of range");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── soft_limit ────────────────────────────────────────────────────────

    #[test]
    fn flat_bypassed_path_is_transparent_below_threshold() {
        let mut proc = EqProcessor::new(44_100, 2);
        // Flat gains, disabled: process should be transparent for samples
        // below the limiter threshold.
        let samples = [0.3, -0.5, 0.7, -0.2, 0.0, 0.94, -0.94, 0.1];
        let mut buf = samples.to_vec();
        let len = buf.len();
        proc.process(&mut buf, len);
        for (i, &orig) in samples.iter().enumerate() {
            assert!(
                (buf[i] - orig).abs() <= 1e-6,
                "flat/disabled changed sample {i}: {orig} -> {}",
                buf[i]
            );
        }
    }

    #[test]
    fn soft_limit_is_continuous_around_threshold() {
        let eps = 1e-4;
        let below = soft_limit(LIMITER_THRESHOLD - eps);
        let at = soft_limit(LIMITER_THRESHOLD);
        let above = soft_limit(LIMITER_THRESHOLD + eps);
        assert!((below - (LIMITER_THRESHOLD - eps)).abs() < 1e-7);
        assert!((at - LIMITER_THRESHOLD).abs() < 1e-7);
        // Just above threshold should be very close to threshold (tanh ~0).
        assert!((above - LIMITER_THRESHOLD).abs() <= 2e-4);
        // Continuity: derivative ~1 at threshold. The tanh curve has slope 1
        // at the threshold, so the finite-difference slope over a small window
        // should be close to 1.
        let slope = (above - below) / (2.0 * eps);
        assert!(
            (slope - 1.0).abs() < 0.05,
            "limiter slope at threshold should be ~1, got {slope}"
        );
    }

    #[test]
    fn soft_limit_output_is_bounded() {
        for &x in &[2.0_f32, 10.0, 100.0, 1_000_000.0, -2.0, -10.0, -100.0] {
            let y = soft_limit(x);
            assert!(y.abs() <= 1.0, "soft_limit({x}) = {y} must be <= 1.0");
            assert!(y.is_finite(), "soft_limit({x}) must be finite");
        }
    }

    #[test]
    fn soft_limit_sanitizes_non_finite() {
        assert_eq!(soft_limit(f32::INFINITY), 0.0);
        assert_eq!(soft_limit(f32::NEG_INFINITY), 0.0);
        assert_eq!(soft_limit(f32::NAN), 0.0);
    }

    #[test]
    fn soft_limit_is_odd_symmetric() {
        for &x in &[0.96, 1.5, 5.0, 100.0] {
            let pos = soft_limit(x);
            let neg = soft_limit(-x);
            assert!(
                (pos + neg).abs() < 1e-5,
                "soft_limit should be odd-symmetric: f({x})={pos}, f({neg_x})={neg}",
                neg_x = -x
            );
        }
    }

    // ── EQ band response ──────────────────────────────────────────────────

    #[test]
    fn plus_12db_band_boosts_in_band_tone_more_than_off_band_tone() {
        // 48 kHz stereo, 910 Hz band boosted by +12 dB.
        let mut proc = EqProcessor::new(48_000, 2);
        proc.set_enabled(true);
        proc.set_gains([0.0, 0.0, 12.0, 0.0, 0.0]);
        // Run a few callbacks to let smoothing settle.
        let frames = 48_000; // 1 second
        let in_band_freq = 910.0_f32;
        let off_band_freq = 60.0_f32;

        let in_band_rms = run_tone_rms(&mut proc, in_band_freq, 48_000, frames);
        let off_band_rms = run_tone_rms(&mut proc, off_band_freq, 48_000, frames);
        assert!(
            in_band_rms > off_band_rms * 1.5,
            "in-band tone should be boosted more than off-band: in={in_band_rms}, off={off_band_rms}"
        );
    }

    fn run_tone_rms(proc: &mut EqProcessor, freq: f32, sample_rate: u32, frames: usize) -> f32 {
        let channels = 2;
        let mut buf = vec![0.0f32; frames * channels];
        let phase_step = 2.0 * std::f32::consts::PI * freq / sample_rate as f32;
        let mut phase: f32 = 0.0;
        // Generate a sine tone into the buffer.
        for frame in 0..frames {
            let s = phase.sin() * 0.5;
            for ch in 0..channels {
                buf[frame * channels + ch] = s;
            }
            phase += phase_step;
        }
        // Process in callback-sized chunks so smoothing advances realistically.
        let chunk = 256;
        let mut start = 0;
        while start < buf.len() {
            let end = (start + chunk * channels).min(buf.len());
            let rendered = end - start;
            proc.process(&mut buf[start..end], rendered);
            start = end;
        }
        // RMS of the second half (after settling).
        let half = buf.len() / 2;
        let sum: f32 = buf[half..].iter().map(|&s| s * s).sum();
        (sum / (buf.len() - half) as f32).sqrt()
    }

    // ── Auto preamp ───────────────────────────────────────────────────────

    #[test]
    fn auto_preamp_target_reduces_wet_gain_for_positive_boost() {
        let mut proc = EqProcessor::new(44_100, 2);
        proc.set_enabled(true);
        proc.set_gains([0.0, 0.0, 0.0, 0.0, 12.0]);
        // target_preamp = db_to_linear(-12) ≈ 0.251
        let expected = db_to_linear(-12.0);
        assert!(
            (proc.target_preamp - expected).abs() < 1e-5,
            "preamp target should be {expected}, got {}",
            proc.target_preamp
        );
    }

    #[test]
    fn auto_preamp_target_is_unity_when_no_positive_gain() {
        let mut proc = EqProcessor::new(44_100, 2);
        proc.set_enabled(true);
        proc.set_gains([-6.0, -3.0, 0.0, -12.0, -1.0]);
        assert!(
            (proc.target_preamp - 1.0).abs() < 1e-6,
            "preamp should be unity when no positive gain, got {}",
            proc.target_preamp
        );
    }

    #[test]
    fn preamp_smooths_over_50ms() {
        let mut proc = EqProcessor::new(48_000, 2);
        proc.set_enabled(true);
        proc.set_gains([12.0, 0.0, 0.0, 0.0, 0.0]);
        // 50 ms = 2400 frames at 48 kHz. After 2400 frames the preamp should
        // have reached the target.
        let frames = 2400;
        let mut buf = vec![0.5f32; frames * 2];
        let len = buf.len();
        proc.process(&mut buf, len);
        assert!(
            (proc.current_preamp - proc.target_preamp).abs() < 1e-3,
            "preamp should reach target after 50ms, current={}, target={}",
            proc.current_preamp,
            proc.target_preamp
        );
    }

    // ── Bypass / gain change discontinuity ────────────────────────────────

    #[test]
    fn bypass_transition_is_bounded() {
        let mut proc = EqProcessor::new(44_100, 2);
        proc.set_enabled(true);
        // Settle wet_mix to 1.0.
        let mut buf = vec![0.5f32; 44_100 * 2];
        let len = buf.len();
        proc.process(&mut buf, len);
        assert!((proc.wet_mix - 1.0).abs() < 1e-6);

        // Disable and check adjacent-sample discontinuity is bounded.
        proc.set_enabled(false);
        let chunk = 256;
        let mut prev = 0.5f32;
        let mut max_jump = 0.0f32;
        for _ in 0..10 {
            let mut b = vec![prev; chunk * 2];
            let len = b.len();
            proc.process(&mut b, len);
            for &s in b.iter() {
                let jump = (s - prev).abs();
                if jump > max_jump {
                    max_jump = jump;
                }
                prev = s;
            }
        }
        // 20 ms bypass ramp at 44.1 kHz = 882 samples. Per-sample jump for a
        // 0.5 amplitude signal over 882 samples ≈ 0.5/882 ≈ 5.7e-4. Allow a
        // generous bound that still rejects a hard click.
        assert!(
            max_jump < 1e-2,
            "bypass transition max adjacent-sample jump {max_jump} too large"
        );
    }

    // ── Independent channel state ─────────────────────────────────────────

    #[test]
    fn left_and_right_filter_state_is_independent() {
        let mut proc = EqProcessor::new(48_000, 2);
        proc.set_enabled(true);
        proc.set_gains([0.0, 0.0, 12.0, 0.0, 0.0]);
        // Feed a 910 Hz tone on the left only; right stays silent.
        let frames = 4096;
        let mut buf = vec![0.0f32; frames * 2];
        let phase_step = 2.0 * std::f32::consts::PI * 910.0 / 48_000.0;
        let mut phase: f32 = 0.0;
        for frame in 0..frames {
            buf[frame * 2] = phase.sin() * 0.5;
            phase += phase_step;
        }
        let len = buf.len();
        proc.process(&mut buf, len);
        // Right channel should remain near zero (independent state, no
        // crossfeed). A tiny non-zero value comes from the dry/wet mix of a
        // zero input, which is still zero.
        let right_max =
            buf.iter().skip(1).step_by(2).fold(
                0.0f32,
                |acc, &v| {
                    if v.abs() > acc {
                        v.abs()
                    } else {
                        acc
                    }
                },
            );
        assert!(
            right_max < 1e-6,
            "right channel should stay silent, max={right_max}"
        );
    }

    // ── Nyquist guards ────────────────────────────────────────────────────

    #[test]
    fn nyquist_guard_skips_bands_above_45_percent() {
        // 22.05 kHz: Nyquist = 11025, 45% = 4961. 14 kHz > 4961 → skipped.
        let proc = EqProcessor::new(22_050, 2);
        for ch in 0..2 {
            assert!(
                proc.filters[ch][4].is_none(),
                "14 kHz band should be skipped at 22.05 kHz"
            );
            // 3.6 kHz < 4961 → present.
            assert!(
                proc.filters[ch][3].is_some(),
                "3.6 kHz band should be present at 22.05 kHz"
            );
        }
    }

    #[test]
    fn all_bands_present_at_48khz() {
        let proc = EqProcessor::new(48_000, 2);
        for ch in 0..2 {
            for band in 0..5 {
                assert!(
                    proc.filters[ch][band].is_some(),
                    "band {band} should be present at 48 kHz"
                );
            }
        }
    }

    #[test]
    fn all_bands_present_at_96khz() {
        let proc = EqProcessor::new(96_000, 2);
        for ch in 0..2 {
            for band in 0..5 {
                assert!(proc.filters[ch][band].is_some());
            }
        }
    }

    #[test]
    fn bands_present_at_32_and_44_1khz() {
        for sr in [32_000u32, 44_100] {
            let proc = EqProcessor::new(sr, 2);
            let nyq = sr as f32 * NYQUIST_RATIO_LIMIT;
            for band in 0..5 {
                let present = EQ_BAND_FREQUENCIES_HZ[band] < nyq;
                for ch in 0..2 {
                    assert_eq!(
                        proc.filters[ch][band].is_some(),
                        present,
                        "sr={sr} band={band} expected present={present}"
                    );
                }
            }
        }
    }

    // ── Validation ────────────────────────────────────────────────────────

    #[test]
    fn validate_gains_rejects_out_of_range() {
        assert!(validate_gains_db(&[0.0, 0.0, 0.0, 0.0, 12.5]).is_err());
        assert!(validate_gains_db(&[-12.5, 0.0, 0.0, 0.0, 0.0]).is_err());
    }

    #[test]
    fn validate_gains_rejects_non_finite() {
        assert!(validate_gains_db(&[f32::NAN, 0.0, 0.0, 0.0, 0.0]).is_err());
        assert!(validate_gains_db(&[0.0, 0.0, f32::INFINITY, 0.0, 0.0]).is_err());
    }

    #[test]
    fn validate_gains_accepts_bounds() {
        assert!(validate_gains_db(&[-12.0, 12.0, 0.0, -12.0, 12.0]).is_ok());
        assert!(validate_gains_db(&[0.0; 5]).is_ok());
    }

    // ── EqConfig ──────────────────────────────────────────────────────────

    #[test]
    fn eq_config_flat_defaults_to_disabled_zero_gains() {
        let c = EqConfig::flat();
        assert!(!c.enabled);
        assert_eq!(c.gains_db, [0.0; 5]);
        assert_eq!(c.revision, 0);
    }
}
