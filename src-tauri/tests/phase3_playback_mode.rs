use openkara_lib::audio::{
    decode::DecodedAudio,
    output::render_output_buffer,
    playback::{PlaybackController, PlaybackMode},
};

fn decoded_audio(samples: Vec<f32>) -> DecodedAudio {
    DecodedAudio {
        sample_rate: 44_100,
        channels: 2,
        duration_ms: 1,
        samples,
    }
}

#[test]
fn render_output_uses_karaoke_track_after_mode_switch() {
    let mut controller = PlaybackController::default();
    controller.start_track(
        "song-a".into(),
        decoded_audio(vec![0.8, 0.7, -0.8, -0.7]),
        0,
    );
    controller
        .attach_karaoke_track("song-a", decoded_audio(vec![0.2, 0.1, -0.2, -0.1]))
        .expect("karaoke track should attach to the current song");
    controller
        .set_mode(PlaybackMode::Karaoke)
        .expect("mode switch should succeed");

    let snapshot = controller.snapshot(0);
    assert_eq!(snapshot.mode, PlaybackMode::Karaoke);

    let mut output = vec![0.0; 4];
    let rendered_samples = render_output_buffer(&mut controller, 0, &mut output);

    assert_eq!(rendered_samples, 4);
    assert_eq!(output, vec![0.2, 0.1, -0.2, -0.1]);
}
