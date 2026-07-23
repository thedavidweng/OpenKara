//! Bounded overlap-add ring buffer for streaming separation output.
//!
//! Each ring holds one stem's worth of OLA state for the full song, but
//! only retains a fixed-size window of active frames. Frames that can no
//! longer be modified by a future chunk are finalized and flushed to the
//! output writer, keeping memory independent of song duration.

use anyhow::Result;

/// A single-stem OLA ring that accumulates windowed output across
/// overlapping inference chunks and flushes finalized frames through a
/// callback.
///
/// The ring covers `chunk_size` frames (one inference window). With 50%
/// overlap, after processing chunk `N`, frames in chunk `N-1` that are
/// not covered by chunk `N+1` are finalized. Since hop = chunk_size / 2,
/// frames `[0, hop)` of chunk `N-1` are safe to flush after chunk `N`
/// completes.
///
/// Memory is bounded: the ring holds at most `chunk_size * channels`
/// samples plus a normalization accumulator of the same size.
pub struct OlaRing {
    channels: usize,
    chunk_size: usize,
    /// Hop size between chunks. Retained for documentation and future use;
    /// the flush logic uses `safe_through_frame` which is derived from the
    /// hop size by the caller.
    #[allow(dead_code)]
    hop_size: usize,
    /// Accumulated windowed samples for the active region, interleaved.
    /// Size: `chunk_size * channels`.
    accum: Vec<f32>,
    /// Accumulated squared-window normalization, one per frame.
    /// Size: `chunk_size`.
    norm: Vec<f32>,
    /// Absolute frame index of the first frame in `accum`.
    base_frame: usize,
    /// Total frames in the song (for clamping final flush).
    total_frames: usize,
    /// Number of frames that have been flushed to the writer.
    flushed_frames: usize,
    /// Staging buffer for finalized frames before sending to the writer.
    /// Reused across flush calls to avoid allocation.
    flush_staging: Vec<f32>,
}

impl OlaRing {
    /// Create a new OLA ring. `chunk_size` is the inference window size,
    /// `hop_size` is the step between chunks (chunk_size / 2 for 50% overlap).
    pub fn new(channels: usize, chunk_size: usize, hop_size: usize, total_frames: usize) -> Self {
        let sample_count = chunk_size * channels;
        Self {
            channels,
            chunk_size,
            hop_size,
            accum: vec![0.0; sample_count],
            norm: vec![0.0; chunk_size],
            base_frame: 0,
            total_frames,
            flushed_frames: 0,
            flush_staging: Vec::with_capacity(sample_count),
        }
    }

    /// Add windowed output for a chunk starting at `chunk_start_frame`.
    /// `stem_samples` is interleaved PCM for `chunk_frame_count` frames.
    /// `window` is the Hann window values (squared during OLA).
    ///
    /// This accumulates the windowed samples and normalization into the
    /// ring. If the new chunk would extend beyond the ring's capacity,
    /// the ring is first shifted: finalized frames before
    /// `chunk_start_frame` are flushed through `sink`, then the remaining
    /// data is shifted to the start of the ring.
    pub fn add_chunk(
        &mut self,
        chunk_start_frame: usize,
        chunk_frame_count: usize,
        stem_samples: &[f32],
        window: &[f32],
        sink: impl FnMut(&[f32]) -> Result<()>,
    ) -> Result<()> {
        let channels = self.channels;
        let chunk_size = self.chunk_size;
        let ring_end = self.base_frame + chunk_size;

        if chunk_start_frame >= ring_end {
            // No overlap with current ring contents — flush everything
            // and reset the ring to start at this chunk.
            self.flush_all(sink)?;
            self.accum.fill(0.0);
            self.norm.fill(0.0);
            self.base_frame = chunk_start_frame;
        } else {
            // Check if the chunk would extend beyond the ring capacity.
            let ring_offset = chunk_start_frame - self.base_frame;
            let chunk_end_in_ring = ring_offset + chunk_frame_count;
            if chunk_end_in_ring > chunk_size {
                // Shift the ring so the chunk fits. This flushes finalized
                // frames (those before chunk_start_frame) through the sink.
                self.shift_to(chunk_start_frame, sink)?;
            }
        }

        let ring_offset = chunk_start_frame - self.base_frame;
        let chunk_end_in_ring = ring_offset + chunk_frame_count;

        if chunk_end_in_ring > chunk_size {
            anyhow::bail!(
                "OLA ring overflow: chunk end frame {} exceeds ring capacity {} (base {})",
                chunk_end_in_ring,
                chunk_size,
                self.base_frame
            );
        }

        // Accumulate windowed samples and normalization.
        for (frame, &w) in window.iter().take(chunk_frame_count).enumerate() {
            let w2 = w * w;
            let ring_frame = ring_offset + frame;
            self.norm[ring_frame] += w2;

            let sample_base = frame * channels;
            let ring_base = ring_frame * channels;
            for ch in 0..channels {
                self.accum[ring_base + ch] += stem_samples[sample_base + ch] * w2;
            }
        }

        Ok(())
    }

