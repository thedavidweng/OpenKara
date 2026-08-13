use crate::audio::decode::DecodedAudio;
use rubato::Resampler;

use super::ResamplerCache;

pub(super) fn mix_stem_resampled(
    output: &mut [f32],
    audio: &DecodedAudio,
    start_frame: u64,
    gain: f32,
    device_sample_rate: u32,
    device_channels: usize,
    resampler_cache: Option<&mut ResamplerCache>,
) -> (usize, u64) {
    // Zero gain must still render (silence) and consume source frames:
    // the consumed count drives the transport clock, and skipping it
    // freezes playback at exactly zero volume (#379). Same lockstep
    // invariant as muted stems in the mix buses below (#143).
    if audio.sample_rate_hz == device_sample_rate {
        return mix_stem_same_rate(output, audio, start_frame, gain, device_channels);
    }

    if let Some(cache) = resampler_cache {
        return mix_stem_rubato(
            output,
            audio,
            start_frame,
            gain,
            device_sample_rate,
            device_channels,
            cache,
        );
    }

    mix_stem_linearly_resampled(
        output,
        audio,
        start_frame,
        gain,
        device_sample_rate,
        device_channels,
    )
}

fn mix_stem_same_rate(
    output: &mut [f32],
    audio: &DecodedAudio,
    start_frame: u64,
    gain: f32,
    device_channels: usize,
) -> (usize, u64) {
    let src_channels = audio.channels;
    let total_src_frames = audio.samples.len() / src_channels;
    let src_start_frame = start_frame as usize;
    if src_start_frame >= total_src_frames {
        return (0, 0);
    }

    let output_frames = output.len() / device_channels;
    let available_frames = (total_src_frames - src_start_frame).min(output_frames);

    for out_frame in 0..available_frames {
        let src_frame = src_start_frame + out_frame;
        for out_ch in 0..device_channels {
            let src_ch = if out_ch < src_channels {
                out_ch
            } else {
                out_ch % src_channels
            };
            let sample = audio.samples[src_frame * src_channels + src_ch];
            output[out_frame * device_channels + out_ch] += sample * gain;
        }
    }

    (available_frames * device_channels, available_frames as u64)
}

fn mix_stem_linearly_resampled(
    output: &mut [f32],
    audio: &DecodedAudio,
    start_frame: u64,
    gain: f32,
    device_sample_rate: u32,
    device_channels: usize,
) -> (usize, u64) {
    let src_rate = audio.sample_rate_hz as f64;
    let dst_rate = device_sample_rate as f64;
    let src_channels = audio.channels;
    let total_src_frames = audio.samples.len() / src_channels;

    let src_start_frame = start_frame as usize;
    if src_start_frame >= total_src_frames {
        return (0, 0);
    }

    let output_frames = output.len() / device_channels;
    let rate_ratio = src_rate / dst_rate;
    let mut written = 0;
    let mut rendered_out_frames: usize = 0;

    for out_frame in 0..output_frames {
        let src_pos = src_start_frame as f64 + out_frame as f64 * rate_ratio;
        let src_frame_lo = src_pos as usize;

        if src_frame_lo >= total_src_frames {
            break;
        }

        let can_interpolate = src_frame_lo + 1 < total_src_frames;
        let frac = (src_pos - src_frame_lo as f64) as f32;

        for out_ch in 0..device_channels {
            let src_ch = if out_ch < src_channels {
                out_ch
            } else {
                out_ch % src_channels
            };
            let idx_lo = src_frame_lo * src_channels + src_ch;
            let sample = if can_interpolate && frac > 0.0 {
                let idx_hi = (src_frame_lo + 1) * src_channels + src_ch;
                audio.samples[idx_lo] * (1.0 - frac) + audio.samples[idx_hi] * frac
            } else {
                audio.samples[idx_lo]
            };
            output[out_frame * device_channels + out_ch] += sample * gain;
        }

        rendered_out_frames = out_frame + 1;
        written = rendered_out_frames * device_channels;
    }

    let src_frames_consumed = (rendered_out_frames as f64 * rate_ratio).round() as u64;

    (written, src_frames_consumed)
}

