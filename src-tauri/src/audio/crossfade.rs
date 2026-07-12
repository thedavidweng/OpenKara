//! #89: Equal-power crossfade DSP helpers.
//!
//! Provides the per-frame gain computation and effective-duration calculation
//! for overlapping the tail of an outgoing decoded track with the start of
//! a prepared incoming track. The actual mixing happens in the CPAL output
//! callback (`audio::output`); this module keeps the sample math testable
//! without a realtime thread.

use std::f32::consts::FRAC_PI_2;

/// Minimum effective overlap in milliseconds. If the calculated overlap
/// is shorter than this, the transition falls back to #88's gapless switch.
pub const CROSSFADE_MIN_MS: u32 = 500;

/// Maximum chunk size (in frames) processed in one iteration of the
/// callback. The incoming scratch buffer is preallocated to this size.
pub const CROSSFADE_SCRATCH_FRAMES: usize = 4096;

/// Compute the equal-power gains for a given overlap frame index.
///
/// For `N > 1` and zero-based overlap index `i`:
/// ```text
/// t = i / (N - 1)
/// outgoing_gain = cos(t * pi/2)
/// incoming_gain = sin(t * pi/2)
/// ```
///
/// The first frame (`i = 0`) uses outgoing gain 1, incoming gain 0.
/// The last frame (`i = N - 1`) uses outgoing gain 0, incoming gain 1.
///
/// `N == 1` is unreachable in production due to the 500 ms floor, but
/// handled as an immediate gapless switch (outgoing 0, incoming 1).
pub fn equal_power_gains(overlap_index: u64, total_overlap_frames: u64) -> (f32, f32) {
    if total_overlap_frames <= 1 {
        return (0.0, 1.0);
    }
    let t = overlap_index as f32 / (total_overlap_frames - 1) as f32;
    let angle = t * FRAC_PI_2;
    (angle.cos(), angle.sin())
}

/// Calculate the effective crossfade duration in frames.
///
/// ```text
/// configured_frames = round(duration_ms * output_sample_rate / 1000)
/// effective_frames = min(
///   configured_frames,
///   floor(outgoing_total_frames / 2),
///   floor(incoming_total_frames / 2),
///   outgoing_frames_remaining
/// )
/// ```
///
/// Returns `None` if the effective duration is less than `CROSSFADE_MIN_MS`
/// worth of frames — the caller should fall back to #88's gapless transition.
pub fn effective_overlap_frames(
    configured_duration_ms: u32,
    output_sample_rate: u32,
    outgoing_total_frames: u64,
    incoming_total_frames: u64,
    outgoing_frames_remaining: u64,
) -> Option<u64> {
    if output_sample_rate == 0 {
        return None;
    }
    let configured_frames =
        (configured_duration_ms as u64 * output_sample_rate as u64).div_ceil(1000);

    let half_outgoing = outgoing_total_frames / 2;
    let half_incoming = incoming_total_frames / 2;

    let effective = configured_frames
        .min(half_outgoing)
        .min(half_incoming)
        .min(outgoing_frames_remaining);

    // Sub-500ms floor: fall back to gapless.
    let min_frames = (CROSSFADE_MIN_MS as u64 * output_sample_rate as u64).div_ceil(1000);
    if effective < min_frames {
        return None;
    }

    Some(effective)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_frame_is_full_outgoing() {
        let (out, inc) = equal_power_gains(0, 100);
        assert!((out - 1.0).abs() < 1e-5);
        assert!(inc.abs() < 1e-5);
    }

    #[test]
    fn last_frame_is_full_incoming() {
        let (out, inc) = equal_power_gains(99, 100);
        assert!(out.abs() < 1e-5);
        assert!((inc - 1.0).abs() < 1e-5);
    }

    #[test]
    fn middle_frame_is_equal_power() {
        let (out, inc) = equal_power_gains(50, 101);
        // At t = 0.5, cos(pi/4) = sin(pi/4) = sqrt(2)/2 ≈ 0.7071
        let expected = (2.0_f32).sqrt() / 2.0;
        assert!((out - expected).abs() < 1e-4);
        assert!((inc - expected).abs() < 1e-4);
    }

    #[test]
    fn equal_power_identity_holds() {
        // cos^2 + sin^2 = 1 for all frames
        for i in 0..200 {
            let (out, inc) = equal_power_gains(i, 200);
            let sum_sq = out * out + inc * inc;
            assert!((sum_sq - 1.0).abs() < 1e-4, "frame {i}: sum_sq = {sum_sq}");
        }
    }

    #[test]
    fn single_frame_returns_gapless_switch() {
        let (out, inc) = equal_power_gains(0, 1);
        assert_eq!(out, 0.0);
        assert_eq!(inc, 1.0);
    }

    #[test]
    fn effective_duration_clamps_to_half_track_limits() {
        // 10s configured, but outgoing is only 4s (176400 frames at 44.1kHz)
        // half = 88200, which is less than 10s of frames (441000)
        let effective = effective_overlap_frames(10_000, 44_100, 176_400, 441_000, 176_400);
        assert_eq!(effective, Some(88_200));
    }

    #[test]
    fn effective_duration_clamps_to_remaining_frames() {
        // Only 1s remaining, configured 10s
        let effective = effective_overlap_frames(10_000, 44_100, 441_000, 441_000, 44_100);
        assert_eq!(effective, Some(44_100));
    }

    #[test]
    fn sub_500ms_falls_back_to_gapless() {
        // 100ms remaining — below 500ms floor
        let frames_100ms = 44_100 * 100 / 1000; // 4410
        let effective =
            effective_overlap_frames(10_000, 44_100, 441_000, 441_000, frames_100ms as u64);
        assert_eq!(effective, None);
    }

    #[test]
    fn effective_duration_uses_configured_when_tracks_are_long() {
        // 3s configured, both tracks 60s, 30s remaining
        let effective = effective_overlap_frames(3_000, 44_100, 2_646_000, 2_646_000, 1_323_000);
        // 3s = 132300 frames
        assert_eq!(effective, Some(132_300));
    }

    #[test]
    fn odd_duration_rounds_correctly() {
        // 750ms at 44100 Hz = 33075 frames
        let effective = effective_overlap_frames(750, 44_100, 2_646_000, 2_646_000, 1_323_000);
        // round(750 * 44100 / 1000) = round(33075) = 33075
        assert_eq!(effective, Some(33_075));
    }

    #[test]
    fn zero_sample_rate_returns_none() {
        let effective = effective_overlap_frames(3_000, 0, 441_000, 441_000, 441_000);
        assert_eq!(effective, None);
    }
}
