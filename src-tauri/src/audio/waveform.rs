//! Waveform peak computation for the seekbar visualizer.
//!
//! `compute_waveform_peaks` reduces a fully decoded track to `buckets` peak
//! values in `0.0..=1.0`. The output is deterministic, allocation-free beyond
//! the output buffer, and never touches playback state — it runs on a
//! background blocking task owned by the singleflight layer.

use crate::audio::decode::DecodedAudio;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WaveformError {
    #[error("audio has zero channels")]
    ZeroChannels,
    #[error("audio has zero sample rate")]
    ZeroSampleRate,
    #[error("samples length {0} is not a multiple of channels {1}")]
    UnalignedSamples(usize, usize),
    #[error("bucket count {0} is outside the valid range 24..=1000")]
    InvalidBuckets(usize),
}

/// Compute `buckets` peak values from interleaved PCM.
///
/// For output bucket `b` the integer frame boundaries are:
/// ```text
/// start_frame = floor(b * total_frames / buckets)
/// end_frame   = floor((b + 1) * total_frames / buckets)
/// ```
/// For non-empty ranges, the peak is the maximum absolute finite sample
/// across every channel and frame, clamped to `0.0..=1.0`. Non-finite
/// source samples are treated as zero. If a short file yields an empty
/// bucket, the nearest existing frame (selected by `min(start_frame,
/// total_frames - 1)`) is sampled so the output length remains exact. Empty
/// audio returns `buckets` zeros.
///
/// Values are NOT normalized by the song-wide maximum — stored peaks represent
/// actual post-decode PCM amplitude so quiet and loud regions retain their
/// relationship.
pub fn compute_waveform_peaks(
    audio: &DecodedAudio,
    buckets: usize,
) -> Result<Arc<[f32]>, WaveformError> {
    if audio.channels == 0 {
        return Err(WaveformError::ZeroChannels);
    }
    if audio.sample_rate_hz == 0 {
        return Err(WaveformError::ZeroSampleRate);
    }
    if !(24..=1000).contains(&buckets) {
        return Err(WaveformError::InvalidBuckets(buckets));
    }
    if !audio.samples.len().is_multiple_of(audio.channels) {
        return Err(WaveformError::UnalignedSamples(
            audio.samples.len(),
            audio.channels,
        ));
    }

    let channels = audio.channels;
    let total_frames = audio.samples.len() / channels;
    let mut peaks = Vec::with_capacity(buckets);

    if total_frames == 0 {
        peaks.resize(buckets, 0.0);
        return Ok(peaks.into());
    }

    for b in 0..buckets {
        let start_frame = (b * total_frames) / buckets;
        let end_frame = ((b + 1) * total_frames) / buckets;
        let peak = if end_frame > start_frame {
            max_abs_peak(&audio.samples, start_frame, end_frame, channels)
        } else {
            // Empty bucket — sample the nearest existing frame.
            let nearest = start_frame.min(total_frames - 1);
            max_abs_peak(&audio.samples, nearest, nearest + 1, channels)
        };
        // Non-finite already sanitized to 0 by max_abs_peak; clamp the final
        // value to the validated output range.
        peaks.push(peak.clamp(0.0, 1.0));
    }

    Ok(peaks.into())
}