fn mix_stem_rubato(
    output: &mut [f32],
    audio: &DecodedAudio,
    start_frame: u64,
    gain: f32,
    device_sample_rate: u32,
    device_channels: usize,
    resampler_cache: &mut ResamplerCache,
) -> (usize, u64) {
    let src_channels = audio.channels;
    let total_src_frames = audio.samples.len() / src_channels;
    let src_start_frame = start_frame as usize;

    if src_start_frame >= total_src_frames {
        return (0, 0);
    }

    let output_frames = output.len() / device_channels;
    if output_frames == 0 {
        return (0, 0);
    }

    // Feed exactly input_frames_next(); zero-pad only at end-of-track.
    let Some(entry) = resampler_cache.get_or_create_mut(
        audio.sample_rate_hz,
        device_sample_rate,
        0,
        output_frames,
    ) else {
        return (0, 0);
    };
    let input_needed = entry.resampler.input_frames_next();
    let real_available = total_src_frames - src_start_frame;
    let frames_from_source = real_available.min(input_needed);
    let feed_frames = input_needed;

    let mut max_out_frames = 0usize;

    // One mono resampler per source channel; planar in, interleaved out.
    for src_ch in 0..src_channels {
        let Some(entry) = resampler_cache.get_or_create_mut(
            audio.sample_rate_hz,
            device_sample_rate,
            src_ch,
            output_frames,
        ) else {
            continue;
        };

        // Explicitly zero the tail: resize leaves stale samples past real frames.
        entry.channel_input.resize(feed_frames, 0.0);
        entry.channel_input[frames_from_source..].fill(0.0);
        for (frame, slot) in entry
            .channel_input
            .iter_mut()
            .enumerate()
            .take(frames_from_source)
        {
            *slot = audio.samples[(src_start_frame + frame) * src_channels + src_ch];
        }

        entry.input_vecs[0] = std::mem::take(&mut entry.channel_input);
        let input_adapter = match rubato::audioadapter_buffers::direct::SequentialSliceOfVecs::new(
            &entry.input_vecs,
            1,
            feed_frames,
        ) {
            Ok(adapter) => adapter,
            Err(_) => {
                entry.channel_input = std::mem::take(&mut entry.input_vecs[0]);
                continue;
            }
        };

        let output_adapter = match entry.resampler.process(&input_adapter, None) {
            Ok(out) => out,
            Err(_) => {
                entry.channel_input = std::mem::take(&mut entry.input_vecs[0]);
                continue;
            }
        };

        entry.channel_input = std::mem::take(&mut entry.input_vecs[0]);

        let out_data = output_adapter.take_data();

        let frames_to_write = out_data.len().min(output_frames);
        for out_frame in 0..frames_to_write {
            let sample = out_data[out_frame];
            for out_ch in 0..device_channels {
                let target_ch = if out_ch < src_channels {
                    out_ch
                } else {
                    out_ch % src_channels
                };
                if target_ch == src_ch {
                    output[out_frame * device_channels + out_ch] += sample * gain;
                }
            }
        }
        max_out_frames = max_out_frames.max(frames_to_write);
    }

    let src_frames_consumed = frames_from_source as u64;

    (max_out_frames * device_channels, src_frames_consumed)
}

