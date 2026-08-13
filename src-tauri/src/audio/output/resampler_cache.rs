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
    ///
    /// Returns `None` for configurations no resampler can serve (zero rate,
    /// zero chunk — rubato accepts an infinite ratio and would consume zero
    /// input forever) and when rubato rejects construction. This runs on the
    /// realtime audio callback, so an invalid track must degrade to silence
    /// rather than unwind the stream; the decode/install boundaries reject
    /// such tracks before playback.
    pub(super) fn get_or_create_mut(
        &mut self,
        src_rate: u32,
        dst_rate: u32,
        channel: usize,
        output_chunk: usize,
    ) -> Option<&mut ResamplerEntry> {
        if src_rate == 0 || dst_rate == 0 || output_chunk == 0 {
            return None;
        }
        match self
            .cache
            .entry((src_rate, dst_rate, channel, output_chunk))
        {
            std::collections::hash_map::Entry::Occupied(entry) => Some(entry.into_mut()),
            std::collections::hash_map::Entry::Vacant(vacant) => {
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
                .ok()?;
                Some(vacant.insert(ResamplerEntry {
                    resampler,
                    channel_input: Vec::new(),
                    input_vecs: vec![Vec::new()],
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{resample_ratio, ResamplerCache};

    #[test]
    fn resample_ratio_is_output_rate_over_input_rate() {
        let upsample = resample_ratio(44_100, 48_000);
        let downsample = resample_ratio(48_000, 44_100);
        assert!((upsample - 48_000.0 / 44_100.0).abs() < f64::EPSILON);
        assert!((downsample - 44_100.0 / 48_000.0).abs() < f64::EPSILON);
        assert!(upsample > 1.0);
        assert!(downsample < 1.0);
    }

    #[test]
    fn valid_rates_create_a_cached_entry() {
        let mut cache = ResamplerCache::new();
        assert!(cache.get_or_create_mut(44_100, 48_000, 0, 512).is_some());
        assert_eq!(cache.cache.len(), 1);
    }

    #[test]
    fn zero_source_rate_returns_none_instead_of_panicking() {
        let mut cache = ResamplerCache::new();
        assert!(cache.get_or_create_mut(0, 48_000, 0, 512).is_none());
        assert!(
            cache.cache.is_empty(),
            "a failed construction must not leave a broken entry behind"
        );
    }

    #[test]
    fn zero_output_chunk_returns_none_instead_of_panicking() {
        let mut cache = ResamplerCache::new();
        assert!(cache.get_or_create_mut(44_100, 48_000, 0, 0).is_none());
        assert!(cache.cache.is_empty());
    }
}