fn max_abs_peak(samples: &[f32], start_frame: usize, end_frame: usize, channels: usize) -> f32 {
    let mut max = 0.0f32;
    for frame in start_frame..end_frame {
        let base = frame * channels;
        for ch in 0..channels {
            let sample = samples[base + ch];
            let abs = if sample.is_finite() {
                sample.abs()
            } else {
                0.0
            };
            if abs > max {
                max = abs;
            }
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(samples: Vec<f32>, channels: usize, sample_rate: u32) -> DecodedAudio {
        DecodedAudio {
            sample_rate_hz: sample_rate,
            channels,
            duration_ms: ((samples.len() / channels.max(1)) as f64 * 1000.0 / sample_rate as f64)
                as u64,
            samples,
        }
    }

    #[test]
    fn exact_output_length_for_typical_input() {
        let samples: Vec<f32> = (0..2000).map(|i| (i as f32) / 2000.0).collect();
        let decoded = audio(samples, 2, 44_100);
        let peaks = compute_waveform_peaks(&decoded, 200).expect("ok");
        assert_eq!(peaks.len(), 200);
        for p in peaks.iter() {
            assert!(p.is_finite());
            assert!((*p).is_finite() && (0.0..=1.0).contains(p));
        }
    }

    #[test]
    fn bucket_boundaries_match_integer_floor_formula() {
        let samples: Vec<f32> = (1..=48).map(|i| i as f32 / 48.0).collect();
        let decoded = audio(samples, 1, 44_100);
        let peaks = compute_waveform_peaks(&decoded, 24).expect("ok");
        assert_eq!(peaks.len(), 24);
        assert!((peaks[0] - 2.0 / 48.0).abs() < 1e-6);
        assert!((peaks[1] - 4.0 / 48.0).abs() < 1e-6);
        assert!((peaks[23] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn short_audio_fills_empty_buckets_from_nearest_frame() {
        let decoded = audio(vec![0.42], 1, 44_100);
        let peaks = compute_waveform_peaks(&decoded, 200).expect("ok");
        assert_eq!(peaks.len(), 200);
        for p in peaks.iter() {
            assert!((p - 0.42).abs() < 1e-6);
        }
    }

    #[test]
    fn empty_audio_returns_buckets_zeros() {
        let decoded = audio(vec![], 2, 44_100);
        let peaks = compute_waveform_peaks(&decoded, 100).expect("ok");
        assert_eq!(peaks.len(), 100);
        assert!(peaks.iter().all(|p| *p == 0.0));
    }

    #[test]
    fn non_finite_source_samples_treated_as_zero() {
        let samples = vec![f32::NAN, f32::INFINITY, -f32::INFINITY, 0.5];
        let decoded = audio(samples, 2, 44_100);
        let peaks = compute_waveform_peaks(&decoded, 24).expect("ok");
        assert_eq!(peaks.len(), 24);
        for b in 0..12 {
            assert!((peaks[b] - 0.0).abs() < 1e-6, "bucket {b} should be 0");
        }
        for b in 12..24 {
            assert!((peaks[b] - 0.5).abs() < 1e-6, "bucket {b} should be 0.5");
        }
    }

    #[test]
    fn final_peak_clamped_to_unit_range() {
        let samples = vec![2.5, -3.0, 1.0, 0.0];
        let decoded = audio(samples, 2, 44_100);
        let peaks = compute_waveform_peaks(&decoded, 24).expect("ok");
        assert_eq!(peaks.len(), 24);
        for p in peaks.iter() {
            assert!(*p <= 1.0);
            assert!(*p >= 0.0);
        }
    }

    #[test]
    fn quiet_and_loud_regions_retain_relationship() {
        let mut samples = vec![0.1f32; 1000];
        samples.extend(vec![0.9; 1000]);
        let decoded = audio(samples, 1, 44_100);
        let peaks = compute_waveform_peaks(&decoded, 200).expect("ok");
        assert_eq!(peaks.len(), 200);
        let quiet_avg: f32 = peaks[..100].iter().sum::<f32>() / 100.0;
        let loud_avg: f32 = peaks[100..].iter().sum::<f32>() / 100.0;
        assert!(
            loud_avg > quiet_avg * 5.0,
            "loud region should be much larger than quiet: {quiet_avg} vs {loud_avg}"
        );
    }

    #[test]
    fn zero_channels_errors() {
        let decoded = audio(vec![0.0; 100], 0, 44_100);
        let err = compute_waveform_peaks(&decoded, 100).unwrap_err();
        assert!(matches!(err, WaveformError::ZeroChannels));
    }

    #[test]
    fn zero_sample_rate_errors() {
        let decoded = audio(vec![0.0; 100], 2, 0);
        let err = compute_waveform_peaks(&decoded, 100).unwrap_err();
        assert!(matches!(err, WaveformError::ZeroSampleRate));
    }

    #[test]
    fn unaligned_samples_errors() {
        let decoded = audio(vec![0.0; 5], 2, 44_100);
        let err = compute_waveform_peaks(&decoded, 100).unwrap_err();
        assert!(matches!(err, WaveformError::UnalignedSamples(5, 2)));
    }

    #[test]
    fn invalid_buckets_errors() {
        let decoded = audio(vec![0.0; 100], 2, 44_100);
        assert!(matches!(
            compute_waveform_peaks(&decoded, 23).unwrap_err(),
            WaveformError::InvalidBuckets(23)
        ));
        assert!(matches!(
            compute_waveform_peaks(&decoded, 1001).unwrap_err(),
            WaveformError::InvalidBuckets(1001)
        ));
    }

    #[test]
    fn stereo_uses_max_across_channels() {
        let samples = vec![0.2, 0.8, 0.4, 0.1];
        let decoded = audio(samples, 2, 44_100);
        let peaks = compute_waveform_peaks(&decoded, 24).expect("ok");
        assert_eq!(peaks.len(), 24);
        for b in 0..12 {
            assert!((peaks[b] - 0.8).abs() < 1e-6, "bucket {b}");
        }
        for b in 12..24 {
            assert!((peaks[b] - 0.4).abs() < 1e-6, "bucket {b}");
        }
    }
}