/// Resample source-domain mix (`feed_frames` interleaved, first `real_frames`
/// live) into device output. Returns `(rendered_samples, real_frames)`.
fn resample_mix_to_output(
    output: &mut [f32],
    mix: &[f32],
    real_frames: usize,
    feed_frames: usize,
    src_channels: usize,
    src_rate: u32,
    device_sample_rate: u32,
    device_channels: usize,
    resampler_cache: &mut ResamplerCache,
) -> (usize, u64) {
    let output_frames = output.len() / device_channels;

    if src_rate == device_sample_rate {
        let frames_to_write = real_frames.min(output_frames);
        for out_frame in 0..frames_to_write {
            for out_ch in 0..device_channels {
                let src_ch = if out_ch < src_channels {
                    out_ch
                } else {
                    out_ch % src_channels
                };
                output[out_frame * device_channels + out_ch] +=
                    mix[out_frame * src_channels + src_ch];
            }
        }
        return (frames_to_write * device_channels, real_frames as u64);
    }

    let mut max_out_frames = 0usize;

    for src_ch in 0..src_channels {
        let Some(entry) =
            resampler_cache.get_or_create_mut(src_rate, device_sample_rate, src_ch, output_frames)
        else {
            continue;
        };

        entry.channel_input.resize(feed_frames, 0.0);
        entry.channel_input[real_frames..].fill(0.0);
        for (frame, slot) in entry.channel_input.iter_mut().enumerate().take(real_frames) {
            *slot = mix[frame * src_channels + src_ch];
        }

        entry.input_vecs[0] = std::mem::take(&mut entry.channel_input);
        let input_adapter = match rubato::audioadapter_buffers::direct::SequentialSliceOfVecs::new(
            &entry.input_vecs,
            1,
            feed_frames,
        ) {
            Ok(adapter) => adapter,
            Err(_) => {
                entry.channel_input = std::mem::take(&mut entry.input_vecs[0]);
                continue;
            }
        };

        let output_adapter = match entry.resampler.process(&input_adapter, None) {
            Ok(out) => out,
            Err(_) => {
                entry.channel_input = std::mem::take(&mut entry.input_vecs[0]);
                continue;
            }
        };

        entry.channel_input = std::mem::take(&mut entry.input_vecs[0]);
        let out_data = output_adapter.take_data();

        let frames_to_write = out_data.len().min(output_frames);
        for out_frame in 0..frames_to_write {
            let sample = out_data[out_frame];
            for out_ch in 0..device_channels {
                let target_ch = if out_ch < src_channels {
                    out_ch
                } else {
                    out_ch % src_channels
                };
                if target_ch == src_ch {
                    output[out_frame * device_channels + out_ch] += sample;
                }
            }
        }
        max_out_frames = max_out_frames.max(frames_to_write);
    }

    (max_out_frames * device_channels, real_frames as u64)
}

/// Source-domain mix bus: every stem (including muted) pops the same frame
/// range, then one resample. Prevents inter-stem drift when a stem is muted.
pub(super) fn render_streaming_mix_bus(
    output: &mut [f32],
    consumers: &mut [&mut crate::audio::streaming::AudioConsumer],
    gains: &[f32],
    stem_scratch: &mut Vec<f32>,
    mix_scratch: &mut Vec<f32>,
    device_sample_rate: u32,
    device_channels: usize,
    resampler_cache: &mut ResamplerCache,
) -> (usize, u64) {
    let output_frames = output.len() / device_channels;
    if output_frames == 0 || consumers.is_empty() {
        return (0, 0);
    }

    let src_rate = consumers[0].sample_rate_hz;
    let src_channels = consumers[0].channels.max(1);

    // Shared pop count for every stem this callback.
    let input_needed = if src_rate == device_sample_rate {
        output_frames
    } else {
        let Some(entry) =
            resampler_cache.get_or_create_mut(src_rate, device_sample_rate, 0, output_frames)
        else {
            return (0, 0);
        };
        entry.resampler.input_frames_next()
    };

    let mut budget = input_needed;
    for consumer in consumers.iter() {
        budget = budget.min(consumer.available_src_frames());
    }

    if budget == 0 {
        return (0, 0);
    }

    let mix_len = budget * src_channels;
    mix_scratch.resize(mix_len, 0.0);
    mix_scratch[..mix_len].fill(0.0);
    stem_scratch.resize(mix_len, 0.0);

    for (i, consumer) in consumers.iter_mut().enumerate() {
        let gain = gains.get(i).copied().unwrap_or(0.0);

        // Muted stems still pop so transport stays lockstep.
        let popped = consumer.pop_samples(&mut stem_scratch[..mix_len]);
        let popped_frames = popped / src_channels;

        if gain == 0.0 {
            continue;
        }

        for frame in 0..popped_frames {
            let src_off = frame * src_channels;
            let mix_off = frame * src_channels;
            for ch in 0..src_channels {
                mix_scratch[mix_off + ch] += stem_scratch[src_off + ch] * gain;
            }
        }
    }

    resample_mix_to_output(
        output,
        mix_scratch,
        budget,
        input_needed,
        src_channels,
        src_rate,
        device_sample_rate,
        device_channels,
        resampler_cache,
    )
}