    /// Flush frames that can no longer be modified by a future chunk.
    /// `safe_through_frame` is the exclusive upper bound of finalized frames.
    /// The `sink` callback receives the interleaved PCM for the flushed
    /// frames.
    pub fn flush_finalized(
        &mut self,
        safe_through_frame: usize,
        mut sink: impl FnMut(&[f32]) -> Result<()>,
    ) -> Result<()> {
        let channels = self.channels;
        let flush_end = safe_through_frame.min(self.total_frames);

        if flush_end <= self.flushed_frames {
            return Ok(());
        }

        self.flush_staging.clear();
        self.flush_staging
            .reserve((flush_end - self.flushed_frames) * channels);

        for frame in self.flushed_frames..flush_end {
            let ring_frame = frame - self.base_frame;
            if ring_frame >= self.chunk_size {
                // Frame is beyond the ring — output silence.
                for _ in 0..channels {
                    self.flush_staging.push(0.0);
                }
                continue;
            }

            let norm = self.norm[ring_frame];
            let ring_base = ring_frame * channels;
            if norm > 1e-8 {
                for ch in 0..channels {
                    self.flush_staging.push(self.accum[ring_base + ch] / norm);
                }
            } else {
                for _ in 0..channels {
                    self.flush_staging.push(0.0);
                }
            }
        }

        sink(&self.flush_staging)?;
        self.flushed_frames = flush_end;
        Ok(())
    }

    /// Flush all remaining frames in the ring. Called after the last chunk.
    pub fn flush_all(&mut self, sink: impl FnMut(&[f32]) -> Result<()>) -> Result<()> {
        self.flush_finalized(self.total_frames, sink)
    }

    /// Shift the ring to make room for a new chunk starting at
    /// `new_base_frame`. Finalizes and flushes frames before
    /// `new_base_frame`, then shifts remaining data to the start.
    pub fn shift_to(
        &mut self,
        new_base_frame: usize,
        mut sink: impl FnMut(&[f32]) -> Result<()>,
    ) -> Result<()> {
        // First flush everything up to new_base_frame.
        self.flush_finalized(new_base_frame, &mut sink)?;

        let shift = new_base_frame - self.base_frame;
        if shift == 0 {
            return Ok(());
        }

        let channels = self.channels;
        let chunk_size = self.chunk_size;

        if shift >= chunk_size {
            self.accum.fill(0.0);
            self.norm.fill(0.0);
            self.base_frame = new_base_frame;
            return Ok(());
        }

        // Shift accum and norm left by `shift` frames.
        let remaining = chunk_size - shift;
        self.accum
            .copy_within(shift * channels..chunk_size * channels, 0);
        self.norm.copy_within(shift..chunk_size, 0);

        // Zero the vacated tail.
        for i in remaining * channels..chunk_size * channels {
            self.accum[i] = 0.0;
        }
        for i in remaining..chunk_size {
            self.norm[i] = 0.0;
        }

        self.base_frame = new_base_frame;
        Ok(())
    }

    pub fn flushed_frames(&self) -> usize {
        self.flushed_frames
    }

