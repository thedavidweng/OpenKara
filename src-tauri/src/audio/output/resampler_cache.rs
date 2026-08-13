use rubato::{Async, FixedAsync, SincInterpolationParameters, SincInterpolationType};
use std::collections::HashMap;

#[derive(Default)]
pub struct ResamplerCache {
    pub(super) cache: HashMap<(u32, u32, usize, usize), ResamplerEntry>,
}

fn resample_ratio(src_rate: u32, dst_rate: u32) -> f64 {
    dst_rate as f64 / src_rate as f64
}

pub(super) struct ResamplerEntry {
    pub(super) resampler: Async<f32>,
    pub(super) channel_input: Vec<f32>,
    pub(super) input_vecs: Vec<Vec<f32>>,
}

impl ResamplerCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn swap(&mut self, other: &mut ResamplerCache) {
        std::mem::swap(&mut self.cache, &mut other.cache);
    }

    /// One rubato resampler per (rate pair, channel, output_chunk); shared
    /// filter state across channels phase-blurs. Chunk size is keyed because
    /// `FixedAsync::Output` fixes the output frame count at creation.
    pub(super) fn get_or_create_mut(
        &mut self,
        src_rate: u32,
        dst_rate: u32,
        channel: usize,
        output_chunk: usize,
    ) -> &mut ResamplerEntry {
        self.cache
            .entry((src_rate, dst_rate, channel, output_chunk))
            .or_insert_with(|| {
                let params = SincInterpolationParameters {
                    sinc_len: 128,
                    f_cutoff: Some(rubato::calculate_cutoff(
                        128,
                        rubato::WindowFunction::Blackman2,
                    )),
                    interpolation: SincInterpolationType::Quadratic,
                    oversampling_factor: 256,
                    window: rubato::WindowFunction::Blackman2,
                };
                // FixedAsync::Output: feed input_frames_next() real frames per
                // call. FixedAsync::Input zero-padded every callback and corrupted
                // the sinc delay line at chunk boundaries.
                let resampler = Async::<f32>::new_sinc(
                    resample_ratio(src_rate, dst_rate),
                    1.1, // max relative ratio
                    &params,
                    output_chunk, // chunk_size = output frames per call
                    1,            // channels (mono; we de-interleave per channel)
                    FixedAsync::Output,
                )
                .expect("failed to create rubato resampler");
                ResamplerEntry {
                    resampler,
                    channel_input: Vec::new(),
                    input_vecs: vec![Vec::new()],
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::resample_ratio;

    #[test]
    fn resample_ratio_is_output_rate_over_input_rate() {
        let upsample = resample_ratio(44_100, 48_000);
        let downsample = resample_ratio(48_000, 44_100);
        assert!((upsample - 48_000.0 / 44_100.0).abs() < f64::EPSILON);
        assert!((downsample - 44_100.0 / 48_000.0).abs() < f64::EPSILON);
        assert!(upsample > 1.0);
        assert!(downsample < 1.0);
    }
}
