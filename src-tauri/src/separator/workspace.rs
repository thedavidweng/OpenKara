//! Reusable separation workspace with bounded memory.
//!
//! The workspace holds all per-chunk buffers needed for the streaming
//! separation path. Buffers are allocated once at construction time and
//! reused across all inference chunks, so steady-state chunk processing
//! performs no large heap allocations.
//!
//! The additional separation working set is bounded by the chunk size.
//! The caller intentionally retains one normalized full-song input buffer;
//! this workspace never allocates full-song output buffers.

use crate::config::StemMode;
use crate::separator::ola::OlaRing;

/// Reusable workspace for streaming stem separation.
///
/// All buffers are sized to the inference chunk window and reused across
/// chunks. No buffer grows with song duration.
pub struct SeparationWorkspace {
    /// Hann window values for OLA. Size: `chunk_size`.
    pub window: Vec<f32>,

    /// Channels-first backing storage for the mix window. It is filled
    /// directly from the normalized interleaved source and borrowed by the
    /// spectral session's forward transform and its `mix` input tensor, so no
    /// per-chunk copy of the window occurs.
    pub tensor_input_backing: Vec<f32>,

    /// Reusable interleaved stem output buffers. Used to hold one chunk's
    /// worth of interleaved PCM for each stem before feeding to OLA rings.
    /// Allocated once and reused across chunks — no per-chunk allocation.
    /// Size: 4 buffers, each `chunk_size * channels`.
    pub stem_output_buffers: [Vec<f32>; 4],

    /// OLA rings for TwoStem mode: vocals and accompaniment.
    pub two_stem_rings: Option<TwoStemRings>,

    /// OLA rings for FourStem mode: drums, bass, other, vocals.
    pub four_stem_rings: Option<FourStemRings>,

    /// Number of channels (always 2 for Demucs stereo).
    pub channels: usize,

    /// Inference window size in frames.
    pub chunk_size: usize,

    /// Hop size between chunks (chunk_size / 2 for 50% overlap).
    pub hop_size: usize,
}

/// OLA rings for TwoStem output.
pub struct TwoStemRings {
    pub vocals: OlaRing,
    pub accompaniment: OlaRing,
}

/// OLA rings for FourStem output.
pub struct FourStemRings {
    pub drums: OlaRing,
    pub bass: OlaRing,
    pub other: OlaRing,
    pub vocals: OlaRing,
}

impl SeparationWorkspace {
    /// Create a new workspace for the given stem mode and chunk geometry.
    /// All buffers are allocated up front and reused for the entire song.
    pub fn new(
        stem_mode: StemMode,
        channels: usize,
        chunk_size: usize,
        hop_size: usize,
        total_frames: usize,
    ) -> Self {
        let sample_count = chunk_size * channels;

        let (two_stem_rings, four_stem_rings) = match stem_mode {
            StemMode::TwoStem => (
                Some(TwoStemRings {
                    vocals: OlaRing::new(channels, chunk_size, hop_size, total_frames),
                    accompaniment: OlaRing::new(channels, chunk_size, hop_size, total_frames),
                }),
                None,
            ),
            StemMode::FourStem => (
                None,
                Some(FourStemRings {
                    drums: OlaRing::new(channels, chunk_size, hop_size, total_frames),
                    bass: OlaRing::new(channels, chunk_size, hop_size, total_frames),
                    other: OlaRing::new(channels, chunk_size, hop_size, total_frames),
                    vocals: OlaRing::new(channels, chunk_size, hop_size, total_frames),
                }),
            ),
        };

        Self {
            window: hann_window(chunk_size),
            tensor_input_backing: vec![0.0; sample_count],
            stem_output_buffers: [
                vec![0.0; sample_count],
                vec![0.0; sample_count],
                vec![0.0; sample_count],
                vec![0.0; sample_count],
            ],
            two_stem_rings,
            four_stem_rings,
            channels,
            chunk_size,
            hop_size,
        }
    }

    /// Fill the planar input buffer from interleaved source PCM for a chunk
    /// starting at `chunk_start_frame` with `chunk_frame_count` valid frames.
    /// The tail beyond `chunk_frame_count` is zeroed.
    pub fn fill_planar_input(
        &mut self,
        source: &[f32],
        chunk_start_frame: usize,
        chunk_frame_count: usize,
    ) {
        let channels = self.channels;
        let chunk_size = self.chunk_size;

        // Zero the entire tensor backing first (tail remains zero).
        self.tensor_input_backing.fill(0.0);

        // Deinterleave directly into the borrowed ORT input backing.
        for frame in 0..chunk_frame_count {
            let src_offset = (chunk_start_frame + frame) * channels;
            for ch in 0..channels {
                let planar_offset = ch * chunk_size + frame;
                self.tensor_input_backing[planar_offset] = source[src_offset + ch];
            }
        }
    }

