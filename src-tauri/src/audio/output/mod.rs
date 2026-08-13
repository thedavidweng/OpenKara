mod cpal_stream;
mod crossfade_render;
mod mix_bus;
mod resampler_cache;

pub use resampler_cache::ResamplerCache;

use crate::airplay_stream::AirPlayAudioTap;
use crate::audio::decode::DecodedAudio;
use crate::audio::eq::{soft_limit, EqProcessor};
use crate::audio::error::PlaybackError;
use crate::audio::output_format::OutputFormatState;
use crate::audio::peaks::{PeakAccumulator, PeakRing};
use crate::audio::playback::{LoadedStems, PlaybackController};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

use cpal_stream::start_output_thread;
use crossfade_render::render_crossfade_overlap;
use mix_bus::{mix_stem_resampled, render_decoded_mix_bus, render_streaming_mix_bus};

pub fn ensure_output_thread(
    started: &Arc<AtomicBool>,
    start_lock: &Arc<Mutex<()>>,
    playback: Arc<Mutex<PlaybackController>>,
    airplay_audio_tap: Arc<AirPlayAudioTap>,
    airplay_local_output_suppressed: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    peak_ring: Arc<PeakRing>,
    output_format: OutputFormatState,
) -> Result<(), PlaybackError> {
    if started.load(Ordering::SeqCst) {
        return Ok(());
    }

    let _guard = start_lock.lock().map_err(|_| {
        PlaybackError::AudioOutputUnavailable("audio output start lock was poisoned".to_owned())
    })?;
    if started.load(Ordering::SeqCst) {
        return Ok(());
    }

    let (startup_tx, startup_rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        if let Err(error) = start_output_thread(
            playback,
            airplay_audio_tap,
            airplay_local_output_suppressed,
            startup_tx,
            shutdown,
            peak_ring,
            output_format,
        ) {
            eprintln!("audio output thread failed to start: {error:#}");
        }
    });

    startup_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| {
            PlaybackError::AudioOutputUnavailable(
                "timed out while waiting for audio output thread startup".to_owned(),
            )
        })?
        .map_err(|e| PlaybackError::AudioOutputUnavailable(e.to_string()))?;
    started.store(true, Ordering::SeqCst);

    Ok(())
}

