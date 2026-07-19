//! Realtime peak envelope visualizer data path.
//!
//! The audio callback publishes post-EQ, post-limiter, post-fade peaks into a
//! fixed-size single-writer ring. The frontend polls a copied snapshot at 30 Hz
//! and draws it on a DPR-aware canvas.
//!
//! # Realtime constraints
//!
//! After `PeakRing::new`, no callback operation may:
//! - lock a mutex
//! - allocate (no `Vec`/`String`/`format!`)
//! - log per sample/callback
//! - serialize or emit an event
//! - perform a syscall
//!
//! The writer publishes one pair with `Relaxed` stores followed by a `Release`
//! on `write_index`. The reader loads `write_index` with `Acquire`, copies at
//! most the latest `PEAK_RING_CAPACITY` entries, then retries once if the index
//! changed during the copy. This is deliberately a lossy observability channel.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const PEAK_WINDOW_FRAMES: u64 = 512;
pub const PEAK_RING_CAPACITY: usize = 256;

/// A fixed-size lock-free ring buffer for stereo peak pairs.
///
/// One CPAL writer publishes pairs; any number of readers may copy snapshots.
/// Each slot stores `f32::to_bits()` in an `AtomicU32` so the writer never
/// needs to allocate or lock.
///
/// # Single-writer requirement
///
/// Only one thread (the CPAL output callback) may call `push`. Multiple readers
/// are safe because all reads use atomic loads with `Acquire`/`Relaxed`.
pub struct PeakRing {
    slots: Box<[[AtomicU32; 2]]>,
    write_index: AtomicU64,
}

impl PeakRing {
    pub fn new() -> Self {
        let mut slots: Vec<[AtomicU32; 2]> = Vec::with_capacity(PEAK_RING_CAPACITY);
        for _ in 0..PEAK_RING_CAPACITY {
            slots.push([AtomicU32::new(0), AtomicU32::new(0)]);
        }
        Self {
            slots: slots.into_boxed_slice(),
            write_index: AtomicU64::new(0),
        }
    }

    /// Publish one sanitized, clamped stereo peak pair.
    ///
    /// # Realtime safety
    ///
    /// No allocation, no lock, no log, no event, no syscall. The writer stores
    /// both slot atomics with `Relaxed`, then publishes `write_index + 1` with
    /// `Release` so a reader that loads the new index with `Acquire` sees the
    /// preceding slot writes.
    pub fn push(&self, left: f32, right: f32) {
        let sanitized_left = sanitize_peak(left);
        let sanitized_right = sanitize_peak(right);
        let idx = self.write_index.load(Ordering::Relaxed);
        let slot = &self.slots[idx as usize % PEAK_RING_CAPACITY];
        slot[0].store(sanitized_left.to_bits(), Ordering::Relaxed);
        slot[1].store(sanitized_right.to_bits(), Ordering::Relaxed);
        self.write_index.store(idx + 1, Ordering::Release);
    }

    /// Copy at most the latest `PEAK_RING_CAPACITY` entries in chronological
    /// (oldest-to-newest) order. Returns `(write_index, peaks)`.
    ///
    /// If `write_index` advances during the copy, retry once from the newer
    /// index. After one retry, return the best-effort coherent snapshot.
    ///
    /// Delegates to the private `snapshot_impl` with no-op hooks, so the
    /// release hot path shares one implementation with the deterministic
    /// regression tests (see `snapshot_impl`).
    pub fn snapshot(&self) -> (u64, Vec<[f32; 2]>) {
        self.snapshot_impl(|_| {}, |_, _, _| {})
    }

    fn copy_entries(&self, write_index: u64) -> Vec<[f32; 2]> {
        let count = (write_index as usize).min(PEAK_RING_CAPACITY);
        if count == 0 {
            return Vec::new();
        }
        let start = write_index.saturating_sub(count as u64);
        let mut out = Vec::with_capacity(count);
        for i in start..write_index {
            let slot = &self.slots[i as usize % PEAK_RING_CAPACITY];
            let l = f32::from_bits(slot[0].load(Ordering::Relaxed));
            let r = f32::from_bits(slot[1].load(Ordering::Relaxed));
            out.push([l, r]);
        }
        out
    }