    /// Get the window values.
    pub fn window(&self) -> &[f32] {
        &self.window
    }

    /// Get the tensor input backing storage (the planar mix window).
    pub fn tensor_input(&self) -> &[f32] {
        &self.tensor_input_backing
    }
}

/// Generate a Hann window of the given size for overlap-add processing.
/// Sine window satisfying the squared constant-overlap-add constraint at 50%
/// overlap: w[n]^2 + w[n + N/2]^2 = 1.
///
/// This is equivalent to `sqrt(hann)` and is the standard choice for
/// overlap-add processing where chunks are windowed, processed, then
/// overlap-added with the same window (squared normalization).
pub(crate) fn hann_window(size: usize) -> Vec<f32> {
    (0..size)
        .map(|i| {
            let phase = std::f64::consts::TAU * i as f64 / size as f64;
            (0.5 * (1.0 - phase.cos())).sqrt() as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_memory_is_bounded_by_chunk_size() {
        let channels = 2;
        let chunk_size = 4096;
        let total_frames = chunk_size * 1000; // very long song

        let ws = SeparationWorkspace::new(
            StemMode::TwoStem,
            channels,
            chunk_size,
            chunk_size / 2,
            total_frames,
        );

        // All buffers should be chunk_size * channels, not total_frames.
        assert_eq!(ws.tensor_input_backing.len(), chunk_size * channels);
        assert_eq!(ws.window.len(), chunk_size);
    }

    #[test]
    fn workspace_fill_planar_input_deinterleaves() {
        let channels = 2;
        let chunk_size = 8;
        let mut ws = SeparationWorkspace::new(
            StemMode::TwoStem,
            channels,
            chunk_size,
            chunk_size / 2,
            chunk_size * 2,
        );

        // Interleaved: [L0, R0, L1, R1, ...]
        let source: Vec<f32> = (0..16).map(|i| i as f32).collect();
        ws.fill_planar_input(&source, 0, 4);

        // Planar: channel 0 = [0, 2, 4, 6, 0, 0, 0, 0]
        //         channel 1 = [1, 3, 5, 7, 0, 0, 0, 0]
        assert_eq!(ws.tensor_input_backing[0], 0.0); // ch0 frame0
        assert_eq!(ws.tensor_input_backing[1], 2.0); // ch0 frame1
        assert_eq!(ws.tensor_input_backing[2], 4.0); // ch0 frame2
        assert_eq!(ws.tensor_input_backing[3], 6.0); // ch0 frame3
        assert_eq!(ws.tensor_input_backing[4], 0.0); // ch0 frame4 (zeroed tail)

        assert_eq!(ws.tensor_input_backing[chunk_size], 1.0); // ch1 frame0
        assert_eq!(ws.tensor_input_backing[chunk_size + 1], 3.0); // ch1 frame1
        assert_eq!(ws.tensor_input_backing[chunk_size + 2], 5.0); // ch1 frame2
        assert_eq!(ws.tensor_input_backing[chunk_size + 3], 7.0); // ch1 frame3
    }

    #[test]
    fn workspace_reuses_tensor_backing() {
        let channels = 2;
        let chunk_size = 8;
        let mut ws = SeparationWorkspace::new(
            StemMode::TwoStem,
            channels,
            chunk_size,
            chunk_size / 2,
            chunk_size * 4,
        );

        // The planar mix backing is allocated once and reused across every
        // chunk fill — no per-chunk reallocation.
        let tensor_ptr = ws.tensor_input().as_ptr();
        let source = vec![0.25_f32; chunk_size * channels * 2];
        ws.fill_planar_input(&source, 0, chunk_size);
        ws.fill_planar_input(&source, chunk_size, chunk_size);

        assert_eq!(ws.tensor_input().as_ptr(), tensor_ptr);
    }

    #[test]
    fn workspace_two_stem_has_two_rings() {
        let ws = SeparationWorkspace::new(StemMode::TwoStem, 2, 1024, 512, 4096);
        assert!(ws.two_stem_rings.is_some());
        assert!(ws.four_stem_rings.is_none());
    }

    #[test]
    fn workspace_four_stem_has_four_rings() {
        let ws = SeparationWorkspace::new(StemMode::FourStem, 2, 1024, 512, 4096);
        assert!(ws.two_stem_rings.is_none());
        assert!(ws.four_stem_rings.is_some());
    }
}