/// Decoded-path source-domain mix bus (same lockstep rule as streaming).
pub(super) fn render_decoded_mix_bus(
    output: &mut [f32],
    stems: &[&DecodedAudio],
    gains: &[f32],
    start_frame: u64,
    mix_scratch: &mut Vec<f32>,
    device_sample_rate: u32,
    device_channels: usize,
    resampler_cache: &mut ResamplerCache,
) -> (usize, u64) {
    let output_frames = output.len() / device_channels;
    if output_frames == 0 || stems.is_empty() {
        return (0, 0);
    }

    let src_rate = stems[0].sample_rate_hz;
    let src_channels = stems[0].channels.max(1);

    let input_needed = if src_rate == device_sample_rate {
        output_frames
    } else {
        let Some(entry) =
            resampler_cache.get_or_create_mut(src_rate, device_sample_rate, 0, output_frames)
        else {
            return (0, 0);
        };
        entry.resampler.input_frames_next()
    };

    let src_start = start_frame as usize;
    let mut budget = input_needed;
    for stem in stems.iter() {
        let total_frames = stem.samples.len() / stem.channels.max(1);
        let remaining = total_frames.saturating_sub(src_start);
        budget = budget.min(remaining);
    }

    if budget == 0 {
        return (0, 0);
    }

    let mix_len = budget * src_channels;
    mix_scratch.resize(mix_len, 0.0);
    mix_scratch[..mix_len].fill(0.0);

    for (i, stem) in stems.iter().enumerate() {
        let gain = gains.get(i).copied().unwrap_or(0.0);
        if gain == 0.0 {
            // Muted: still advances with budget (lockstep).
            continue;
        }
        for frame in 0..budget {
            let src_off = (src_start + frame) * src_channels;
            let mix_off = frame * src_channels;
            for ch in 0..src_channels {
                mix_scratch[mix_off + ch] += stem.samples[src_off + ch] * gain;
            }
        }
    }

    resample_mix_to_output(
        output,
        mix_scratch,
        budget,
        input_needed,
        src_channels,
        src_rate,
        device_sample_rate,
        device_channels,
        resampler_cache,
    )
}

#[cfg(test)]
mod tests {
    use super::render_streaming_mix_bus;
    use crate::audio::crossfade::CROSSFADE_SCRATCH_FRAMES;
    use crate::audio::eq::EqProcessor;
    use crate::audio::output::{render_output_buffer, ResamplerCache};
    use crate::audio::playback::PlaybackController;

