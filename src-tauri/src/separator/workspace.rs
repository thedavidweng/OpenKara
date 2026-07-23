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

    /// Channels-first backing storage for the ORT audio input tensor.
    /// It is filled directly from the normalized interleaved source and
    /// borrowed by `TensorRef`, so no per-chunk audio tensor copy occurs.
    pub tensor_input_backing: Vec<f32>,

    /// Input tensor shape `[1, channels, chunk_size]`.
    pub input_shape: Vec<i64>,

    /// Audio input name resolved once from the loaded model.
    audio_input_name: Option<String>,

    /// Auxiliary input name, shape, and zero-filled backing storage resolved
    /// once from the loaded model and reused for every chunk.
    auxiliary_inputs: Vec<(String, Vec<i64>, Vec<f32>)>,

    /// Accompaniment scratch buffer for TwoStem mode. Drums, bass, and
    /// other are summed here per-sample before writing to the accompaniment
    /// OLA ring. Size: `chunk_size * channels`.
    pub accompaniment_scratch: Vec<f32>,

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
            input_shape: vec![1, channels as i64, chunk_size as i64],
            audio_input_name: None,
            auxiliary_inputs: Vec::new(),
            accompaniment_scratch: vec![0.0; sample_count],
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

    /// Cache the loaded model's input contract once for the full separation run.
    pub fn configure_model_inputs(
        &mut self,
        audio_input_name: String,
        auxiliary_inputs: Vec<(String, Vec<i64>, Vec<f32>)>,
    ) {
        self.audio_input_name = Some(audio_input_name);
        self.auxiliary_inputs = auxiliary_inputs;
    }

    pub fn audio_input_name(&self) -> Option<&str> {
        self.audio_input_name.as_deref()
    }

    pub fn auxiliary_inputs(&self) -> &[(String, Vec<i64>, Vec<f32>)] {
        &self.auxiliary_inputs
    }

    /// Reset the accompaniment scratch buffer to zero before summing
    /// drums, bass, and other for a new chunk.
    pub fn reset_accompaniment_scratch(&mut self) {
        self.accompaniment_scratch.fill(0.0);
    }

    /// Sum one stem's chunk output into the accompaniment scratch buffer.
    /// `stem_samples` is interleaved PCM for `chunk_frame_count` frames.
    pub fn add_to_accompaniment(&mut self, stem_samples: &[f32], chunk_frame_count: usize) {
        let channels = self.channels;
        for frame in 0..chunk_frame_count {
            let base = frame * channels;
            for ch in 0..channels {
                self.accompaniment_scratch[base + ch] += stem_samples[base + ch];
            }
        }
    }

    /// Get the accompaniment scratch buffer for the current chunk.
    pub fn accompaniment(&self) -> &[f32] {
        &self.accompaniment_scratch
    }

    /// Get the window values.
    pub fn window(&self) -> &[f32] {
        &self.window
    }

    /// Get the input shape for ORT tensor construction.
    pub fn input_shape(&self) -> &[i64] {
        &self.input_shape
    }

    /// Get the tensor input backing storage.
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
        assert_eq!(ws.accompaniment_scratch.len(), chunk_size * channels);
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
    fn workspace_reuses_tensor_and_auxiliary_backing() {
        let channels = 2;
        let chunk_size = 8;
        let mut ws = SeparationWorkspace::new(
            StemMode::TwoStem,
            channels,
            chunk_size,
            chunk_size / 2,
            chunk_size * 4,
        );
        ws.configure_model_inputs(
            "audio".to_string(),
            vec![("aux".to_string(), vec![1, 4], vec![0.0; 4])],
        );

        let tensor_ptr = ws.tensor_input().as_ptr();
        let aux_ptr = ws.auxiliary_inputs()[0].2.as_ptr();
        let source = vec![0.25_f32; chunk_size * channels * 2];
        ws.fill_planar_input(&source, 0, chunk_size);
        ws.fill_planar_input(&source, chunk_size, chunk_size);

        assert_eq!(ws.tensor_input().as_ptr(), tensor_ptr);
        assert_eq!(ws.auxiliary_inputs()[0].2.as_ptr(), aux_ptr);
        assert_eq!(ws.audio_input_name(), Some("audio"));
    }

    #[test]
    fn workspace_accompaniment_scratch_sums_stems() {
        let channels = 2;
        let chunk_size = 4;
        let mut ws = SeparationWorkspace::new(
            StemMode::TwoStem,
            channels,
            chunk_size,
            chunk_size / 2,
            chunk_size * 2,
        );

        let drums = vec![1.0, 0.5, 1.0, 0.5];
        let bass = vec![0.3, 0.3, 0.3, 0.3];
        let other = vec![0.2, 0.2, 0.2, 0.2];

        ws.reset_accompaniment_scratch();
        ws.add_to_accompaniment(&drums, 2);
        ws.add_to_accompaniment(&bass, 2);
        ws.add_to_accompaniment(&other, 2);

        // Sum: [1.5, 1.0, 1.5, 1.0]
        assert_eq!(ws.accompaniment()[0], 1.5);
        assert_eq!(ws.accompaniment()[1], 1.0);
        assert_eq!(ws.accompaniment()[2], 1.5);
        assert_eq!(ws.accompaniment()[3], 1.0);
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