pub fn render_output_buffer(
    playback: &mut PlaybackController,
    output: &mut [f32],
    stem_scratch: &mut Vec<f32>,
    mix_scratch: &mut Vec<f32>,
    crossfade_scratch: &mut [f32],
    device_sample_rate: u32,
    device_channels: usize,
    resampler_cache: &mut ResamplerCache,
    crossfade_incoming_resampler_cache: &mut ResamplerCache,
    eq_processor: &mut EqProcessor,
    peak_accumulator: &mut PeakAccumulator,
    peak_ring: &PeakRing,
) -> usize {
    output.fill(0.0);

    // Streaming: check buffer levels even while is_buffering (snapshot may
    // report is_playing=false); otherwise recovery never clears the flag.
    if playback
        .current_track
        .as_ref()
        .is_some_and(|t| t.streaming.is_some())
    {
        let track = playback.current_track.as_mut().unwrap();
        let streaming = track.streaming.as_mut().unwrap();

        streaming.acknowledge_flush_if_needed();

        let below_low = streaming.any_consumer_below_low_water();
        let all_above_high = streaming.all_consumers_above_high_water();

        if below_low {
            playback.is_buffering = true;
            if !matches!(
                playback.fade,
                crate::audio::playback::FadeState::FadingAfterSeek { .. }
            ) {
                playback.fade = crate::audio::playback::FadeState::None;
            }
        } else if playback.is_buffering && all_above_high {
            playback.is_buffering = false;
            // FadingAfterSeek uses wall-clock Instant; reset when buffering
            // clears so the seek-click mask is not already expired.
            if let crate::audio::playback::FadeState::FadingAfterSeek { .. } = playback.fade {
                playback.fade = crate::audio::playback::FadeState::FadingAfterSeek {
                    start: std::time::Instant::now(),
                };
            }
        }
    }

    playback.finalize_fade_if_complete();

    // Snapshot after buffer-level update so is_buffering is current.
    let snapshot = playback.snapshot();
    // Underrun: silence until rings recover (snapshot may still say playing).
    if playback.is_buffering {
        return 0;
    }
    // Still render during FadingOut (envelope) and FadingIn (EOF resume must
    // reach the gapless swap; snapshot may report is_playing=false at EOF).
    if !snapshot.is_playing
        && !matches!(
            playback.fade,
            crate::audio::playback::FadeState::FadingOut { .. }
        )
        && !matches!(
            playback.fade,
            crate::audio::playback::FadeState::FadingIn { .. }
        )
    {
        return 0;
    }

    let Some(track) = &playback.current_track else {
        return 0;
    };

    let master = snapshot.volume;
    let sv = snapshot.stem_volumes;
    let render_frame = track.render_frame;
    let has_stems = track.stems.is_some();
    let has_streaming = track.streaming.is_some();

    // Clear stale incoming sinc state when no crossfade is active, or after
    // seek-abort (prepared_track may be restored while resampler is still stale).
    if playback.crossfade_abort_pending
        || (playback.active_crossfade.is_none() && playback.prepared_track.is_none())
    {
        crossfade_incoming_resampler_cache.clear();
        playback.crossfade_abort_pending = false;
    }

    if !has_streaming
        && !has_stems
        && (playback.active_crossfade.is_some()
            || (playback.crossfade_config.enabled && playback.prepared_track.is_some()))
    {
        let crossfade_result = render_crossfade_overlap(
            playback,
            output,
            crossfade_scratch,
            master,
            device_sample_rate,
            device_channels,
            resampler_cache,
            crossfade_incoming_resampler_cache,
        );
        if let Some((rendered, src_frames_advanced)) = crossfade_result {
            let rendered_samples = rendered * device_channels;

            eq_processor.process(output, rendered_samples);
            if eq_processor.is_fully_bypassed() {
                for sample in output[..rendered_samples].iter_mut() {
                    *sample = sample.clamp(-1.0, 1.0);
                }
            } else {
                for sample in output[..rendered_samples].iter_mut() {
                    *sample = soft_limit(*sample);
                }
            }

            let fade_gain = playback.take_fade_gain();
            if let Some(fade_gain) = fade_gain {
                if fade_gain < 1.0 {
                    for sample in output[..rendered_samples].iter_mut() {
                        *sample *= fade_gain;
                    }
                }
            }

            peak_accumulator.process(output, rendered_samples, device_channels, peak_ring);

            playback.advance_render_frame(src_frames_advanced);

            let mut total_rendered = rendered_samples;
            if !has_streaming
                && playback.current_track_reached_eof()
                && playback.current_track_is_playing()
                && playback.perform_gapless_swap()
            {
                let remaining = &mut output[rendered_samples..];
                if !remaining.is_empty() {
                    let track = playback.current_track.as_ref().unwrap();
                    let original = &track.original_audio;
                    let (extra_rendered, extra_frames) = mix_stem_resampled(
                        remaining,
                        original,
                        0,
                        master,
                        device_sample_rate,
                        device_channels,
                        Some(resampler_cache),
                    );
                    eq_processor.process(remaining, extra_rendered);
                    if eq_processor.is_fully_bypassed() {
                        for sample in remaining[..extra_rendered].iter_mut() {
                            *sample = sample.clamp(-1.0, 1.0);
                        }
                    } else {
                        for sample in remaining[..extra_rendered].iter_mut() {
                            *sample = soft_limit(*sample);
                        }
                    }
                    // Keep this callback's fade gain on post-swap samples
                    // (EOF resume), mirroring the non-crossfade path below.
                    if let Some(fade_gain) = fade_gain {
                        if fade_gain < 1.0 {
                            for sample in remaining[..extra_rendered].iter_mut() {
                                *sample *= fade_gain;
                            }
                        }
                    }
                    peak_accumulator.process(remaining, extra_rendered, device_channels, peak_ring);
                    playback.advance_render_frame(extra_frames);
                    total_rendered += extra_rendered;
                }
            }

            return total_rendered;
        }
    }

    let (rendered, src_frames_advanced) = if has_streaming {
        let track = playback.current_track.as_mut().unwrap();
        let streaming = track.streaming.as_mut().unwrap();

        match streaming {
            crate::audio::streaming::StreamingTrack::Single { consumer } => {
                let gains = [master];
                let mut consumers: [&mut crate::audio::streaming::AudioConsumer; 1] = [consumer];
                render_streaming_mix_bus(
                    output,
                    &mut consumers,
                    &gains,
                    stem_scratch,
                    mix_scratch,
                    device_sample_rate,
                    device_channels,
                    resampler_cache,
                )
            }
            crate::audio::streaming::StreamingTrack::TwoStem {
                vocals,
                accompaniment,
            } => {
                // Legacy two-stem: map drums/bass/other knobs onto one accomp PCM.
                let accomp_gain = sv.drums.max(sv.bass).max(sv.other);
                let gains = [master * sv.vocals, master * accomp_gain];
                let mut consumers: [&mut crate::audio::streaming::AudioConsumer; 2] =
                    [vocals, accompaniment];
                render_streaming_mix_bus(
                    output,
                    &mut consumers,
                    &gains,
                    stem_scratch,
                    mix_scratch,
                    device_sample_rate,
                    device_channels,
                    resampler_cache,
                )
            }
            crate::audio::streaming::StreamingTrack::FourStem {
                vocals,
                drums,
                bass,
                other,
            } => {
                let gains = [
                    master * sv.vocals,
                    master * sv.drums,
                    master * sv.bass,
                    master * sv.other,
                ];
                let mut consumers: [&mut crate::audio::streaming::AudioConsumer; 4] =
                    [vocals, drums, bass, other];
                render_streaming_mix_bus(
                    output,
                    &mut consumers,
                    &gains,
                    stem_scratch,
                    mix_scratch,
                    device_sample_rate,
                    device_channels,
                    resampler_cache,
                )
            }
        }
    } else if has_stems {
        let track = playback.current_track.as_ref().unwrap();
        let loaded_stems = track.stems.as_ref().unwrap();
        match loaded_stems {
            LoadedStems::TwoStem {
                vocals,
                accompaniment,
            } => {
                let accomp_gain = sv.drums.max(sv.bass).max(sv.other);
                let gains = [master * sv.vocals, master * accomp_gain];
                let stems: [&DecodedAudio; 2] = [vocals, accompaniment];
                render_decoded_mix_bus(
                    output,
                    &stems,
                    &gains,
                    render_frame,
                    mix_scratch,
                    device_sample_rate,
                    device_channels,
                    resampler_cache,
                )
            }
            LoadedStems::FourStem(stems) => {
                let gains = [
                    master * sv.vocals,
                    master * sv.drums,
                    master * sv.bass,
                    master * sv.other,
                ];
                let stems: [&DecodedAudio; 4] =
                    [&stems.vocals, &stems.drums, &stems.bass, &stems.other];
                render_decoded_mix_bus(
                    output,
                    &stems,
                    &gains,
                    render_frame,
                    mix_scratch,
                    device_sample_rate,
                    device_channels,
                    resampler_cache,
                )
            }
        }
    } else {
        let track = playback.current_track.as_ref().unwrap();
        let original = &track.original_audio;
        mix_stem_resampled(
            output,
            original,
            render_frame,
            master,
            device_sample_rate,
            device_channels,
            Some(resampler_cache),
        )
    };

    eq_processor.process(output, rendered);

    if eq_processor.is_fully_bypassed() {
        for sample in output[..rendered].iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
    } else {
        for sample in output[..rendered].iter_mut() {
            *sample = soft_limit(*sample);
        }
    }

    let fade_gain = playback.take_fade_gain();
    if let Some(fade_gain) = fade_gain {
        if fade_gain < 1.0 {
            for sample in output[..rendered].iter_mut() {
                *sample *= fade_gain;
            }
        }
    }

    // `rendered` is already interleaved sample count — do not × device_channels.
    peak_accumulator.process(output, rendered, device_channels, peak_ring);

    playback.advance_render_frame(src_frames_advanced);

    if has_streaming {
        finalize_streaming_natural_end(playback);
    }

    let mut total_rendered = rendered;
    // Skip gapless swap while paused / FadingOut (EOF during pause must not advance).
    if !has_streaming
        && playback.current_track_reached_eof()
        && playback.current_track_is_playing()
        && playback.perform_gapless_swap()
    {
        let remaining = &mut output[rendered..];
        if !remaining.is_empty() {
            let track = playback.current_track.as_ref().unwrap();
            let original = &track.original_audio;
            let (extra_rendered, extra_frames) = mix_stem_resampled(
                remaining,
                original,
                0,
                master,
                device_sample_rate,
                device_channels,
                Some(resampler_cache),
            );
            eq_processor.process(remaining, extra_rendered);
            if eq_processor.is_fully_bypassed() {
                for sample in remaining[..extra_rendered].iter_mut() {
                    *sample = sample.clamp(-1.0, 1.0);
                }
            } else {
                for sample in remaining[..extra_rendered].iter_mut() {
                    *sample = soft_limit(*sample);
                }
            }
            // Keep this callback's fade gain on post-swap samples (EOF resume).
            if let Some(fade_gain) = fade_gain {
                if fade_gain < 1.0 {
                    for sample in remaining[..extra_rendered].iter_mut() {
                        *sample *= fade_gain;
                    }
                }
            }
            peak_accumulator.process(remaining, extra_rendered, device_channels, peak_ring);
            playback.advance_render_frame(extra_frames);
            total_rendered += extra_rendered;
        }
    }

    total_rendered
}