    pub fn total_frames(&self) -> usize {
        self.total_frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hann_window(size: usize) -> Vec<f32> {
        (0..size)
            .map(|i| {
                let phase = std::f64::consts::TAU * i as f64 / size as f64;
                (0.5 * (1.0 - phase.cos())).sqrt() as f32
            })
            .collect()
    }

    #[test]
    fn ola_ring_reconstructs_constant_signal() {
        let channels = 2;
        let chunk_size = 256;
        let hop = chunk_size / 2;
        let total_frames = chunk_size * 3;
        let window = hann_window(chunk_size);

        let mut ring = OlaRing::new(channels, chunk_size, hop, total_frames);
        let mut all_output = vec![0.0_f32; total_frames * channels];

        // Helper: run a closure that flushes, then copy flushed data into
        // all_output at the correct position based on flushed_frames before
        // and after the call.
        macro_rules! flush_and_capture {
            ($call:expr) => {{
                let prev = ring.flushed_frames();
                let mut captured = Vec::new();
                $call(&mut |flushed: &[f32]| {
                    captured.extend_from_slice(flushed);
                    Ok(())
                })
                .unwrap();
                let frames = captured.len() / channels;
                for f in 0..frames {
                    for ch in 0..channels {
                        all_output[(prev + f) * channels + ch] = captured[f * channels + ch];
                    }
                }
            }};
        }

        for chunk_start in (0..total_frames).step_by(hop) {
            let chunk_frames = (total_frames - chunk_start).min(chunk_size);
            let stem = vec![1.0_f32; chunk_frames * channels];
            flush_and_capture! {
                |sink| ring.add_chunk(chunk_start, chunk_frames, &stem, &window, sink)
            }

            let safe_through = chunk_start;
            flush_and_capture! {
                |sink| ring.flush_finalized(safe_through, sink)
            }
        }

        // Flush remaining.
        flush_and_capture! {
            |sink| ring.flush_all(sink)
        }

        let total_flushed = ring.flushed_frames();
        assert_eq!(total_flushed, total_frames);

        // Interior samples should reconstruct to ~1.0.
        let mut interior_count = 0;
        for frame in hop..total_frames - hop {
            for ch in 0..channels {
                let val = all_output[frame * channels + ch];
                assert!(
                    (val - 1.0).abs() < 0.01,
                    "frame {frame} ch {ch} = {val}, expected ~1.0"
                );
                interior_count += 1;
            }
        }
        assert!(interior_count > 0, "should have tested interior samples");
    }

    #[test]
    fn ola_ring_memory_is_bounded() {
        let channels = 2;
        let chunk_size = 1024;
        let total_frames = chunk_size * 100;

        let ring = OlaRing::new(channels, chunk_size, chunk_size / 2, total_frames);

        assert_eq!(ring.accum.len(), chunk_size * channels);
        assert_eq!(ring.norm.len(), chunk_size);
        assert!(ring.accum.len() < total_frames * channels);
    }

    #[test]
    fn ola_ring_shift_advances_base() {
        let channels = 1;
        let chunk_size = 8;
        let hop = 4;
        let total_frames = 32;
        let window = hann_window(chunk_size);

        let mut ring = OlaRing::new(channels, chunk_size, hop, total_frames);

        let stem = vec![1.0; chunk_size];
        ring.add_chunk(0, chunk_size, &stem, &window, |_| Ok(()))
            .unwrap();

        ring.shift_to(hop, |_| Ok(())).unwrap();
        assert_eq!(ring.base_frame, hop);

        let stem2 = vec![1.0; chunk_size];
        ring.add_chunk(hop, chunk_size, &stem2, &window, |_| Ok(()))
            .unwrap();

        let mut got_data = false;
        ring.flush_finalized(hop * 2, |flushed| {
            if !flushed.is_empty() {
                got_data = true;
            }
            Ok(())
        })
        .unwrap();
        assert!(got_data || ring.flushed_frames() > 0);
    }

    #[test]
    fn sink_error_does_not_advance_flushed_frames() {
        let channels = 1;
        let chunk_size = 8;
        let window = hann_window(chunk_size);
        let mut ring = OlaRing::new(channels, chunk_size, chunk_size / 2, chunk_size);
        let stem = vec![1.0; chunk_size];

        ring.add_chunk(0, chunk_size, &stem, &window, |_| Ok(()))
            .unwrap();
        let error = ring
            .flush_finalized(chunk_size / 2, |_| anyhow::bail!("injected sink failure"))
            .expect_err("sink failure must propagate");

        assert!(error.to_string().contains("injected sink failure"));
        assert_eq!(ring.flushed_frames(), 0);
    }

    #[test]
    fn ola_ring_auto_resets_when_chunk_beyond_ring() {
        let channels = 1;
        let chunk_size = 8;
        let total_frames = 32;
        let window = hann_window(chunk_size);

        let mut ring = OlaRing::new(channels, chunk_size, chunk_size / 2, total_frames);

        let stem = vec![1.0; chunk_size];
        let result = ring.add_chunk(chunk_size * 2, chunk_size, &stem, &window, |_| Ok(()));
        assert!(result.is_ok());
        assert_eq!(ring.base_frame, chunk_size * 2);
    }
}