    /// The single snapshot algorithm shared by production `snapshot()` and the
    /// deterministic regression tests.
    ///
    /// Two interception points let a single-threaded test deterministically
    /// force the retry path and a post-retry advancement without threads or
    /// sleeps:
    ///
    /// 1. `after_first_copy`: called after the first `copy_entries` and before
    ///    the second `write_index` load. A test pushes here to force the retry
    ///    path (making `second_index != first_index`).
    ///
    /// 2. `after_retry_copy`: called after the retry `copy_entries` and before
    ///    the return. A test pushes here to advance `write_index` past
    ///    `second_index`, simulating the "third_index" that the buggy code
    ///    would have returned.
    ///
    /// Production `snapshot()` passes no-op closures. Because they are generic
    /// and monomorphized, the compiler inlines and eliminates them, so the
    /// release hot path gains no allocation or synchronization. The lock-free
    /// / audio-thread contract is unchanged: only `Acquire` loads and `Relaxed`
    /// slot reads, no locks.
    ///
    /// The returned cursor must equal `second_index` in all cases, proving the
    /// snapshot does not advertise entries beyond the copied data.
    fn snapshot_impl<F1, F2>(
        &self,
        after_first_copy: F1,
        after_retry_copy: F2,
    ) -> (u64, Vec<[f32; 2]>)
    where
        F1: FnOnce(&Self),
        F2: FnOnce(&Self, u64, &[f32; 2]),
    {
        let first_index = self.write_index.load(Ordering::Acquire);
        let result = self.copy_entries(first_index);
        // Intercept 1: tests may push here to advance write_index, forcing
        // the retry path (second_index != first_index). No-op in production.
        after_first_copy(self);
        let second_index = self.write_index.load(Ordering::Acquire);
        if first_index == second_index {
            return (first_index, result);
        }
        // Retry once from the newer index. Return `second_index` — the index
        // that was actually used for the copy — so the cursor never points
        // past data that was included in the snapshot. Loading a newer index
        // after the copy would advertise entries the reader never observed.
        let retry = self.copy_entries(second_index);
        // Intercept 2: tests may push here to advance write_index past
        // second_index. The return must still use second_index. No-op in
        // production; the guard keeps the `retry.last()` call sound.
        if !retry.is_empty() {
            after_retry_copy(self, second_index, retry.last().unwrap());
        }
        (second_index, retry)
    }
}

impl Default for PeakRing {
    fn default() -> Self {
        Self::new()
    }
}

/// Sanitize a peak sample: non-finite → 0, clamp to `0.0..=1.0`.
fn sanitize_peak(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 1.0)
}

/// Accumulates per-frame maxima over a 512-frame window, then pushes one pair
/// to the shared `PeakRing`.
///
/// Owned by the output callback beside `EqProcessor`. A device restart starts
/// a fresh partial window while retaining the process-wide ring and write
/// counter.
pub struct PeakAccumulator {
    frames_in_window: u64,
    max_left: f32,
    max_right: f32,
}

impl PeakAccumulator {
    pub fn new() -> Self {
        Self {
            frames_in_window: 0,
            max_left: 0.0,
            max_right: 0.0,
        }
    }

    /// Feed one rendered interleaved frame. Mono sources duplicate to both
    /// channels; stereo-or-greater uses channels 0 and 1 only.
    pub fn feed_frame(&mut self, samples: &[f32], channels: usize, ring: &PeakRing) {
        if channels == 0 || samples.len() < channels {
            return;
        }
        let (l, r) = if channels == 1 {
            let m = samples[0].abs();
            (m, m)
        } else {
            (samples[0].abs(), samples[1].abs())
        };
        if l > self.max_left {
            self.max_left = l;
        }
        if r > self.max_right {
            self.max_right = r;
        }
        self.frames_in_window += 1;
        if self.frames_in_window >= PEAK_WINDOW_FRAMES {
            ring.push(self.max_left, self.max_right);
            self.frames_in_window = 0;
            self.max_left = 0.0;
            self.max_right = 0.0;
        }
    }

    /// Feed all rendered frames from an interleaved buffer. Only the first
    /// `rendered_samples` samples participate; trailing zero padding is ignored.
    pub fn process(
        &mut self,
        output: &[f32],
        rendered_samples: usize,
        channels: usize,
        ring: &PeakRing,
    ) {
        if channels == 0 || rendered_samples == 0 {
            return;
        }
        // Clamp to the actual buffer length — the caller may pass a frame count
        // that slightly exceeds the output slice due to resampler rounding.
        let effective_samples = rendered_samples.min(output.len());
        let frame_count = effective_samples / channels;
        for frame in 0..frame_count {
            let start = frame * channels;
            let end = start + channels;
            self.feed_frame(&output[start..end], channels, ring);
        }
    }