fn finalize_streaming_natural_end(playback: &mut PlaybackController) {
    playback.finalize_streaming_natural_end();
}

#[cfg(test)]
mod tests {
    use super::{render_output_buffer, ResamplerCache};
    use crate::audio::crossfade::CROSSFADE_SCRATCH_FRAMES;
    use crate::audio::eq::EqProcessor;

    #[test]
    fn bypassed_eq_hard_clamps_multistem_summation() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::playback::{FadeState, LoadedStems, PlaybackController};

        let sample_rate = 44_100;
        let channels = 2;
        let frames = 128;
        let decoded = |sample| DecodedAudio {
            sample_rate_hz: sample_rate,
            channels,
            duration_ms: (frames * 1_000 / sample_rate as usize) as u64,
            samples: vec![sample; frames * channels],
        };

        let mut controller = PlaybackController::default();
        controller.start_track("song-a".to_owned(), decoded(0.0), 0);
        controller
            .attach_stems(
                "song-a",
                LoadedStems::TwoStem {
                    vocals: decoded(0.75),
                    accompaniment: decoded(0.75),
                },
            )
            .expect("stems should attach to the active track");
        controller.play(0).expect("track should start");
        controller.fade = FadeState::None;

        let mut output = vec![0.0_f32; frames * channels];
        let mut resampler_cache = ResamplerCache::new();
        let mut crossfade_incoming_rc = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * channels];
        let mut eq = EqProcessor::new(sample_rate, channels);
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_accumulator = crate::audio::peaks::PeakAccumulator::new();
        let rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            sample_rate,
            channels,
            &mut resampler_cache,
            &mut crossfade_incoming_rc,
            &mut eq,
            &mut peak_accumulator,
            &ring,
        );

        assert_eq!(rendered, frames * channels);
        assert!(
            output[..rendered]
                .iter()
                .all(|sample| (-1.0..=1.0).contains(sample)),
            "EQ bypass must still keep mixed output in the PCM range",
        );
        assert!(
            output[..rendered]
                .iter()
                .any(|sample| (*sample - 1.0).abs() < f32::EPSILON),
            "two 0.75 stems should clamp to +1.0 rather than wrap or attenuate",
        );
    }

    /// #378: a track that slips through with a zero sample rate must degrade
    /// to silence on the realtime callback instead of panicking inside the
    /// rubato resampler construction.
    #[test]
    fn zero_sample_rate_track_renders_silence_without_panicking() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::playback::{FadeState, PlaybackController};

        let device_rate: u32 = 48_000;
        let device_channels: usize = 2;

        let mut controller = PlaybackController::default();
        controller.start_track(
            "corrupt-song".to_owned(),
            DecodedAudio {
                sample_rate_hz: 0,
                channels: device_channels,
                duration_ms: 1_000,
                samples: vec![0.5; 4_410 * device_channels],
            },
            0,
        );
        controller.play(0).unwrap();
        controller.fade = FadeState::None;

        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut crossfade_incoming_rc = ResamplerCache::new();
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
            &mut crossfade_incoming_rc,
            &mut EqProcessor::new(device_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        assert_eq!(rendered, 0, "corrupt track must render nothing");
        assert!(
            output.iter().all(|sample| *sample == 0.0),
            "corrupt track must leave the buffer silent"
        );
    }

    #[test]
    fn streaming_buffering_recovers_after_underrun() {
        use crate::audio::playback::PlaybackController;
        use crate::audio::streaming::{self, StreamingTrack};

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        let (mut prod, consumer) = streaming::create_stream_pair(sample_rate, channels);

        let mut controller = PlaybackController::default();
        controller.start_track_streaming(
            "test-recovery".to_owned(),
            sample_rate,
            channels,
            30_000, // 30-second track
            StreamingTrack::Single { consumer },
            0,
        );

        // 1st callback: buffer is empty → below low water → is_buffering = true.
        let device_channels = 2;
        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut crossfade_incoming_rc = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let mut eq = crate::audio::eq::EqProcessor::new(sample_rate, device_channels);
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
            &mut crossfade_incoming_rc,
            &mut eq,
            &mut peak_acc,
            &ring,
        );
        assert_eq!(rendered, 0);
        assert!(
            controller.is_buffering,
            "should enter buffering after empty underrun"
        );

        // 2nd callback: is_buffering is true → snapshot reports transport intent
        // (is_playing stays true) but render_output_buffer still outputs silence.
        output.fill(0.0);
        let rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            sample_rate,
            device_channels,
            &mut rc,
            &mut crossfade_incoming_rc,
            &mut eq,
            &mut peak_acc,
            &ring,
        );
        assert_eq!(rendered, 0);

        // Simulate the decode thread filling the buffer past the high-water mark.
        let high_water = 88_200usize; // HIGH_WATER_SAMPLES
        let filler = vec![0.5f32; high_water + 1000];
        prod.push_samples(&filler);

        // 3rd callback: buffer is now above high water → is_buffering clears.
        output.fill(0.0);
        let rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            sample_rate,
            device_channels,
            &mut rc,
            &mut crossfade_incoming_rc,
            &mut eq,
            &mut peak_acc,
            &ring,
        );
        assert!(
            !controller.is_buffering,
            "buffering flag should clear once buffer refills"
        );
        assert!(rendered > 0, "should render audio after recovery");
    }

    /// R5: Position must not advance while any required stem is unavailable
    /// (below low water). The all-or-nothing buffering policy ensures this.
    #[test]
    fn streaming_two_stem_enters_buffering_when_one_stem_below_low_water() {
        use crate::audio::playback::PlaybackController;
        use crate::audio::streaming::{self, StreamingTrack};

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;

        let (mut prod_v, consumer_v) = streaming::create_stream_pair(sample_rate, channels);
        let (_prod_a, consumer_a) = streaming::create_stream_pair(sample_rate, channels);

        // Fill vocals generously but leave accompaniment empty (below low water).
        let filler = vec![0.5_f32; sample_rate as usize * channels];
        prod_v.push_samples(&filler);

        let mut controller = PlaybackController::default();
        controller.start_track_streaming(
            "test-r5".to_owned(),
            sample_rate,
            channels,
            30_000,
            StreamingTrack::TwoStem {
                vocals: consumer_v,
                accompaniment: consumer_a,
            },
            0,
        );

        let device_channels = 2;
        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut crossfade_incoming_rc = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let mut eq = crate::audio::eq::EqProcessor::new(sample_rate, device_channels);
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
            &mut crossfade_incoming_rc,
            &mut eq,
            &mut peak_acc,
            &ring,
        );

        // Accompaniment is empty → below low water → must enter buffering.
        assert!(
            controller.is_buffering,
            "must enter buffering when a required stem is below low water"
        );
        assert_eq!(rendered, 0, "must render silence during buffering");

        let snapshot = controller.snapshot();
        assert_eq!(
            snapshot.position_ms, 0,
            "position must not advance during buffering"
        );
    }

    #[test]
    fn gapless_swap_fills_remaining_buffer_from_next_track() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::output_format::OutputFormatSnapshot;
        use crate::audio::playback::{PlaybackController, PreparedTrack};

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        let device_channels = 2;

        // Track A: 100 frames of 0.1, then EOF. Buffer is 512 frames, so
        // track A ends mid-callback and the remaining 412 frames should be
        // filled from track B.
        let track_a_samples = vec![0.1_f32; 100 * channels];
        let track_a = DecodedAudio {
            sample_rate_hz: sample_rate,
            channels,
            duration_ms: (100 * 1000 / sample_rate as usize) as u64,
            samples: track_a_samples,
        };

        let mut controller = PlaybackController::default();
        controller.start_track("song-a".to_owned(), track_a, 0);
        controller.play(0).unwrap();
        // Clear the fade-in so the test measures raw sample values.
        controller.fade = crate::audio::playback::FadeState::None;

        // Prepare track B: 512 frames of 0.5, distinguishable from track A.
        let fmt = OutputFormatSnapshot::new(1, sample_rate, channels as u16);
        let track_b_samples = vec![0.5_f32; 512 * channels];
        let prepared = PreparedTrack {
            preload_request_generation: crate::audio::playback::PreloadRequestGeneration(0),
            preload_generation: fmt.generation,
            song_id: "song-b".to_owned(),
            output_format: fmt,
            audio: DecodedAudio {
                sample_rate_hz: sample_rate,
                channels,
                duration_ms: (512 * 1000 / sample_rate as usize) as u64,
                samples: track_b_samples,
            },
        };
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());

        // Render one buffer. Track A fills the first 100 frames; the gapless
        // swap should fill the remaining 412 frames from track B.
        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut crossfade_incoming_rc = ResamplerCache::new();
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
            &mut crossfade_incoming_rc,
            &mut EqProcessor::new(sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        // The entire buffer should be filled — no silence gap.
        assert_eq!(
            rendered,
            512 * device_channels,
            "gapless swap should fill the entire buffer"
        );

        // First 100 frames are from track A (0.1).
        for (i, sample) in output.iter().enumerate().take(100 * device_channels) {
            assert!(
                (*sample - 0.1).abs() < 1e-6,
                "frame {i} should be from track A: got {sample}"
            );
        }

        // Remaining 412 frames are from track B (0.5), not zeros.
        for (i, sample) in output
            .iter()
            .enumerate()
            .take(512 * device_channels)
            .skip(100 * device_channels)
        {
            assert!(
                (*sample - 0.5).abs() < 1e-6,
                "frame {i} should be from track B: got {sample}"
            );
        }

        // The new track's render frame should have advanced by 412.
        let track = controller.current_track.as_ref().unwrap();
        assert_eq!(track.song_id, "song-b");
        assert_eq!(track.render_frame, 412);
    }

    /// #375: when the gapless swap fires inside the crossfade branch, the
    /// samples rendered after the swap must still receive this callback's
    /// fade gain — otherwise the level steps from `fade_gain` to full
    /// amplitude in the middle of one buffer.
    #[test]
    fn crossfade_branch_gapless_swap_keeps_fade_gain_on_post_swap_samples() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::output_format::OutputFormatSnapshot;
        use crate::audio::playback::{
            ActiveCrossfade, PlaybackController, PreloadRequestGeneration, PreparedTrack,
        };

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        let device_channels = 2;

        // Track A (outgoing): already at EOF when the callback starts.
        let track_a_frames = 1_000usize;
        let mut controller = PlaybackController::default();
        controller.start_track(
            "song-a".to_owned(),
            DecodedAudio {
                sample_rate_hz: sample_rate,
                channels,
                duration_ms: (track_a_frames * 1_000 / sample_rate as usize) as u64,
                samples: vec![0.4; track_a_frames * channels],
            },
            0,
        );
        controller.play(0).unwrap();
        controller.current_track.as_mut().unwrap().render_frame = track_a_frames as u64;

        let fmt = OutputFormatSnapshot::new(1, sample_rate, channels as u16);
        let make_prepared = |song_id: &str, frames: usize, amplitude: f32| PreparedTrack {
            preload_request_generation: PreloadRequestGeneration(0),
            preload_generation: fmt.generation,
            song_id: song_id.to_owned(),
            output_format: fmt,
            audio: DecodedAudio {
                sample_rate_hz: sample_rate,
                channels,
                duration_ms: (frames * 1_000 / sample_rate as usize) as u64,
                samples: vec![amplitude; frames * channels],
            },
        };

        // A crossfade into song-b is active while a fresh prepared track
        // (song-c) has already been installed behind it.
        controller.active_crossfade = Some(ActiveCrossfade {
            prepared: make_prepared("song-b", 88_200, 0.2),
            total_frames: 22_050,
            rendered_frames: 0,
            incoming_source_frame: 0,
        });
        assert!(controller
            .install_prepared_track(make_prepared("song-c", 44_100, 0.5), fmt)
            .is_ok());

        // A resume fade is in progress; its gain is ~0 right after start.
        controller.fade = crate::audio::playback::FadeState::FadingIn {
            start: std::time::Instant::now(),
        };

        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut crossfade_incoming_rc = ResamplerCache::new();
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
            &mut crossfade_incoming_rc,
            &mut EqProcessor::new(sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        // The outgoing track was at EOF, so the swap must have fired.
        assert_eq!(
            controller.current_track.as_ref().unwrap().song_id,
            "song-c",
            "gapless swap must fire for the EOF outgoing track"
        );

        // Post-swap samples come from song-c (0.5 amplitude) and must be
        // scaled by the active fade gain instead of jumping to full level.
        let max_sample = output.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(
            max_sample < 0.49,
            "post-swap samples must keep this callback's fade gain, got {max_sample}"
        );
    }

    #[test]
    fn gapless_swap_does_not_advance_when_paused_at_eof() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::output_format::OutputFormatSnapshot;
        use crate::audio::playback::{PlaybackController, PreparedTrack};

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        let device_channels = 2;

        // Track A: 100 frames of 0.1, then EOF.
        let track_a_samples = vec![0.1_f32; 100 * channels];
        let track_a = DecodedAudio {
            sample_rate_hz: sample_rate,
            channels,
            duration_ms: (100 * 1000 / sample_rate as usize) as u64,
            samples: track_a_samples,
        };

        let mut controller = PlaybackController::default();
        controller.start_track("song-a".to_owned(), track_a, 0);
        controller.play(0).unwrap();
        // Clear the fade-in so the test measures raw sample values.
        controller.fade = crate::audio::playback::FadeState::None;

        // Prepare track B so a gapless swap would be possible if not paused.
        let fmt = OutputFormatSnapshot::new(1, sample_rate, channels as u16);
        let track_b_samples = vec![0.5_f32; 512 * channels];
        let prepared = PreparedTrack {
            preload_request_generation: crate::audio::playback::PreloadRequestGeneration(0),
            preload_generation: fmt.generation,
            song_id: "song-b".to_owned(),
            output_format: fmt,
            audio: DecodedAudio {
                sample_rate_hz: sample_rate,
                channels,
                duration_ms: (512 * 1000 / sample_rate as usize) as u64,
                samples: track_b_samples,
            },
        };
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());

        // Simulate a user pause: set FadingOut. The render callback still
        // runs during the fade-out (to ramp the volume down), so
        // render_frame will advance and reach EOF — but the gapless swap
        // must not fire because the user intended to pause.
        controller.pause(0).unwrap();
        assert!(matches!(
            controller.fade,
            crate::audio::playback::FadeState::FadingOut { .. }
        ));

        // Render one buffer. Track A fills the first 100 frames; the
        // remaining 412 frames should be silence (zeros) because the
        // gapless swap is suppressed by the pause.
        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut crossfade_incoming_rc = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let _rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            sample_rate,
            device_channels,
            &mut rc,
            &mut crossfade_incoming_rc,
            &mut EqProcessor::new(sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        // The current track must still be song-a, not swapped to song-b.
        let track = controller.current_track.as_ref().unwrap();
        assert_eq!(
            track.song_id, "song-a",
            "gapless swap must not fire during a pause"
        );
        // The prepared track must still be available for a future resume.
        assert!(
            controller.prepared_track.is_some(),
            "prepared track must not be consumed during a pause"
        );

        // No transition must be queued — the swap was suppressed, so
        // the position emitter must not emit a `track-transitioned` event.
        assert!(
            controller.pending_transition_out.is_none(),
            "no CompletedTransition must be queued while paused at EOF"
        );

        // The tail (after track A's 100 frames) must contain no
        // samples from track B (0.5 amplitude). It should be silence.
        let tail_start = 100 * device_channels;
        let tail = &output[tail_start..];
        let max_tail = tail.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(
            max_tail < 1e-6,
            "tail after EOF during pause must be silence (no track B samples), \
             max amplitude {max_tail}"
        );
    }

    #[test]
    fn gapless_swap_proceeds_after_resume_from_pause_at_eof() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::output_format::OutputFormatSnapshot;
        use crate::audio::playback::{PlaybackController, PreparedTrack};

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        let device_channels = 2;

        // Track A: 100 frames of 0.1, then EOF.
        let track_a_samples = vec![0.1_f32; 100 * channels];
        let track_a = DecodedAudio {
            sample_rate_hz: sample_rate,
            channels,
            duration_ms: (100 * 1000 / sample_rate as usize) as u64,
            samples: track_a_samples,
        };

        let mut controller = PlaybackController::default();
        controller.start_track("song-a".to_owned(), track_a, 0);
        controller.play(0).unwrap();
        // Clear the fade-in so the test measures raw sample values.
        controller.fade = crate::audio::playback::FadeState::None;

        // Prepare track B (0.5 amplitude) so a gapless swap is possible.
        let fmt = OutputFormatSnapshot::new(1, sample_rate, channels as u16);
        let track_b_samples = vec![0.5_f32; 512 * channels];
        let prepared = PreparedTrack {
            preload_request_generation: crate::audio::playback::PreloadRequestGeneration(0),
            preload_generation: fmt.generation,
            song_id: "song-b".to_owned(),
            output_format: fmt,
            audio: DecodedAudio {
                sample_rate_hz: sample_rate,
                channels,
                duration_ms: (512 * 1000 / sample_rate as usize) as u64,
                samples: track_b_samples,
            },
        };
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());

        // Pause near EOF — the gapless swap is suppressed during the
        // fade-out. Render one buffer to advance render_frame to EOF.
        controller.pause(0).unwrap();
        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut crossfade_incoming_rc = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let _rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            sample_rate,
            device_channels,
            &mut rc,
            &mut crossfade_incoming_rc,
            &mut EqProcessor::new(sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        // Track A should still be current — swap was suppressed.
        assert_eq!(
            controller.current_track.as_ref().unwrap().song_id,
            "song-a",
            "gapless swap must not fire during pause"
        );
        assert!(
            controller.prepared_track.is_some(),
            "prepared track must survive the pause"
        );

        // The track is now at EOF (render_frame >= total_frames). The
        // fade-out is still in progress (FadingOut) — the render callback
        // calls finalize_fade_if_complete, but FADE_DURATION has not
        // elapsed in the test, so is_playing is still true on the track.
        // However, current_track_is_playing() returns false because of
        // the FadingOut state.

        // User presses resume (play). This sets fade = FadingIn (overriding
        // the FadingOut) and is_playing = true. The helper must now return
        // true because FadingIn is not FadingOut.
        controller.play(0).unwrap();
        assert!(
            controller.current_track_is_playing(),
            "current_track_is_playing must be true after resume"
        );

        // Reset the start timestamp immediately before rendering so the
        // assertion below exercises an active fade rather than setup time.
        controller.fade = crate::audio::playback::FadeState::FadingIn {
            start: std::time::Instant::now(),
        };

        // Render another buffer. The track is at EOF and the user has
        // resumed, so the gapless swap should now fire and fill the buffer
        // from track B.
        let mut output2 = vec![0.0f32; 512 * device_channels];
        let _rendered2 = render_output_buffer(
            &mut controller,
            &mut output2,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            sample_rate,
            device_channels,
            &mut rc,
            &mut crossfade_incoming_rc,
            &mut EqProcessor::new(sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        // The current track must now be song-b — the swap fired after resume.
        assert_eq!(
            controller.current_track.as_ref().unwrap().song_id,
            "song-b",
            "gapless swap must fire after resume from pause at EOF"
        );
        assert!(
            controller.prepared_track.is_none(),
            "prepared track must be consumed after resume swap"
        );

        // Exactly one transition must have been queued — not zero
        // (the swap fired) and not more than one (no double-swap). Drain it
        // and verify a second drain yields None.
        let transition = controller
            .drain_pending_transition()
            .expect("exactly one CompletedTransition after resume swap");
        assert_eq!(transition.from_song_id, "song-a");
        assert_eq!(transition.to_song_id, "song-b");
        assert!(
            controller.drain_pending_transition().is_none(),
            "must not queue a second transition after resume swap"
        );

        // The output buffer must contain track B's audio in the tail
        // (the swap filled the remaining frames from track B). Track B is
        // 0.5 amplitude, attenuated by the fade-in gain which ramps from
        // ~0 to 1 over FADE_DURATION. Verify the tail is not all silence
        // and that non-silent samples are bounded by track B's amplitude
        // (0.5) — proving the tail fill came from track B, not track A.
        let non_zero = output2.iter().filter(|s| s.abs() > 1e-6).count();
        assert!(
            non_zero > 0,
            "output buffer must contain non-zero samples from track B after resume swap"
        );
        // The fade must survive the gapless swap and be applied to tail audio.
        // Without this regression fix perform_gapless_swap clears FadingIn, so
        // this would be 0.5 exactly despite the deliberate play fade.
        let max_sample = output2.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(
            max_sample < 0.49,
            "tail must remain attenuated by the active resume fade, got {max_sample}"
        );
        assert!(
            matches!(
                controller.fade,
                crate::audio::playback::FadeState::FadingIn { .. }
            ),
            "gapless swap must preserve the active resume fade for later callbacks"
        );
    }

    #[test]
    fn post_pause_tail_is_silence_not_next_track() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::output_format::OutputFormatSnapshot;
        use crate::audio::playback::{PlaybackController, PreparedTrack};

        let sample_rate: u32 = 44_100;
        let channels: usize = 2;
        let device_channels = 2;

        // Track A: 50 frames of 0.3, then EOF. The buffer is 512 frames,
        // so 462 frames should be silence after EOF.
        let track_a_samples = vec![0.3_f32; 50 * channels];
        let track_a = DecodedAudio {
            sample_rate_hz: sample_rate,
            channels,
            duration_ms: (50 * 1000 / sample_rate as usize) as u64,
            samples: track_a_samples,
        };

        let mut controller = PlaybackController::default();
        controller.start_track("song-a".to_owned(), track_a, 0);
        controller.play(0).unwrap();
        // Clear the fade-in so the test measures raw sample values.
        controller.fade = crate::audio::playback::FadeState::None;

        // Prepare track B with a distinct amplitude (0.9) so we can detect
        // if any of its audio leaks into the tail.
        let fmt = OutputFormatSnapshot::new(1, sample_rate, channels as u16);
        let track_b_samples = vec![0.9_f32; 512 * channels];
        let prepared = PreparedTrack {
            preload_request_generation: crate::audio::playback::PreloadRequestGeneration(0),
            preload_generation: fmt.generation,
            song_id: "song-b".to_owned(),
            output_format: fmt,
            audio: DecodedAudio {
                sample_rate_hz: sample_rate,
                channels,
                duration_ms: (512 * 1000 / sample_rate as usize) as u64,
                samples: track_b_samples,
            },
        };
        assert!(controller.install_prepared_track(prepared, fmt).is_ok());

        // Pause — the gapless swap is suppressed.
        controller.pause(0).unwrap();

        // Render one buffer. Track A fills the first 50 frames (attenuated
        // by the fade-out gain); the remaining 462 frames must be silence.
        let mut output = vec![0.0f32; 512 * device_channels];
        let mut rc = ResamplerCache::new();
        let mut crossfade_incoming_rc = ResamplerCache::new();
        let mut crossfade_scratch = vec![0.0f32; CROSSFADE_SCRATCH_FRAMES * device_channels];
        let ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let _rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            sample_rate,
            device_channels,
            &mut rc,
            &mut crossfade_incoming_rc,
            &mut EqProcessor::new(sample_rate, device_channels),
            &mut peak_acc,
            &ring,
        );

        // The tail (after track A's 50 frames) must be all zeros — not
        // track B's 0.9 amplitude. The fade-out gain attenuates track A's
        // samples, but the tail must be pure silence.
        let tail_start = 50 * device_channels;
        let tail = &output[tail_start..];
        let max_tail = tail.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(
            max_tail < 1e-6,
            "tail after EOF during pause must be silence, max amplitude {max_tail}"
        );

        // Track A must still be current — no swap.
        assert_eq!(
            controller.current_track.as_ref().unwrap().song_id,
            "song-a",
            "no gapless swap during pause"
        );
    }

    #[test]
    fn attach_stems_rejects_mismatched_sample_rate() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::playback::{LoadedStems, PlaybackController};

        let channels = 2;
        let frames = 128;
        let decoded = |sr, sample| DecodedAudio {
            sample_rate_hz: sr,
            channels,
            duration_ms: (frames * 1000 / sr as usize) as u64,
            samples: vec![sample; frames * channels],
        };

        let mut controller = PlaybackController::default();
        controller.start_track("song-a".to_owned(), decoded(44_100, 0.0), 0);
        let result = controller.attach_stems(
            "song-a",
            LoadedStems::TwoStem {
                vocals: decoded(44_100, 0.5),
                accompaniment: decoded(48_000, 0.3), // mismatched rate
            },
        );
        assert!(
            result.is_err(),
            "attach_stems must reject mismatched sample rates"
        );
    }

    #[test]
    fn attach_stems_rejects_mismatched_frame_count() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::playback::{LoadedStems, PlaybackController};

        let sample_rate = 44_100;
        let channels = 2;
        let decoded = |frames, sample| DecodedAudio {
            sample_rate_hz: sample_rate,
            channels,
            duration_ms: (frames * 1000 / sample_rate as usize) as u64,
            samples: vec![sample; frames * channels],
        };

        let mut controller = PlaybackController::default();
        controller.start_track("song-a".to_owned(), decoded(128, 0.0), 0);
        let result = controller.attach_stems(
            "song-a",
            LoadedStems::TwoStem {
                vocals: decoded(128, 0.5),
                accompaniment: decoded(256, 0.3), // mismatched frame count
            },
        );
        assert!(
            result.is_err(),
            "attach_stems must reject mismatched frame counts"
        );
    }

    #[test]
    fn attach_stems_accepts_matching_metadata() {
        use crate::audio::decode::DecodedAudio;
        use crate::audio::playback::{LoadedStems, PlaybackController};

        let sample_rate = 44_100;
        let channels = 2;
        let frames = 128;
        let decoded = |sample| DecodedAudio {
            sample_rate_hz: sample_rate,
            channels,
            duration_ms: (frames * 1000 / sample_rate as usize) as u64,
            samples: vec![sample; frames * channels],
        };

        let mut controller = PlaybackController::default();
        controller.start_track("song-a".to_owned(), decoded(0.0), 0);
        let result = controller.attach_stems(
            "song-a",
            LoadedStems::TwoStem {
                vocals: decoded(0.5),
                accompaniment: decoded(0.3),
            },
        );
        assert!(
            result.is_ok(),
            "attach_stems must accept stems with matching metadata"
        );
    }
}
