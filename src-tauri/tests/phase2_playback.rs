use std::path::PathBuf;

mod support;

use openkara_lib::{
    audio::{
        decode,
        playback::{PlaybackController, PlaybackStateSnapshot},
    },
    cache,
    commands::import::import_songs_from_paths,
    library_root::LibraryRoot,
};

fn fixture_path(directory: &str, filename: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(directory)
        .join(filename)
        .display()
        .to_string()
}

fn assert_snapshot(
    snapshot: &PlaybackStateSnapshot,
    expected_song_id: Option<&str>,
    expected_playing: bool,
    expected_position_ms: u64,
) {
    assert_eq!(snapshot.song_id.as_deref(), expected_song_id);
    assert_eq!(snapshot.is_playing, expected_playing);
    assert_eq!(snapshot.position_ms, expected_position_ms);
}

#[test]
fn playback_controller_transitions_pause_seek_and_volume() {
    let decoded =
        decode::decode_file(PathBuf::from(fixture_path("audio", "fixture.wav")).as_path()).unwrap();
    let mut controller = PlaybackController::default();
    assert_eq!(controller.snapshot().volume, 1.0);

    // 44100 Hz, 1 second track
    let started = controller.start_track("song-a".into(), decoded, 1_000);
    assert_snapshot(&started, Some("song-a"), true, 0);

    // Simulate 250ms of playback (250ms * 44100 / 1000 = 11025 frames)
    controller.advance_render_frame(11_025);

    let paused = controller.pause(1_250).expect("pause should succeed");
    assert_snapshot(&paused, Some("song-a"), false, 250);

    // Resume — render_frame unchanged, position stays at 250ms
    let resumed = controller.play(1_500).expect("resume should succeed");
    assert_snapshot(&resumed, Some("song-a"), true, 250);

    // Seek to 900ms — resets render_frame
    let sought = controller.seek(900, 1_700).expect("seek should succeed");
    assert_snapshot(&sought, Some("song-a"), true, 900);

    let clamped = controller
        .set_volume(1.5)
        .expect("set volume should succeed for loaded track");
    assert_eq!(clamped.volume, 1.0);

    let quiet = controller
        .set_volume(-0.25)
        .expect("volume clamp should allow values below zero");
    assert_eq!(quiet.volume, 0.0);
}

#[test]
fn playback_controller_advances_and_stops_at_track_end() {
    let decoded =
        decode::decode_file(PathBuf::from(fixture_path("audio", "fixture.wav")).as_path()).unwrap();
    let mut controller = PlaybackController::default();

    // 44100 Hz, ~1 second track
    controller.start_track("song-a".into(), decoded, 5_000);

    // Simulate 400ms of playback (400ms * 44100 / 1000 = 17640 frames)
    controller.advance_render_frame(17_640);
    let advanced = controller.snapshot();
    assert_snapshot(&advanced, Some("song-a"), true, 400);

    // Advance past the 1-second duration — should clamp and stop
    controller.advance_render_frame(44_100);
    let ended = controller.snapshot();
    assert!(ended.duration_ms.is_some());
    assert_eq!(
        ended.position_ms,
        ended.duration_ms.expect("duration should exist")
    );
    assert!(!ended.is_playing);
}

#[test]
fn track_load_starts_an_imported_song_by_hash() {
    let tmp = tempfile::tempdir().expect("temp dir should create");
    let library =
        LibraryRoot::create(tmp.path().join("lib").as_path()).expect("library should create");
    cache::initialize_library_database(&library.database_path())
        .expect("library database should initialize");
    let connection =
        cache::open_database(&library.database_path()).expect("library database should open");

    let import_result = import_songs_from_paths(
        &connection,
        &library,
        &[fixture_path("metadata", "fixture.mp3")],
    );
    assert_eq!(import_result.imported.len(), 1);
    let song_hash = import_result.imported[0].hash.clone();

    let harness = support::PlaybackHarness::new(&library);
    let snapshot = harness
        .play(&song_hash)
        .expect("the track-load path should install the imported song");

    assert_snapshot(&snapshot, Some(song_hash.as_str()), true, 0);
    assert!(snapshot.duration_ms.is_some());
}
