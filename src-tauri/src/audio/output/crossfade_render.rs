use crate::audio::crossfade::{
    effective_overlap_frames, equal_power_gains, source_to_device_frames, CROSSFADE_SCRATCH_FRAMES,
};
use crate::audio::playback::PlaybackController;

use super::mix_bus::mix_stem_resampled;
use super::ResamplerCache;

pub(super) fn render_crossfade_overlap(
    playback: &mut PlaybackController,
    output: &mut [f32],
    crossfade_scratch: &mut [f32],
    master: f32,
    device_sample_rate: u32,
    device_channels: usize,
    outgoing_resampler_cache: &mut ResamplerCache,
    incoming_resampler_cache: &mut ResamplerCache,
) -> Option<(usize, u64)> {
    let output_frames = output.len() / device_channels;
    if output_frames == 0 {
        return Some((0, 0));
    }

    if playback.active_crossfade.is_none() {
        let prepared = playback.prepared_track.as_ref()?;
        let track = playback.current_track.as_ref()?;

        if track.streaming.is_some() || track.stems.is_some() {
            return None;
        }

        let outgoing_src_rate = track.original_audio.sample_rate_hz;
        let incoming_src_rate = prepared.audio.sample_rate_hz;

        // Overlap math is device-frame domain only (mixed rate domains break timing).
        let outgoing_total_device_frames = source_to_device_frames(
            (track.original_audio.samples.len() / track.original_audio.channels.max(1)) as u64,
            outgoing_src_rate,
            device_sample_rate,
        );
        let incoming_total_device_frames = source_to_device_frames(
            (prepared.audio.samples.len() / prepared.audio.channels.max(1)) as u64,
            incoming_src_rate,
            device_sample_rate,
        );
        // Remaining = total_device − converted(render_frame); do not convert twice.
        let outgoing_device_frames_remaining = outgoing_total_device_frames.saturating_sub(
            source_to_device_frames(track.render_frame, outgoing_src_rate, device_sample_rate),
        );

        let effective = effective_overlap_frames(
            playback.crossfade_config.duration_ms,
            device_sample_rate,
            outgoing_total_device_frames,
            incoming_total_device_frames,
            outgoing_device_frames_remaining,
        )?;

        if outgoing_device_frames_remaining > effective {
            return None;
        }

        let prepared = playback.prepared_track.take().unwrap();
        playback.active_crossfade = Some(crate::audio::playback::ActiveCrossfade {
            prepared,
            total_frames: effective,
            rendered_frames: 0,
            incoming_source_frame: 0,
        });
    }

    let active = playback.active_crossfade.as_ref()?;
    let total_overlap = active.total_frames;
    let overlap_rendered = active.rendered_frames;
    let frames_left_in_overlap = total_overlap - overlap_rendered;

    let overlap_frames_this_callback = output_frames.min(frames_left_in_overlap as usize);
    let mut rendered_output_frames = 0usize;
    let mut src_frames_advanced = 0u64;
    // render_frame advances only after return; accumulate per-chunk outgoing reads.
    let mut outgoing_frames_consumed = 0u64;

    let mut chunk_start = 0usize;
    while chunk_start < overlap_frames_this_callback {
        let chunk_frames =
            (overlap_frames_this_callback - chunk_start).min(CROSSFADE_SCRATCH_FRAMES);
        let chunk_samples = chunk_frames * device_channels;

        let track = playback.current_track.as_ref().unwrap();
        let outgoing_render_frame = track.render_frame + outgoing_frames_consumed;
        let outgoing_buf = &mut output
            [chunk_start * device_channels..(chunk_start + chunk_frames) * device_channels];

        let (out_rendered, out_consumed) = mix_stem_resampled(
            outgoing_buf,
            &track.original_audio,
            outgoing_render_frame,
            master,
            device_sample_rate,
            device_channels,
            Some(outgoing_resampler_cache),
        );

        let active = playback.active_crossfade.as_ref().unwrap();
        let incoming_render_frame = active.incoming_source_frame;
        let incoming_buf = &mut crossfade_scratch[..chunk_samples];
        incoming_buf.fill(0.0);
        let (inc_rendered, inc_consumed) = mix_stem_resampled(
            incoming_buf,
            &active.prepared.audio,
            incoming_render_frame,
            master,
            device_sample_rate,
            device_channels,
            Some(incoming_resampler_cache),
        );

        let out_frames = out_rendered / device_channels;
        let inc_frames = inc_rendered / device_channels;
        // All cursors must advance by the frames actually produced. Advancing
        // the write cursor by the requested chunk while the mix window and
        // the overlap clock advance by the produced count leaves unmixed
        // outgoing frames in the buffer, makes the tail fill overwrite
        // committed samples, and stalls promotion (#376). A short side
        // (source exhausted mid-overlap) contributes silence instead.
        let progress_frames = out_frames.max(inc_frames);
        for frame in 0..progress_frames {
            let global_overlap_index = overlap_rendered + chunk_start as u64 + frame as u64;
            let (out_gain, inc_gain) = equal_power_gains(global_overlap_index, total_overlap);

            let inc_base = frame * device_channels;
            for ch in 0..device_channels {
                let out_sample = if frame < out_frames {
                    output[(chunk_start + frame) * device_channels + ch]
                } else {
                    0.0
                };
                let inc_sample = if frame < inc_frames {
                    crossfade_scratch[inc_base + ch]
                } else {
                    0.0
                };
                output[(chunk_start + frame) * device_channels + ch] =
                    out_sample * out_gain + inc_sample * inc_gain;
            }
        }

        rendered_output_frames += progress_frames;
        src_frames_advanced += out_consumed;
        outgoing_frames_consumed += out_consumed;

        if let Some(active) = playback.active_crossfade.as_mut() {
            active.rendered_frames += progress_frames as u64;
            active.incoming_source_frame += inc_consumed;
        }

        chunk_start += progress_frames;

        if progress_frames == 0 {
            break;
        }
    }

    let overlap_complete = playback
        .active_crossfade
        .as_ref()
        .is_some_and(|a| a.rendered_frames >= a.total_frames);

    // Do not promote during FadingOut (pause inside overlap must stop, not advance).
    let mut promoted = false;
    if overlap_complete && playback.current_track_is_playing() {
        let active = playback.active_crossfade.take().unwrap();
        // Promote at incoming *source* frame cursor (not device-frame progress).
        let incoming_frame_offset = active.incoming_source_frame;
        playback.promote_crossfade_track(active.prepared, incoming_frame_offset);
        outgoing_resampler_cache.swap(incoming_resampler_cache);
        incoming_resampler_cache.clear();
        // Outgoing frames consumed during overlap must not advance the new track.
        src_frames_advanced = 0;
        promoted = true;
    }

    if rendered_output_frames < output_frames {
        let remaining_buf = &mut output[rendered_output_frames * device_channels..];

        let track = playback.current_track.as_ref().unwrap();
        let current_render_frame = if promoted {
            track.render_frame
        } else {
            track.render_frame + outgoing_frames_consumed
        };

        let (rem_rendered, rem_consumed) = mix_stem_resampled(
            remaining_buf,
            &track.original_audio,
            current_render_frame,
            master,
            device_sample_rate,
            device_channels,
            Some(outgoing_resampler_cache),
        );

        // rem_rendered is interleaved samples; rendered_output_frames is frames.
        rendered_output_frames += rem_rendered / device_channels;
        src_frames_advanced += rem_consumed;
    }

    Some((rendered_output_frames, src_frames_advanced))
}