    #[test]
    fn streaming_resample_keeps_lookahead_sample_for_next_callback() {
        use crate::audio::streaming;

        // The mix bus uses rubato sinc resampling with FixedAsync::Output.
        // Each callback consumes `input_frames_next()` source frames and
        // produces exactly `output_frames` device frames. The resampler
        // maintains internal sinc state across callbacks, so consecutive
        // callbacks join seamlessly — no lookahead sample is lost.
        let (mut prod, mut consumer) = streaming::create_stream_pair(4, 1);
        let input = [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(prod.push_samples(&input), input.len());

        let mut first = vec![0.0_f32; 4];
        let mut scratch = Vec::new();
        let mut mix_scratch = Vec::new();
        let mut rc = ResamplerCache::new();
        let gains = [1.0f32];
        let mut consumers: [&mut streaming::AudioConsumer; 1] = [&mut consumer];
        let (rendered, consumed) = render_streaming_mix_bus(
            &mut first,
            &mut consumers,
            &gains,
            &mut scratch,
            &mut mix_scratch,
            8,
            1,
            &mut rc,
        );
        assert_eq!(rendered, 4, "mix bus should fill the output buffer");
        assert!(
            consumed > 0 && consumed <= 6,
            "mix bus should consume > 0 and <= available source frames, got {consumed}"
        );

        // Second callback: the remaining source frames should be consumed.
        let mut second = vec![0.0_f32; 4];
        let (_rendered2, consumed2) = render_streaming_mix_bus(
            &mut second,
            &mut consumers,
            &gains,
            &mut scratch,
            &mut mix_scratch,
            8,
            1,
            &mut rc,
        );
        // The total consumed across both callbacks must not exceed the input.
        let total_consumed = consumed + consumed2;
        assert!(
            total_consumed <= input.len() as u64,
            "total consumed {total_consumed} must not exceed input length {}",
            input.len()
        );
    }

    #[test]
    fn muted_streaming_stem_advances_in_lockstep_with_audible_stem() {
        use crate::audio::streaming;

        // Issue #143 core invariant: a muted stem (gain == 0.0) must be
        // popped over the exact same source-frame range as an audible stem.
        // After N callbacks, both consumers must have been drained by the
        // same total source-frame count, so restoring the muted stem
        // produces zero inter-stem offset.
        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        let (mut prod_a, mut consumer_a) = streaming::create_stream_pair(sample_rate, channels);
        let (mut prod_m, mut consumer_m) = streaming::create_stream_pair(sample_rate, channels);

        // Fill both stems with the same amount of data.
        let frames = sample_rate as usize; // 1 second
        let filler = vec![0.5_f32; frames * channels];
        prod_a.push_samples(&filler);
        prod_m.push_samples(&filler);

        let device_channels = 2;
        let buffer_frames = 512usize;
        let mut scratch = Vec::new();
        let mut mix_scratch = Vec::new();
        let mut rc = ResamplerCache::new();

        // Mix both stems: audible gain=1.0, muted gain=0.0.
        let gains = [1.0f32, 0.0f32];
        let mut consumers: [&mut streaming::AudioConsumer; 2] = [&mut consumer_a, &mut consumer_m];

        let mut output = vec![0.0f32; buffer_frames * device_channels];
        let (_rendered, consumed) = render_streaming_mix_bus(
            &mut output,
            &mut consumers,
            &gains,
            &mut scratch,
            &mut mix_scratch,
            sample_rate,
            device_channels,
            &mut rc,
        );

        // Both consumers must have been drained by exactly `consumed` frames.
        let a_remaining = consumer_a.available_src_frames();
        let m_remaining = consumer_m.available_src_frames();
        assert_eq!(
            a_remaining, m_remaining,
            "audible and muted stems must have identical remaining frame counts"
        );
        let expected_remaining = frames - consumed as usize;
        assert_eq!(
            a_remaining, expected_remaining,
            "audible stem remaining {a_remaining} must equal initial {frames} - consumed {consumed}"
        );
        assert_eq!(
            m_remaining, expected_remaining,
            "muted stem remaining {m_remaining} must equal initial {frames} - consumed {consumed}"
        );
    }

    /// R4: When one stem has fewer samples than another, the source clock must
    /// NOT advance past the slow stem. The mix bus budget is min(available per
    /// stem, input_needed), so the clock advances only by the minimum.
    #[test]
    fn streaming_two_stem_clock_does_not_advance_past_slow_stem() {
        use crate::audio::streaming;

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        // Stem 1 (vocals): plenty of data
        let (mut prod1, mut consumer1) = streaming::create_stream_pair(sample_rate, channels);
        let filler1 = vec![0.5_f32; sample_rate as usize * channels]; // 1 second
        prod1.push_samples(&filler1);

        // Stem 2 (accompaniment): only 100ms of data — will under-render
        let (mut prod2, mut consumer2) = streaming::create_stream_pair(sample_rate, channels);
        let filler2 = vec![0.3_f32; (sample_rate as usize / 10) * channels]; // 100ms
        prod2.push_samples(&filler2);

        let device_channels = 2;
        let mut scratch = Vec::new();
        let mut mix_scratch = Vec::new();
        let mut rc = ResamplerCache::new();

        let big_frames = (sample_rate / 10) as usize; // 4410 frames = 100ms
        let mut big_output = vec![0.0f32; big_frames * device_channels];

        // First callback: both stems have enough for 4410 frames.
        let gains = [1.0f32, 1.0f32];
        let mut consumers: [&mut streaming::AudioConsumer; 2] = [&mut consumer1, &mut consumer2];
        let (_rendered_1, frames_1) = render_streaming_mix_bus(
            &mut big_output,
            &mut consumers,
            &gains,
            &mut scratch,
            &mut mix_scratch,
            sample_rate,
            device_channels,
            &mut rc,
        );
        assert!(frames_1 > 0, "first callback should render frames");

        // Second callback: stem1 still has data, stem2 has nothing.
        // Budget = min(available_stem1, available_stem2=0, input_needed) = 0.
        big_output.fill(0.0);
        let (_rendered_2, frames_2) = render_streaming_mix_bus(
            &mut big_output,
            &mut consumers,
            &gains,
            &mut scratch,
            &mut mix_scratch,
            sample_rate,
            device_channels,
            &mut rc,
        );

        assert_eq!(
            frames_2, 0,
            "when one stem has no data, mix bus budget must be 0 (min)"
        );
    }

    /// R4: Four-stem variant — when one of four stems under-renders, the source
    /// clock must advance only by the minimum rendered count.
    #[test]
    fn streaming_four_stem_clock_uses_minimum_across_stems() {
        use crate::audio::streaming;

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;

        let (mut p_v, mut c_v) = streaming::create_stream_pair(sample_rate, channels);
        let (mut p_d, mut c_d) = streaming::create_stream_pair(sample_rate, channels);
        let (mut p_b, mut c_b) = streaming::create_stream_pair(sample_rate, channels);
        let (_p_o, mut c_o) = streaming::create_stream_pair(sample_rate, channels);

        // Fill all stems generously except "other" which is empty
        let fill = vec![0.5_f32; sample_rate as usize * channels];
        p_v.push_samples(&fill);
        p_d.push_samples(&fill);
        p_b.push_samples(&fill);
        // p_o: no data

        let device_channels = 2;
        let frames = 512usize;
        let mut output = vec![0.0f32; frames * device_channels];
        let mut scratch = Vec::new();
        let mut mix_scratch = Vec::new();
        let mut rc = ResamplerCache::new();

        let gains = [1.0f32, 1.0f32, 1.0f32, 1.0f32];
        let mut consumers: [&mut streaming::AudioConsumer; 4] =
            [&mut c_v, &mut c_d, &mut c_b, &mut c_o];
        let (_rendered, src_frames) = render_streaming_mix_bus(
            &mut output,
            &mut consumers,
            &gains,
            &mut scratch,
            &mut mix_scratch,
            sample_rate,
            device_channels,
            &mut rc,
        );

        assert_eq!(
            src_frames, 0,
            "four-stem mix bus must not advance when any stem has no data"
        );
    }

    fn build_two_stem_streaming_controller(
        sample_rate: u32,
        channels: usize,
        frames: usize,
    ) -> PlaybackController {
        use crate::audio::streaming::{self, StreamingTrack};

        let (mut prod_v, cons_v) = streaming::create_stream_pair(sample_rate, channels);
        let (mut prod_a, cons_a) = streaming::create_stream_pair(sample_rate, channels);
        let filler = vec![0.5f32; frames * channels];
        prod_v.push_samples(&filler);
        prod_a.push_samples(&filler);

        let mut controller = PlaybackController::default();
        controller.start_track_streaming(
            "test-twostem".to_owned(),
            sample_rate,
            channels,
            (frames * 1000 / sample_rate as usize) as u64,
            StreamingTrack::TwoStem {
                vocals: cons_v,
                accompaniment: cons_a,
            },
            0,
        );
        controller.play(0).unwrap();
        controller.fade = crate::audio::playback::FadeState::None;
        controller
    }

    /// After N callbacks with one stem muted, both stems must have been
    /// drained by exactly the same total source-frame count. This is the
    /// core #143 invariant: muting is amplitude-only, not clock-affecting.
    #[test]
    fn mix_bus_mute_restore_preserves_zero_inter_stem_offset() {
        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        let total_frames = sample_rate as usize * 5; // 5 seconds
        let mut controller =
            build_two_stem_streaming_controller(sample_rate, channels, total_frames);

        // Mute the vocals stem.
        let _ = controller.set_stem_volume(crate::audio::playback::StemName::Vocals, 0.0);

        let device_channels = 2;
        let buffer_frames = 512usize;
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();

        // Run 50 callbacks with vocals muted.
        for _ in 0..50 {
            let mut output = vec![0.0f32; buffer_frames * device_channels];
            let _ = render_output_buffer(
                &mut controller,
                &mut output,
                &mut Vec::new(),
                &mut Vec::new(),
                &mut crossfade_scratch,
                sample_rate,
                device_channels,
                &mut rc,
                &mut rc_in,
                &mut EqProcessor::new(sample_rate, device_channels),
                &mut peak_acc,
                &ring,
            );
        }

        // Both stems must have identical remaining frame counts.
        let track = controller.current_track.as_ref().unwrap();
        let streaming = track.streaming.as_ref().unwrap();
        if let crate::audio::streaming::StreamingTrack::TwoStem {
            vocals,
            accompaniment,
        } = streaming
        {
            let v_remaining = vocals.available_src_frames();
            let a_remaining = accompaniment.available_src_frames();
            assert_eq!(
                v_remaining, a_remaining,
                "after muted callbacks, vocals and accompaniment must have identical remaining frames"
            );
        } else {
            panic!("expected TwoStem streaming track");
        }

        // Restore vocals and run 50 more callbacks.
        let _ = controller.set_stem_volume(crate::audio::playback::StemName::Vocals, 1.0);
        for _ in 0..50 {
            let mut output = vec![0.0f32; buffer_frames * device_channels];
            let _ = render_output_buffer(
                &mut controller,
                &mut output,
                &mut Vec::new(),
                &mut Vec::new(),
                &mut crossfade_scratch,
                sample_rate,
                device_channels,
                &mut rc,
                &mut rc_in,
                &mut EqProcessor::new(sample_rate, device_channels),
                &mut peak_acc,
                &ring,
            );
        }

        // Still identical — no drift accumulated.
        let track = controller.current_track.as_ref().unwrap();
        let streaming = track.streaming.as_ref().unwrap();
        if let crate::audio::streaming::StreamingTrack::TwoStem {
            vocals,
            accompaniment,
        } = streaming
        {
            let v_remaining = vocals.available_src_frames();
            let a_remaining = accompaniment.available_src_frames();
            assert_eq!(
                v_remaining, a_remaining,
                "after restore, stems must still have identical remaining frames (no drift)"
            );
        } else {
            panic!("expected TwoStem streaming track");
        }
    }

    /// #379: at exactly zero master volume the transport clock must keep
    /// advancing (rendering silence), so EOF and auto-advance still fire.
    #[test]
    fn zero_master_volume_still_advances_transport_clock() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::playback::FadeState;

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        let frames = sample_rate as usize; // 1 second

        let mut controller = PlaybackController::default();
        controller.start_track(
            "song-a".to_owned(),
            DecodedAudio {
                sample_rate_hz: sample_rate,
                channels,
                duration_ms: 1_000,
                samples: vec![0.5; frames * channels],
            },
            0,
        );
        controller.play(0).expect("track should start");
        controller.fade = FadeState::None;
        controller.set_volume(0.0).expect("volume should clamp");

        let device_channels = 2;
        let buffer_frames = 512usize;
        let mut output = vec![0.0f32; buffer_frames * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert_eq!(
            controller.current_render_frame(),
            buffer_frames as u64,
            "transport clock must advance at zero volume"
        );
        assert!(
            output.iter().all(|s| *s == 0.0),
            "zero-volume output must be silence"
        );
    }

    /// Rate-converted variant of the zero-volume clock test: the rubato path
    /// must also consume source frames at zero gain.
    #[test]
    fn zero_master_volume_advances_clock_through_resampler() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::playback::FadeState;

        let src_rate: u32 = 44_100;
        let device_rate: u32 = 48_000;
        let channels: usize = 2;
        let frames = src_rate as usize; // 1 second

        let mut controller = PlaybackController::default();
        controller.start_track(
            "song-a".to_owned(),
            DecodedAudio {
                sample_rate_hz: src_rate,
                channels,
                duration_ms: 1_000,
                samples: vec![0.5; frames * channels],
            },
            0,
        );
        controller.play(0).expect("track should start");
        controller.fade = FadeState::None;
        controller.set_volume(0.0).expect("volume should clamp");

        let device_channels = 2;
        let buffer_frames = 512usize;
        let mut output = vec![0.0f32; buffer_frames * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(
            controller.current_render_frame() > 0,
            "transport clock must advance at zero volume through the resampler"
        );
        assert!(
            output.iter().all(|s| *s == 0.0),
            "zero-volume output must be silence"
        );
    }

    #[test]
    fn mix_bus_produces_audio_when_one_stem_is_muted() {
        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        let total_frames = sample_rate as usize; // 1 second
        let mut controller =
            build_two_stem_streaming_controller(sample_rate, channels, total_frames);

        // Mute vocals, keep accompaniment audible.
        let _ = controller.set_stem_volume(crate::audio::playback::StemName::Vocals, 0.0);

        let device_channels = 2;
        let buffer_frames = 512usize;
        let mut output = vec![0.0f32; buffer_frames * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(
            rendered > 0,
            "mix bus must render audio when one stem is audible"
        );
        let max_sample = output[..rendered]
            .iter()
            .fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(
            max_sample > 0.0,
            "output must contain non-zero samples from the audible stem"
        );
    }

    /// Decoded multi-stem mix bus: all stems must be read over the same
    /// source-frame range. After rendering, the render_frame must advance
    /// by exactly the number of source frames consumed (not per-stem max).
    #[test]
    fn decoded_mix_bus_advances_render_frame_by_consumed_frames() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::playback::{FadeState, LoadedStems, PlaybackController};

        let sample_rate = 44_100;
        let channels = 2;
        let frames = 1024;
        let decoded = |sample| DecodedAudio {
            sample_rate_hz: sample_rate,
            channels,
            duration_ms: (frames * 1000 / sample_rate as usize) as u64,
            samples: vec![sample; frames * channels],
        };

        let mut controller = PlaybackController::default();
        controller.start_track("song-a".to_owned(), decoded(0.0), 0);
        controller
            .attach_stems(
                "song-a",
                LoadedStems::TwoStem {
                    vocals: decoded(0.5),
                    accompaniment: decoded(0.3),
                },
            )
            .expect("stems should attach");
        controller.play(0).expect("track should start");
        controller.fade = FadeState::None;

        let device_channels = 2;
        let buffer_frames = 256usize;
        let mut output = vec![0.0f32; buffer_frames * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert_eq!(rendered, buffer_frames * device_channels);
        // render_frame should have advanced by exactly buffer_frames (same rate).
        let track = controller.current_track.as_ref().unwrap();
        assert_eq!(
            track.render_frame, buffer_frames as u64,
            "render_frame must advance by the source frames consumed, not per-stem max"
        );
    }

    /// Decoded mix bus with mismatched sample rates: the mix bus resamples
    /// the completed source-domain mix once, producing device-rate output.
    /// The render_frame advances by source frames, not device frames.
    #[test]
    fn decoded_mix_bus_resamples_once_with_mismatched_rates() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::playback::{FadeState, LoadedStems, PlaybackController};

        let src_rate = 44_100;
        let device_rate = 48_000;
        let channels = 2;
        let frames = src_rate as usize * 5; // 5 seconds of source audio
        let decoded = |sample| DecodedAudio {
            sample_rate_hz: src_rate,
            channels,
            duration_ms: 5000,
            samples: vec![sample; frames * channels],
        };

        let mut controller = PlaybackController::default();
        controller.start_track("song-a".to_owned(), decoded(0.0), 0);
        controller
            .attach_stems(
                "song-a",
                LoadedStems::TwoStem {
                    vocals: decoded(0.5),
                    accompaniment: decoded(0.3),
                },
            )
            .expect("stems should attach");
        controller.play(0).expect("track should start");
        controller.fade = FadeState::None;

        let device_channels = 2;
        let buffer_frames = 512usize;
        let mut output = vec![0.0f32; buffer_frames * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(rendered > 0, "mix bus must render audio with resampling");
        // render_frame advances by source frames consumed, which is ~buffer_frames * src/dst.
        // The rubato sinc resampler's first callback consumes extra frames to prime
        // its delay line, so the tolerance is generous for the first callback.
        let track = controller.current_track.as_ref().unwrap();
        assert!(
            track.render_frame > 0,
            "render_frame must advance after a resampled callback"
        );
        let expected_approx = (buffer_frames as f64 * src_rate as f64 / device_rate as f64) as u64;
        assert!(
            (track.render_frame as i64 - expected_approx as i64).abs() <= 128,
            "render_frame {} should be ~{} (buffer_frames * src/dst), \
             allowing sinc filter priming overhead",
            track.render_frame,
            expected_approx
        );
    }
}