    /// Reset the partial window (e.g. on device restart). The process-wide
    /// ring and write counter are retained.
    pub fn reset_window(&mut self) {
        self.frames_in_window = 0;
        self.max_left = 0.0;
        self.max_right = 0.0;
    }
}

impl Default for PeakAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioPeakSnapshot {
    pub write_index: u64,
    pub peaks: Vec<[f32; 2]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_ring_capacity() {
        assert_eq!(PEAK_RING_CAPACITY, 256);
        assert_eq!(PEAK_WINDOW_FRAMES, 512);
    }

    #[test]
    fn test_empty_ring_returns_empty_peaks() {
        let ring = PeakRing::new();
        let (idx, peaks) = ring.snapshot();
        assert_eq!(idx, 0);
        assert!(peaks.is_empty());
    }

    #[test]
    fn test_push_and_snapshot_single_pair() {
        let ring = PeakRing::new();
        ring.push(0.5, 0.75);
        let (idx, peaks) = ring.snapshot();
        assert_eq!(idx, 1);
        assert_eq!(peaks.len(), 1);
        assert!((peaks[0][0] - 0.5).abs() < 1e-6);
        assert!((peaks[0][1] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn test_non_finite_sanitization_and_clamping() {
        let ring = PeakRing::new();
        ring.push(f32::NAN, f32::INFINITY);
        ring.push(-0.5, 2.0);
        let (_, peaks) = ring.snapshot();
        assert_eq!(peaks.len(), 2);
        // NaN → 0, Inf → 0 (non-finite sanitization)
        assert!((peaks[0][0] - 0.0).abs() < 1e-6);
        assert!((peaks[0][1] - 0.0).abs() < 1e-6);
        // Negative → 0, >1 → 1.0
        assert!((peaks[1][0] - 0.0).abs() < 1e-6);
        assert!((peaks[1][1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_chronological_order() {
        let ring = PeakRing::new();
        for i in 0..10u32 {
            let v = i as f32 * 0.1;
            ring.push(v, v);
        }
        let (_, peaks) = ring.snapshot();
        assert_eq!(peaks.len(), 10);
        for (i, pair) in peaks.iter().enumerate() {
            let expected = i as f32 * 0.1;
            assert!((pair[0] - expected).abs() < 1e-6, "left[{i}]");
            assert!((pair[1] - expected).abs() < 1e-6, "right[{i}]");
        }
    }

    #[test]
    fn test_wraparound_at_more_than_twice_capacity() {
        let ring = PeakRing::new();
        let total = (PEAK_RING_CAPACITY * 3) as u32;
        for i in 0..total {
            let v = (i as f32) / (total as f32);
            ring.push(v, v);
        }
        let (idx, peaks) = ring.snapshot();
        assert_eq!(idx as u32, total);
        assert_eq!(peaks.len(), PEAK_RING_CAPACITY);
        let oldest = total as usize - PEAK_RING_CAPACITY;
        let first_expected = oldest as f32 / total as f32;
        assert!(
            (peaks[0][0] - first_expected).abs() < 1e-5,
            "oldest entry should be at index {oldest}, expected {first_expected}, got {}",
            peaks[0][0]
        );
        let last_expected = (total - 1) as f32 / total as f32;
        assert!(
            (peaks[peaks.len() - 1][0] - last_expected).abs() < 1e-5,
            "newest entry expected {last_expected}, got {}",
            peaks[peaks.len() - 1][0]
        );
    }

    #[test]
    fn test_concurrent_reader_stress() {
        use std::sync::Arc;
        use std::thread;
        let ring = Arc::new(PeakRing::new());
        let reader_ring = Arc::clone(&ring);
        let reader = thread::spawn(move || {
            let mut last_idx = 0u64;
            for _ in 0..1000 {
                let (idx, peaks) = reader_ring.snapshot();
                assert!(
                    idx >= last_idx,
                    "index must be monotonic: {idx} < {last_idx}"
                );
                last_idx = idx;
                for pair in &peaks {
                    assert!(pair[0] >= 0.0 && pair[0] <= 1.0);
                    assert!(pair[1] >= 0.0 && pair[1] <= 1.0);
                }
            }
        });
        for i in 0..2000u32 {
            ring.push(i as f32 / 2000.0, i as f32 / 2000.0);
        }
        reader.join().unwrap();
    }

    #[test]
    fn test_accumulator_exact_window_boundary() {
        let ring = PeakRing::new();
        let mut acc = PeakAccumulator::new();
        for _ in 0..511 {
            acc.feed_frame(&[0.5, 0.5], 2, &ring);
        }
        let (idx, _) = ring.snapshot();
        assert_eq!(idx, 0, "no push before 512 frames");
        acc.feed_frame(&[0.9, 0.9], 2, &ring);
        let (idx, peaks) = ring.snapshot();
        assert_eq!(idx, 1);
        assert_eq!(peaks.len(), 1);
        assert!((peaks[0][0] - 0.9).abs() < 1e-6);
        assert!((peaks[0][1] - 0.9).abs() < 1e-6);
    }

    #[test]
    fn test_accumulator_mono_duplication() {
        let ring = PeakRing::new();
        let mut acc = PeakAccumulator::new();
        for _ in 0..512 {
            acc.feed_frame(&[0.7], 1, &ring);
        }
        let (_, peaks) = ring.snapshot();
        assert_eq!(peaks.len(), 1);
        assert!((peaks[0][0] - 0.7).abs() < 1e-6);
        assert!((peaks[0][1] - 0.7).abs() < 1e-6);
    }

    #[test]
    fn test_accumulator_stereo_channel_selection() {
        let ring = PeakRing::new();
        let mut acc = PeakAccumulator::new();
        for _ in 0..512 {
            acc.feed_frame(&[0.3, 0.8], 2, &ring);
        }
        let (_, peaks) = ring.snapshot();
        assert_eq!(peaks.len(), 1);
        assert!((peaks[0][0] - 0.3).abs() < 1e-6);
        assert!((peaks[0][1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_accumulator_non_finite_sanitization() {
        let ring = PeakRing::new();
        let mut acc = PeakAccumulator::new();
        for _ in 0..512 {
            acc.feed_frame(&[f32::NAN, f32::INFINITY], 2, &ring);
        }
        let (_, peaks) = ring.snapshot();
        assert_eq!(peaks.len(), 1);
        assert!((peaks[0][0] - 0.0).abs() < 1e-6);
        assert!((peaks[0][1] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_accumulator_reset_window() {
        let ring = PeakRing::new();
        let mut acc = PeakAccumulator::new();
        for _ in 0..100 {
            acc.feed_frame(&[0.5, 0.5], 2, &ring);
        }
        acc.reset_window();
        assert_eq!(acc.frames_in_window, 0);
        assert_eq!(acc.max_left, 0.0);
        assert_eq!(acc.max_right, 0.0);
        for _ in 0..512 {
            acc.feed_frame(&[0.3, 0.3], 2, &ring);
        }
        let (idx, peaks) = ring.snapshot();
        assert_eq!(idx, 1);
        assert!((peaks[0][0] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn test_process_respects_rendered_samples_only() {
        let ring = PeakRing::new();
        let mut acc = PeakAccumulator::new();
        let channels = 2;
        let rendered = 512 * channels;
        let total = rendered + 100 * channels; // extra padding
        let mut buf = vec![0.6f32; total];
        // Set padding to high values that should be ignored.
        for sample in buf.iter_mut().take(total).skip(rendered) {
            *sample = 1.0;
        }
        acc.process(&buf, rendered, channels, &ring);
        let (idx, peaks) = ring.snapshot();
        assert_eq!(idx, 1);
        assert_eq!(peaks.len(), 1);
        assert!((peaks[0][0] - 0.6).abs() < 1e-6);
        assert!((peaks[0][1] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn test_zero_render_calls_do_not_invent_frames() {
        let ring = PeakRing::new();
        let mut acc = PeakAccumulator::new();
        acc.process(&[0.0; 1024], 0, 2, &ring);
        assert_eq!(acc.frames_in_window, 0);
        let (idx, _) = ring.snapshot();
        assert_eq!(idx, 0);
    }

    #[test]
    fn test_device_restart_keeps_ring_counter() {
        let ring = PeakRing::new();
        let mut acc = PeakAccumulator::new();
        for _ in 0..512 {
            acc.feed_frame(&[0.5, 0.5], 2, &ring);
        }
        let (idx_before, _) = ring.snapshot();
        assert_eq!(idx_before, 1);

        // Device restart: reset accumulator window, but ring counter persists.
        let _acc = PeakAccumulator::new();
        let (idx_after, _) = ring.snapshot();
        assert_eq!(idx_after, 1, "ring counter retained after restart");
    }

    #[test]
    fn test_retry_cursor_not_ahead_of_copied_data() {
        let ring = PeakRing::new();

        // Push 2 pairs with distinct values.
        ring.push(0.10, 0.10); // index 0 → write_index 1
        ring.push(0.20, 0.20); // index 1 → write_index 2
                               // write_index = 2

        let (cursor, peaks) = ring.snapshot_impl(
            // Intercept 1: push after the first copy, before the second load.
            // This forces the retry path (second_index = 3 != first_index = 2).
            |ring| {
                ring.push(0.30, 0.30); // index 2 → write_index 3
            },
            // Intercept 2: push after the retry copy, before the return.
            // This simulates the "third_index" advancement that the buggy
            // code would have returned.
            |ring, second_index, last| {
                // second_index should be 3 (after intercept 1 pushed 0.30).
                assert_eq!(
                    second_index, 3,
                    "second_index should be 3 (after intercept 1 push)",
                );

                // The last entry in the retry copy should be 0.30 (the
                // value at index 2, which is the (second_index-1)th entry).
                assert!(
                    (last[0] - 0.30).abs() < 1e-6,
                    "last retry entry should be 0.30, got {}",
                    last[0],
                );

                // Push one more pair — the buggy code would have loaded
                // write_index after this and returned 4 as the cursor.
                ring.push(0.40, 0.40); // index 3 → write_index 4
            },
        );

        // The cursor must be second_index (3), NOT the post-intercept-2
        // write_index (4).
        assert_eq!(
            cursor, 3,
            "cursor must equal second_index (3), not the post-intercept write_index (4)",
        );

        assert!(!peaks.is_empty());
        let last = peaks.last().unwrap();
        assert!(
            (last[0] - 0.30).abs() < 1e-6,
            "last peak must be 0.30 (the value at second_index-1), got {}",
            last[0],
        );

        let final_index = ring.write_index.load(Ordering::Acquire);
        assert_eq!(
            final_index, 4,
            "write_index should be 4 after intercept 2 push",
        );
    }

    #[test]
    fn test_non_retry_path_returns_first_index() {
        // When the writer does not advance between the first copy and the
        // second load, the snapshot returns first_index (no retry).
        let ring = PeakRing::new();
        ring.push(0.50, 0.50);

        let (cursor, peaks) = ring.snapshot_impl(
            |_| {}, // no push → no retry
            |_, _, _| {
                panic!("after_retry_copy should not be called on non-retry path");
            },
        );

        assert_eq!(cursor, 1);
        assert_eq!(peaks.len(), 1);
        assert!((peaks[0][0] - 0.50).abs() < 1e-6);
    }

    #[test]
    fn test_retry_with_multiple_advancements_cursor_matches_copied_data() {
        // Larger test: push enough pairs to fill part of the ring, then
        // force the retry path and a post-retry advancement. Verify the
        // cursor and the last entry are consistent.
        let ring = PeakRing::new();

        // Push 5 pairs with distinct values.
        for i in 0..5u32 {
            let v = i as f32 * 0.1;
            ring.push(v, v);
        }
        // write_index = 5

        let (cursor, peaks) = ring.snapshot_impl(
            |ring| {
                // Push to force retry: write_index becomes 6.
                ring.push(0.50, 0.50);
            },
            |ring, second_index, last| {
                // second_index should be 6.
                assert_eq!(second_index, 6);

                // Last entry should be 0.50 (the value at index 5).
                assert!((last[0] - 0.50).abs() < 1e-6);

                // Push again: write_index becomes 7.
                ring.push(0.60, 0.60);
            },
        );

        // Cursor must be 6, not 7.
        assert_eq!(cursor, 6);

        // Last peak must be 0.50, not 0.60.
        let last = peaks.last().unwrap();
        assert!((last[0] - 0.50).abs() < 1e-6);

        // All 6 entries should be present (indices 0..5).
        assert_eq!(peaks.len(), 6);
        for (i, pair) in peaks.iter().enumerate() {
            let expected = i as f32 * 0.1;
            assert!((pair[0] - expected).abs() < 1e-6, "peaks[{i}]");
        }
    }
}