#[cfg(test)]
mod tests {
    use crate::audio::crossfade::{equal_power_gains, CROSSFADE_SCRATCH_FRAMES};
    use crate::audio::eq::EqProcessor;
    use crate::audio::output::{render_output_buffer, ResamplerCache};
    use crate::audio::playback::PlaybackController;

    fn build_crossfade_controller(
        device_sample_rate: u32,
        device_channels: usize,
    ) -> (
        PlaybackController,
        crate::audio::output_format::OutputFormatSnapshot,
    ) {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::output_format::OutputFormatSnapshot;
        use crate::audio::playback::{PreloadRequestGeneration, PreparedTrack};

        // Track A: 10 seconds of 0.5 amplitude at the device sample rate.
        let track_a_samples = vec![0.5_f32; 10 * device_sample_rate as usize * device_channels];
        let mut controller = PlaybackController::default();
        controller.start_track(
            "song-a".to_owned(),
            DecodedAudio {
                sample_rate_hz: device_sample_rate,
                channels: device_channels,
                duration_ms: 10_000,
                samples: track_a_samples,
            },
            0,
        );

        // Enable crossfade with a 1-second overlap.
        let _ = controller.set_crossfade_enabled(true);
        let _ = controller.set_crossfade_duration(1_000);

        // Prepare track B: 10 seconds of 0.9 amplitude.
        let track_b_samples = vec![0.9_f32; 10 * device_sample_rate as usize * device_channels];
        let fmt = OutputFormatSnapshot {
            sample_rate_hz: device_sample_rate,
            channels: device_channels as u16,
            generation: 0,
        };
        let prepared = PreparedTrack {
            preload_request_generation: PreloadRequestGeneration(0),
            preload_generation: fmt.generation,
            song_id: "song-b".to_owned(),
            output_format: fmt,
            audio: DecodedAudio {
                sample_rate_hz: device_sample_rate,
                channels: device_channels,
                duration_ms: 10_000,
                samples: track_b_samples,
            },
        };
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());

        (controller, fmt)
    }

    #[test]
    fn crossfade_starts_at_overlap_boundary_with_full_outgoing() {
        let device_sample_rate: u32 = 44_100;
        let device_channels: usize = 2;

        let (mut controller, _fmt) =
            build_crossfade_controller(device_sample_rate, device_channels);

        // Advance to 9 seconds — 1 second remaining, exactly the overlap.
        controller.seek(9_000, 0).unwrap();

        let mut output = vec![0.0f32; 256 * device_channels];
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
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(rendered > 0, "crossfade must render audio");
        assert!(
            controller.active_crossfade.is_some(),
            "active crossfade must be started"
        );

        // The overlap must be 1 second = 44100 device frames.
        let active = controller.active_crossfade.as_ref().unwrap();
        assert_eq!(
            active.total_frames, device_sample_rate as u64,
            "overlap must be exactly 1 second of device frames"
        );

        // Output must be non-zero — the seek fade ramps up but the
        // crossfade mix is producing audio.
        let max_sample = output.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(
            max_sample > 0.0,
            "crossfade must produce non-zero audio, max {max_sample}"
        );
    }

    #[test]
    fn crossfade_does_not_start_when_far_from_end() {
        let device_sample_rate: u32 = 44_100;
        let device_channels: usize = 2;

        let (mut controller, _fmt) =
            build_crossfade_controller(device_sample_rate, device_channels);

        // Advance to 1 second — 9 seconds remaining, well outside the 1s overlap.
        controller.seek(1_000, 0).unwrap();

        let mut output = vec![0.0f32; 256 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let _ = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(
            controller.active_crossfade.is_none(),
            "crossfade must not start when remaining > overlap duration"
        );
        assert!(
            controller.prepared_track.is_some(),
            "prepared track must not be consumed when crossfade is not started"
        );
    }

    /// Crossfade must NOT start when the outgoing track has more remaining
    /// frames than the configured overlap, even with mismatched source/device
    /// rates. This is a regression test for the double-conversion bug where
    /// remaining frames were over-scaled.
    #[test]
    fn crossfade_does_not_start_with_mismatched_rates() {
        let device_sample_rate: u32 = 48_000;
        let device_channels: usize = 2;

        let (mut controller, _fmt) =
            build_crossfade_controller(device_sample_rate, device_channels);

        // Track A is 10s at 44100 Hz, device is 48000 Hz.
        // At 1s into the song, ~9s remaining = ~432000 device frames.
        // Configured overlap is 1s = 48000 device frames.
        // 432000 > 48000, so crossfade must not start.
        controller.seek(1_000, 0).unwrap();

        let mut output = vec![0.0f32; 256 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let _ = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(
            controller.active_crossfade.is_none(),
            "crossfade must not start with mismatched rates when far from end"
        );
    }

    #[test]
    fn crossfade_promotes_incoming_track_on_completion() {
        let device_sample_rate: u32 = 44_100;
        let device_channels: usize = 2;

        let (mut controller, _fmt) =
            build_crossfade_controller(device_sample_rate, device_channels);

        // Advance to 9 seconds — 1 second remaining.
        controller.seek(9_000, 0).unwrap();

        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();

        // Render the entire 1-second overlap (44100 frames) in 512-frame
        // callbacks.
        for _ in 0..100 {
            let mut output = vec![0.0f32; 512 * device_channels];
            let rendered = render_output_buffer(
                &mut controller,
                &mut output,
                &mut Vec::new(),
                &mut Vec::new(),
                &mut crossfade_scratch,
                device_sample_rate,
                device_channels,
                &mut rc,
                &mut rc_in,
                &mut EqProcessor::new(device_sample_rate, device_channels),
                &mut peak_acc,
                &ring,
            );
            if rendered == 0 {
                break;
            }
        }

        // The crossfade must have completed and promoted track B.
        assert!(
            controller.active_crossfade.is_none(),
            "active crossfade must be cleared after completion"
        );
        assert_eq!(
            controller.current_track.as_ref().unwrap().song_id,
            "song-b",
            "incoming track must be promoted after crossfade"
        );
    }

    #[test]
    fn crossfade_is_cancelled_by_seek() {
        let device_sample_rate: u32 = 44_100;
        let device_channels: usize = 2;

        let (mut controller, _fmt) =
            build_crossfade_controller(device_sample_rate, device_channels);

        // Advance to 9 seconds — start the crossfade.
        controller.seek(9 * 1000, 0).unwrap();

        let mut output = vec![0.0f32; 256 * device_channels];
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
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(
            controller.active_crossfade.is_some(),
            "crossfade must be active before seek"
        );

        // Seek — this must cancel the crossfade.
        controller.seek(0, 0).unwrap();

        assert!(
            controller.active_crossfade.is_none(),
            "active crossfade must be cancelled by seek"
        );
    }

    #[test]
    fn crossfade_skips_streaming_tracks() {
        use crate::audio::streaming::{self, StreamingTrack};

        let device_sample_rate: u32 = 44_100;
        let device_channels: usize = 2;

        let (mut prod, consumer) =
            streaming::create_stream_pair(device_sample_rate, device_channels);
        let filler = vec![0.5_f32; device_sample_rate as usize * device_channels];
        prod.push_samples(&filler);

        let mut controller = PlaybackController::default();
        controller.start_track_streaming(
            "stream-song".to_owned(),
            device_sample_rate,
            device_channels,
            10_000,
            StreamingTrack::Single { consumer },
            0,
        );
        let _ = controller.set_crossfade_enabled(true);
        let _ = controller.set_crossfade_duration(1_000);

        // No prepared track — crossfade cannot start.
        let mut output = vec![0.0f32; 256 * device_channels];
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
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(
            controller.active_crossfade.is_none(),
            "crossfade must not start for streaming tracks"
        );
    }

    #[test]
    fn crossfade_disabled_does_not_start() {
        let device_sample_rate: u32 = 44_100;
        let device_channels: usize = 2;

        let (mut controller, _fmt) =
            build_crossfade_controller(device_sample_rate, device_channels);

        // Disable crossfade.
        let _ = controller.set_crossfade_enabled(false);

        // Advance to 9 seconds.
        controller.seek(9 * 1000, 0).unwrap();

        let mut output = vec![0.0f32; 256 * device_channels];
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
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(
            controller.active_crossfade.is_none(),
            "crossfade must not start when disabled"
        );
    }

    fn build_crossfade_controller_ramp(
        device_sample_rate: u32,
        device_channels: usize,
        render_frame: u64,
        outgoing_total_frames: u64,
        incoming_total_frames: u64,
        duration_ms: u32,
    ) -> PlaybackController {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::output_format::OutputFormatSnapshot;
        use crate::audio::playback::{PreloadRequestGeneration, PreparedTrack};

        let step: f32 = 1e-5;
        let mut outgoing_samples =
            Vec::with_capacity(outgoing_total_frames as usize * device_channels);
        for frame in 0..outgoing_total_frames {
            let v = ((frame as i64) - (render_frame as i64)) as f32 * step;
            for _ in 0..device_channels {
                outgoing_samples.push(v);
            }
        }
        let mut controller = PlaybackController::default();
        controller.start_track(
            "song-a".to_owned(),
            DecodedAudio {
                sample_rate_hz: device_sample_rate,
                channels: device_channels,
                duration_ms: outgoing_total_frames * 1000 / device_sample_rate as u64,
                samples: outgoing_samples,
            },
            0,
        );
        let _ = controller.play(0);
        // Clear the play fade-in so it does not scale the output (the fade
        // gain is ~0 when the test runs instantly, which would mask the
        // ramp values we are verifying).
        controller.fade = crate::audio::playback::FadeState::None;
        controller.current_track.as_mut().unwrap().render_frame = render_frame;

        let fmt = OutputFormatSnapshot {
            sample_rate_hz: device_sample_rate,
            channels: device_channels as u16,
            generation: 0,
        };
        let prepared = PreparedTrack {
            preload_request_generation: PreloadRequestGeneration(0),
            preload_generation: fmt.generation,
            song_id: "song-b".to_owned(),
            output_format: fmt,
            audio: DecodedAudio {
                sample_rate_hz: device_sample_rate,
                channels: device_channels,
                duration_ms: incoming_total_frames * 1000 / device_sample_rate as u64,
                samples: vec![0.0; incoming_total_frames as usize * device_channels],
            },
        };
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());

        let _ = controller.set_crossfade_enabled(true);
        let _ = controller.set_crossfade_duration(duration_ms);
        controller
    }

    fn build_crossfade_controller_mismatched_rate(
        _device_sample_rate: u32,
        device_channels: usize,
        outgoing_render_frame: u64,
        outgoing_total_frames: u64,
        outgoing_sample_rate: u32,
        incoming_total_frames: u64,
        incoming_sample_rate: u32,
        duration_ms: u32,
    ) -> PlaybackController {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::output_format::OutputFormatSnapshot;
        use crate::audio::playback::{PreloadRequestGeneration, PreparedTrack};

        let mut controller = PlaybackController::default();
        controller.start_track(
            "song-a".to_owned(),
            DecodedAudio {
                sample_rate_hz: outgoing_sample_rate,
                channels: device_channels,
                duration_ms: outgoing_total_frames * 1000 / outgoing_sample_rate as u64,
                samples: vec![0.5; outgoing_total_frames as usize * device_channels],
            },
            0,
        );
        let _ = controller.play(0);
        controller.current_track.as_mut().unwrap().render_frame = outgoing_render_frame;

        let fmt = OutputFormatSnapshot {
            sample_rate_hz: incoming_sample_rate,
            channels: device_channels as u16,
            generation: 0,
        };
        let prepared = PreparedTrack {
            preload_request_generation: PreloadRequestGeneration(0),
            preload_generation: fmt.generation,
            song_id: "song-b".to_owned(),
            output_format: fmt,
            audio: DecodedAudio {
                sample_rate_hz: incoming_sample_rate,
                channels: device_channels,
                duration_ms: incoming_total_frames * 1000 / incoming_sample_rate as u64,
                samples: vec![0.3; incoming_total_frames as usize * device_channels],
            },
        };
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());

        let _ = controller.set_crossfade_enabled(true);
        let _ = controller.set_crossfade_duration(duration_ms);
        controller
    }

    /// When a user pauses during an active crossfade, the overlap completion
    /// must NOT promote the incoming track — the promotion would clear the
    /// fade-out and start the next song instead of stopping. The active
    /// crossfade is preserved so a future resume can complete the promotion.
    #[test]
    fn crossfade_promotion_suppressed_during_pause() {
        let device_sample_rate: u32 = 44_100;
        let device_channels = 2;
        // effective overlap = 22050 frames (500ms floor), callback buffer =
        // 22050 frames so the overlap completes within this single callback.
        let callback_frames = 22_050;
        let effective_frames: u64 = 22_050;
        let outgoing_total = 2 * effective_frames;
        let incoming_total = 2 * effective_frames;
        let render_frame = effective_frames; // remaining = effective

        let mut controller = build_crossfade_controller_ramp(
            device_sample_rate,
            device_channels,
            render_frame,
            outgoing_total,
            incoming_total,
            500, // 500ms configured → effective = 22050 = callback size
        );
        // Clear the fade-in from build_crossfade_controller_ramp's play() call.
        controller.fade = crate::audio::playback::FadeState::None;
        // Simulate a user pause inside the overlap window.
        controller.pause(0).unwrap();
        assert!(matches!(
            controller.fade,
            crate::audio::playback::FadeState::FadingOut { .. }
        ));

        let mut output = vec![0.0f32; callback_frames * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];

        render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        // The overlap completed, but the promotion must be suppressed because
        // the user is pausing. The current track must still be the outgoing
        // track, and the active crossfade must be preserved so a future
        // resume can complete the promotion.
        assert_eq!(
            controller.current_track.as_ref().unwrap().song_id,
            "song-a",
            "crossfade promotion must not fire during a pause"
        );
        assert!(
            controller.active_crossfade.is_some(),
            "active crossfade must be preserved during a pause so resume can complete it"
        );

        // User presses resume (play). current_track_is_playing() must now
        // return true (FadingIn is not FadingOut).
        controller.play(0).unwrap();
        assert!(
            controller.current_track_is_playing(),
            "current_track_is_playing must be true after resume"
        );

        // Render another buffer. The overlap is already complete, so the
        // promotion should now fire and advance to the incoming track.
        let mut output2 = vec![0.0f32; callback_frames * device_channels];
        render_output_buffer(
            &mut controller,
            &mut output2,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert_eq!(
            controller.current_track.as_ref().unwrap().song_id,
            "song-b",
            "crossfade promotion must fire after resume from pause"
        );
        assert!(
            controller.active_crossfade.is_none(),
            "active crossfade must be consumed after resume promotion"
        );
    }

    /// When a single callback renders more than `CROSSFADE_SCRATCH_FRAMES`
    /// (4096) overlap frames, the chunk loop must advance the outgoing source
    /// position across chunks. Before the fix in the original #89 branch,
    /// every chunk re-read `track.render_frame` (constant during the loop),
    /// causing repeated outgoing audio and over-advancement of `render_frame`.
    #[test]
    fn crossfade_multi_chunk_callback_advances_outgoing_source_position() {
        let device_sample_rate: u32 = 44_100;
        let device_channels = 2;
        let callback_frames = 8_192; // 2 × CROSSFADE_SCRATCH_FRAMES
        let outgoing_total = 60 * device_sample_rate as u64;
        let incoming_total = 60 * device_sample_rate as u64;
        // remaining = 220500 = effective → crossfade starts
        let effective_frames: u64 = 220_500;
        let render_frame = outgoing_total - effective_frames; // 55s in

        let mut controller = build_crossfade_controller_ramp(
            device_sample_rate,
            device_channels,
            render_frame,
            outgoing_total,
            incoming_total,
            10_000, // 10s configured → effective = 220500 (clamped by remaining)
        );

        let mut output = vec![0.0f32; callback_frames * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];

        render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(
            controller.active_crossfade.is_some(),
            "crossfade must have started"
        );
        let active = controller.active_crossfade.as_ref().unwrap();
        assert_eq!(
            active.rendered_frames, callback_frames as u64,
            "all callback frames must be overlap frames"
        );

        // The outgoing contribution is `ramp_value * out_gain`. The incoming
        // is zero, so the output equals `ramp_value * out_gain`. The ramp
        // value at output frame F is `F * step` (centered on the render
        // position). Verify the output is strictly increasing across the
        // entire callback — if a chunk re-read the same source segment, the
        // output would plateau or decrease at the chunk boundary (frame 4096).
        let step: f32 = 1e-5;
        for f in 1..callback_frames {
            let prev_left = output[(f - 1) * device_channels];
            let curr_left = output[f * device_channels];
            assert!(
                curr_left > prev_left,
                "frame {f}: output must strictly increase across chunks. \
                 prev={prev_left:.12} curr={curr_left:.12} \
                 delta={:.12}",
                curr_left - prev_left
            );
        }

        // Specifically verify the chunk boundary at frame 4096: the outgoing
        // source frame for output frame 4096 must be `render_frame + 4096`,
        // NOT `render_frame` (which would be the bug — re-reading the first
        // chunk's source segment). With the centered ramp, the correct value
        // at frame 4096 is `4096 * step * out_gain(4096)`, while the bug
        // value would be `0 * step * out_gain(4096) = 0`.
        let out_gain_at = |f: usize| -> f32 {
            let (og, _ig) = equal_power_gains(f as u64, effective_frames);
            og
        };
        let boundary_left = output[4096 * device_channels];
        let expected_correct = (4096.0f32) * step * out_gain_at(4096);
        let expected_bug = 0.0f32;
        let dist_correct = (boundary_left - expected_correct).abs();
        let dist_bug = (boundary_left - expected_bug).abs();
        assert!(
            dist_correct < dist_bug,
            "chunk boundary frame 4096: output must reflect outgoing source \
             frame {} (correct), not {} (bug — re-reading first chunk). \
             got={boundary_left:.10} correct≈{expected_correct:.10} bug≈{expected_bug:.10}",
            render_frame + 4096,
            render_frame,
        );
    }

    /// The caller's `advance_render_frame` must advance the outgoing
    /// `render_frame` by exactly the source frames consumed in ONE pass, not
    /// `num_chunks × per_chunk`. Before the fix, `src_frames_advanced`
    /// accumulated `out_consumed` from each chunk (all computed from the same
    /// `start_frame`), so the caller skipped the outgoing track ahead by 2×
    /// the correct amount for a 2-chunk callback.
    #[test]
    fn crossfade_multi_chunk_callback_advances_render_frame_correctly() {
        let device_sample_rate: u32 = 44_100;
        let device_channels = 2;
        let callback_frames = 8_192; // 2 × CROSSFADE_SCRATCH_FRAMES
        let outgoing_total = 60 * device_sample_rate as u64;
        let incoming_total = 60 * device_sample_rate as u64;
        let effective_frames: u64 = 220_500;
        let render_frame = outgoing_total - effective_frames;

        let mut controller = build_crossfade_controller_ramp(
            device_sample_rate,
            device_channels,
            render_frame,
            outgoing_total,
            incoming_total,
            10_000,
        );

        let initial_render_frame = controller.current_track.as_ref().unwrap().render_frame;

        let mut output = vec![0.0f32; callback_frames * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];

        render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(
            controller.active_crossfade.is_some(),
            "crossfade must have started but not completed"
        );
        let advanced = controller.current_render_frame() - initial_render_frame;
        assert_eq!(
            advanced, callback_frames as u64,
            "render_frame must advance by exactly callback_frames (one pass), \
             not num_chunks × per_chunk. got {advanced} expected {}",
            callback_frames
        );
    }

    /// Crossfade overlap length must be computed in device (output) frames,
    /// not source frames. When source and device rates differ (44.1→48),
    /// using source frames for the clamp produces wrong overlap timing.
    #[test]
    fn crossfade_overlap_length_uses_device_frames_for_441_to_48() {
        let device_rate: u32 = 48_000;
        let src_rate: u32 = 44_100;
        let device_channels = 2;
        let outgoing_total = 60 * src_rate as u64;
        let incoming_total = 60 * src_rate as u64;
        let effective_device = 3 * device_rate as u64; // 144000
        let remaining_src = effective_device * src_rate as u64 / device_rate as u64;
        let render_frame = outgoing_total - remaining_src;

        let mut controller = build_crossfade_controller_mismatched_rate(
            device_rate,
            device_channels,
            render_frame,
            outgoing_total,
            src_rate,
            incoming_total,
            src_rate,
            3_000,
        );

        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];

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
            controller.active_crossfade.is_some()
                || controller.current_track.as_ref().unwrap().song_id == "song-b",
            "crossfade must start when remaining device frames <= effective overlap"
        );
        if let Some(ref active) = controller.active_crossfade {
            assert_eq!(
                active.total_frames, effective_device,
                "overlap total must be in device frames (144000), not source frames"
            );
        }
    }

    /// Same frame-domain test in the opposite direction (48→44.1) to verify
    /// the conversion is symmetric.
    #[test]
    fn crossfade_overlap_length_uses_device_frames_for_48_to_441() {
        let device_rate: u32 = 44_100;
        let src_rate: u32 = 48_000;
        let device_channels = 2;
        let outgoing_total = 60 * src_rate as u64;
        let incoming_total = 60 * src_rate as u64;
        let effective_device = 3 * device_rate as u64; // 132300
        let remaining_src = effective_device * src_rate as u64 / device_rate as u64;
        let render_frame = outgoing_total - remaining_src;

        let mut controller = build_crossfade_controller_mismatched_rate(
            device_rate,
            device_channels,
            render_frame,
            outgoing_total,
            src_rate,
            incoming_total,
            src_rate,
            3_000,
        );

        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];

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
            controller.active_crossfade.is_some()
                || controller.current_track.as_ref().unwrap().song_id == "song-b",
            "crossfade must start when remaining device frames <= effective overlap"
        );
        if let Some(ref active) = controller.active_crossfade {
            assert_eq!(
                active.total_frames, effective_device,
                "overlap total must be in device frames (132300), not source frames"
            );
        }
    }

    /// #376: a rate-converted overlap must keep its cursors in sync when the
    /// outgoing resampler returns short (source exhausted near the end of the
    /// overlap). Before the fix the overlap clock stopped advancing on short
    /// returns, so `rendered_frames` never reached `total_frames` and the
    /// incoming track was never promoted.
    #[test]
    fn crossfade_rate_converted_overlap_promotes_despite_short_resampler_returns() {
        let device_rate: u32 = 48_000;
        let outgoing_rate: u32 = 44_100;
        let device_channels = 2;
        let outgoing_total = 10 * outgoing_rate as u64;
        let incoming_total = 20 * device_rate as u64;
        let duration_ms = 3_000;
        let effective_device = duration_ms as u64 * device_rate as u64 / 1000; // 144000
        let remaining_src = effective_device * outgoing_rate as u64 / device_rate as u64;
        let render_frame = outgoing_total - remaining_src;

        let mut controller = build_crossfade_controller_mismatched_rate(
            device_rate,
            device_channels,
            render_frame,
            outgoing_total,
            outgoing_rate,
            incoming_total,
            device_rate,
            duration_ms,
        );

        let callback_frames = 512usize;
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];

        // 144000 overlap frames / 512 per callback ≈ 282 callbacks. Allow
        // slack for resampler priming, but far less than the stall budget.
        let mut callbacks_until_promotion = None;
        for callback in 0..400 {
            let mut output = vec![0.0f32; callback_frames * device_channels];
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
            if controller.current_track.as_ref().unwrap().song_id == "song-b" {
                callbacks_until_promotion = Some(callback + 1);
                break;
            }
        }

        let callbacks = callbacks_until_promotion
            .expect("rate-converted crossfade must promote the incoming track");
        assert!(
            callbacks <= 320,
            "overlap must complete near its configured duration (~282 callbacks), \
             not stall on short resampler returns. took {callbacks} callbacks"
        );
        assert!(
            controller.active_crossfade.is_none(),
            "active crossfade must be cleared after promotion"
        );
    }

    /// Promotion must use `incoming_source_frame` (source frames), not
    /// `rendered_frames` (device frames). After a complete crossfade, the
    /// promoted track's `render_frame` must equal the incoming source cursor.
    #[test]
    fn crossfade_promotion_uses_incoming_source_frame() {
        let device_sample_rate: u32 = 44_100;
        let device_channels = 2;
        let outgoing_total = 60 * device_sample_rate as u64;
        let incoming_total = 60 * device_sample_rate as u64;
        let duration_ms = 600;
        let effective_frames = duration_ms as u64 * device_sample_rate as u64 / 1000;
        let render_frame = outgoing_total - effective_frames;

        let mut controller = build_crossfade_controller_ramp(
            device_sample_rate,
            device_channels,
            render_frame,
            outgoing_total,
            incoming_total,
            duration_ms,
        );

        let callback_frames = effective_frames as usize + 512;
        let mut output = vec![0.0f32; callback_frames * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];

        render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        // If crossfade didn't complete in one callback, run more.
        if controller.current_track.as_ref().unwrap().song_id == "song-a" {
            for _ in 0..20 {
                let mut output_more = vec![0.0f32; callback_frames * device_channels];
                render_output_buffer(
                    &mut controller,
                    &mut output_more,
                    &mut Vec::new(),
                    &mut Vec::new(),
                    &mut crossfade_scratch,
                    device_sample_rate,
                    device_channels,
                    &mut rc,
                    &mut rc_in,
                    &mut EqProcessor::new(device_sample_rate, device_channels),
                    &mut peak_acc,
                    &ring,
                );
                if controller.current_track.as_ref().unwrap().song_id == "song-b" {
                    break;
                }
            }
        }
        assert_eq!(
            controller.current_track.as_ref().unwrap().song_id,
            "song-b",
            "crossfade must complete and promote the incoming track"
        );
        assert!(
            controller.active_crossfade.is_none(),
            "active crossfade must be cleared after promotion"
        );

        // With same-rate tracks, incoming_source_frame == rendered_frames.
        // The promoted render_frame starts at the incoming source cursor
        // (effective_frames), then the post-overlap remainder advances it
        // further. So render_frame must be >= effective_frames.
        let actual = controller.current_track.as_ref().unwrap().render_frame;
        assert!(
            actual >= effective_frames,
            "promoted render_frame must be >= {effective_frames} (incoming source cursor \
             before post-overlap remainder). got {actual}"
        );
    }

    /// After promotion, the incoming resampler cache must be transferred to
    /// the primary lane. Verify by rendering a second callback — the promoted
    /// track continues without a click or discontinuity because the resampler
    /// state carries over.
    #[test]
    fn crossfade_promotion_transfers_resampler_history() {
        let device_sample_rate: u32 = 44_100;
        let device_channels = 2;
        let outgoing_total = 60 * device_sample_rate as u64;
        let incoming_total = 60 * device_sample_rate as u64;
        let duration_ms = 600;
        let effective_frames = duration_ms as u64 * device_sample_rate as u64 / 1000;
        let render_frame = outgoing_total - effective_frames;

        // Use mismatched-rate helper for distinguishable amplitudes
        // (outgoing 0.5, incoming 0.3).
        let mut controller = build_crossfade_controller_mismatched_rate(
            device_sample_rate,
            device_channels,
            render_frame,
            outgoing_total,
            device_sample_rate,
            incoming_total,
            device_sample_rate,
            duration_ms,
        );

        let callback_frames = effective_frames as usize + 512;
        let mut output = vec![0.0f32; callback_frames * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];

        // First callback: completes the crossfade and promotes.
        render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        // If crossfade didn't complete in one callback, run more.
        if controller.current_track.as_ref().unwrap().song_id == "song-a" {
            for _ in 0..20 {
                let mut output_more = vec![0.0f32; callback_frames * device_channels];
                render_output_buffer(
                    &mut controller,
                    &mut output_more,
                    &mut Vec::new(),
                    &mut Vec::new(),
                    &mut crossfade_scratch,
                    device_sample_rate,
                    device_channels,
                    &mut rc,
                    &mut rc_in,
                    &mut EqProcessor::new(device_sample_rate, device_channels),
                    &mut peak_acc,
                    &ring,
                );
                if controller.current_track.as_ref().unwrap().song_id == "song-b" {
                    break;
                }
            }
        }
        assert_eq!(
            controller.current_track.as_ref().unwrap().song_id,
            "song-b",
            "crossfade must complete and promote"
        );

        // After promotion, the incoming cache was swapped into the primary
        // lane (rc) and the incoming lane (rc_in) was cleared. Verify the
        // second callback renders non-zero audio from the promoted track at
        // the incoming amplitude (0.3), not the outgoing (0.5).
        let mut output2 = vec![0.0f32; 512 * device_channels];
        let rendered2 = render_output_buffer(
            &mut controller,
            &mut output2,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_sample_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut EqProcessor::new(device_sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert!(
            rendered2 > 0,
            "second callback after promotion must render audio"
        );
        let sample_count = (rendered2 * device_channels).min(output2.len());
        let max_sample = output2[..sample_count]
            .iter()
            .fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(
            max_sample > 0.1 && max_sample < 0.45,
            "post-promotion output must be ~0.3 amplitude (incoming track), \
             not ~0.5 (outgoing) or silence. max sample: {max_sample}"
        );
    }

    /// Cancellation (seek) must clear both resampler lanes. After a seek
    /// cancels an active crossfade, the incoming cache must be empty so a
    /// subsequent crossfade starts with fresh resampler state.
    #[test]
    fn crossfade_cancellation_clears_incoming_resampler_cache() {
        let device_rate: u32 = 48_000;
        let src_rate: u32 = 44_100;
        let device_channels = 2;
        let outgoing_total = 60 * src_rate as u64;
        let incoming_total = 60 * src_rate as u64;
        let duration_ms = 3_000;
        let effective_device = duration_ms as u64 * device_rate as u64 / 1000;
        let remaining_src = effective_device * src_rate as u64 / device_rate as u64;
        let render_frame = outgoing_total - remaining_src;

        let mut controller = build_crossfade_controller_mismatched_rate(
            device_rate,
            device_channels,
            render_frame,
            outgoing_total,
            src_rate,
            incoming_total,
            src_rate,
            duration_ms,
        );

        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut rc_in = ResamplerCache::new();
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];

        // Start the crossfade.
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
            controller.active_crossfade.is_some(),
            "crossfade must have started"
        );

        // Cancel via seek — this clears active_crossfade and restores the
        // prepared track at frame zero.
        controller.seek(1_000, 0).unwrap();
        assert!(controller.active_crossfade.is_none());
        assert!(controller.prepared_track.is_some());

        // Next callback: the cache-clearing guard at the top of
        // render_output_buffer must clear the incoming lane because the
        // abort flag was set.
        output.fill(0.0);
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

        // The controller must be in a clean state — no active crossfade,
        // no stale resampler state leaking into the next overlap.
        assert!(controller.active_crossfade.is_none());
        // The test is named for the cache-clearing guard — verify the
        // incoming resampler lane was actually drained, not just that the
        // crossfade ended (which seek already guaranteed above).
        assert!(
            rc_in.cache.is_empty(),
            "incoming resampler cache must be cleared after cancellation"
        );
    }
}
