use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

mod support;

use openkara_lib::audio::encode::StreamingOggWriter;
use openkara_lib::{
    audio::decode,
    config::{ExecutionProviderPreference, StemMode},
    separator::{
        inference::{self, StemWriters},
        model, preprocess,
        workspace::SeparationWorkspace,
    },
};

fn fixture_path(directory: &str, filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(directory)
        .join(filename)
}

fn unique_output_dir() -> PathBuf {
    support::unique_temp_path("phase3-inference")
}

fn cleanup_dir(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).expect("temporary output directory should be removable");
    }
}

fn model_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("models")
        .join("htdemucs.onnx")
}

fn initialize_test_runtime() {
    let runtime_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("generated")
        .join("onnxruntime")
        .join(model::ORT_RUNTIME_FILENAME);
    model::ensure_runtime_loaded_from_path(&runtime_path)
        .expect("CI-prepared runtime should initialize explicitly for inference tests");
}

/// Run streaming FourStem separation on the given audio and return the
/// output directory with the four stem OGG files.
fn separate_four_stem_to_dir(
    decoded: openkara_lib::audio::decode::DecodedAudio,
    output_dir: &Path,
) -> openkara_lib::audio::decode::DecodedAudio {
    initialize_test_runtime();
    let loaded_model = model::load_from_path(&model_path(), ExecutionProviderPreference::Cpu)
        .expect("demucs model should load");

    let normalized =
        preprocess::normalize_audio_for_model(decoded).expect("audio should normalize for model");

    let channels = normalized.channels;
    let input_frame_count = normalized.samples.len() / channels;
    let chunk_size =
        preprocess::target_frame_count(&loaded_model, input_frame_count).expect("chunk size");
    let hop_size = chunk_size / 2;

    fs::create_dir_all(output_dir).expect("output dir should be created");

    let sample_rate = normalized.sample_rate;
    let vocals_path = output_dir.join("vocals.ogg");
    let drums_path = output_dir.join("drums.ogg");
    let bass_path = output_dir.join("bass.ogg");
    let other_path = output_dir.join("other.ogg");

    let mut writers = StemWriters {
        mode: StemMode::FourStem,
        vocals: StreamingOggWriter::new(&vocals_path, sample_rate, channels, None, None)
            .expect("vocals writer"),
        accompaniment: None,
        drums: Some(
            StreamingOggWriter::new(&drums_path, sample_rate, channels, None, None)
                .expect("drums writer"),
        ),
        bass: Some(
            StreamingOggWriter::new(&bass_path, sample_rate, channels, None, None)
                .expect("bass writer"),
        ),
        other: Some(
            StreamingOggWriter::new(&other_path, sample_rate, channels, None, None)
                .expect("other writer"),
        ),
    };

    let mut workspace = SeparationWorkspace::new(
        StemMode::FourStem,
        channels,
        chunk_size,
        hop_size,
        input_frame_count,
    );

    let _outcome = inference::separate_streaming(
        &loaded_model,
        &normalized,
        StemMode::FourStem,
        &mut writers,
        &mut workspace,
        &AtomicBool::new(false),
        |_, _| {},
    )
    .expect("streaming separation should succeed");

    writers.finish_all().expect("writers should finalize");

    normalized
}

/// Run a cancellable FourStem streaming pass. The writers are intentionally
/// NOT finalized, so on the cancel path they are dropped and their temp files
/// cleaned up — no final stem files should ever be promoted.
fn run_streaming_with_cancel(
    decoded: openkara_lib::audio::decode::DecodedAudio,
    output_dir: &Path,
    cancel: &AtomicBool,
    on_chunk: impl FnMut(usize, usize),
) -> anyhow::Result<()> {
    initialize_test_runtime();
    let loaded_model = model::load_from_path(&model_path(), ExecutionProviderPreference::Cpu)
        .expect("demucs model should load");

    let normalized =
        preprocess::normalize_audio_for_model(decoded).expect("audio should normalize for model");

    let channels = normalized.channels;
    let input_frame_count = normalized.samples.len() / channels;
    let chunk_size =
        preprocess::target_frame_count(&loaded_model, input_frame_count).expect("chunk size");
    let hop_size = chunk_size / 2;

    fs::create_dir_all(output_dir).expect("output dir should be created");

    let sample_rate = normalized.sample_rate;
    let mut writers = StemWriters {
        mode: StemMode::FourStem,
        vocals: StreamingOggWriter::new(
            &output_dir.join("vocals.ogg"),
            sample_rate,
            channels,
            None,
            None,
        )
        .expect("vocals writer"),
        accompaniment: None,
        drums: Some(
            StreamingOggWriter::new(
                &output_dir.join("drums.ogg"),
                sample_rate,
                channels,
                None,
                None,
            )
            .expect("drums writer"),
        ),
        bass: Some(
            StreamingOggWriter::new(
                &output_dir.join("bass.ogg"),
                sample_rate,
                channels,
                None,
                None,
            )
            .expect("bass writer"),
        ),
        other: Some(
            StreamingOggWriter::new(
                &output_dir.join("other.ogg"),
                sample_rate,
                channels,
                None,
                None,
            )
            .expect("other writer"),
        ),
    };

    let mut workspace = SeparationWorkspace::new(
        StemMode::FourStem,
        channels,
        chunk_size,
        hop_size,
        input_frame_count,
    );

    inference::separate_streaming(
        &loaded_model,
        &normalized,
        StemMode::FourStem,
        &mut writers,
        &mut workspace,
        cancel,
        on_chunk,
    )
    .map(|_| ())
}

fn assert_no_stem_files(output_dir: &Path) {
    for stem_name in ["vocals", "drums", "bass", "other"] {
        let stem_path = output_dir.join(format!("{stem_name}.ogg"));
        assert!(
            !stem_path.exists(),
            "{} must not exist after a cancelled run",
            stem_path.display()
        );
    }
}

#[test]
fn streaming_separation_cancelled_before_first_chunk_writes_no_stems() {
    let decoded = decode::decode_file(&fixture_path("audio", "fixture.wav"))
        .expect("wav fixture should decode");

    let output_dir = unique_output_dir();
    cleanup_dir(&output_dir);

    let cancel = AtomicBool::new(true);
    let result = run_streaming_with_cancel(decoded, &output_dir, &cancel, |_, _| {});

    let error = result.expect_err("a pre-cancelled run must return an error");
    assert!(
        openkara_lib::separator::error::is_cancelled(&error),
        "error should be the cancellation sentinel, got: {error:?}"
    );
    assert_no_stem_files(&output_dir);

    cleanup_dir(&output_dir);
}

#[test]
fn streaming_separation_cancelled_after_first_chunk_writes_no_stems() {
    let fixture = decode::decode_file(&fixture_path("audio", "fixture.wav"))
        .expect("wav fixture should decode");
    // Repeat so the schedule spans multiple chunks and a later iteration hits
    // the cancellation checkpoint.
    let mut long_audio = fixture.clone();
    long_audio.samples = fixture.samples.repeat(8);

    let output_dir = unique_output_dir();
    cleanup_dir(&output_dir);

    let cancel = AtomicBool::new(false);
    let mut chunks_completed = 0usize;
    let result =
        run_streaming_with_cancel(long_audio, &output_dir, &cancel, |completed, _total| {
            chunks_completed = completed;
            // Flag cancellation after the first chunk finishes; the next iteration
            // checkpoint returns early.
            if completed == 1 {
                cancel.store(true, Ordering::SeqCst);
            }
        });

    let error = result.expect_err("a mid-run cancellation must return an error");
    assert!(
        openkara_lib::separator::error::is_cancelled(&error),
        "error should be the cancellation sentinel, got: {error:?}"
    );
    assert!(
        chunks_completed >= 1,
        "at least one chunk should complete before cancellation"
    );
    assert_no_stem_files(&output_dir);

    cleanup_dir(&output_dir);
}

#[test]
fn separates_fixture_audio_into_four_stem_ogg_files() {
    let decoded = decode::decode_file(&fixture_path("audio", "fixture.wav"))
        .expect("wav fixture should decode");
    let expected_sample_rate = decoded.sample_rate;
    let expected_channels = decoded.channels;

    let output_dir = unique_output_dir();
    cleanup_dir(&output_dir);

    let normalized = separate_four_stem_to_dir(decoded, &output_dir);

    // All four stem files should exist.
    for stem_name in ["vocals", "drums", "bass", "other"] {
        let stem_path = output_dir.join(format!("{stem_name}.ogg"));
        assert!(stem_path.exists(), "{} should exist", stem_path.display());
    }

    // Verify the output files are valid OGG files by decoding them.
    for stem_name in ["vocals", "drums", "bass", "other"] {
        let stem_path = output_dir.join(format!("{stem_name}.ogg"));
        let stem_audio = decode::decode_file(&stem_path).expect("stem should decode");
        assert_eq!(
            stem_audio.sample_rate, expected_sample_rate,
            "{stem_name} sample rate should match"
        );
        assert_eq!(
            stem_audio.channels, expected_channels,
            "{stem_name} channel count should match"
        );
        // The stem should have approximately the same number of frames as the input.
        let expected_frames = normalized.samples.len() / normalized.channels;
        let actual_frames = stem_audio.samples.len() / stem_audio.channels;
        let diff = expected_frames.abs_diff(actual_frames);
        // Allow up to 1% difference due to OGG encoding boundaries.
        let tolerance = (expected_frames / 100).max(1);
        assert!(
            diff <= tolerance,
            "{stem_name} frame count {actual_frames} differs from expected {expected_frames} by {diff} (tolerance {tolerance})"
        );
    }

    cleanup_dir(&output_dir);
}

#[test]
fn separates_audio_longer_than_a_single_demucs_window() {
    let fixture = decode::decode_file(&fixture_path("audio", "fixture.wav"))
        .expect("wav fixture should decode");

    let mut long_audio = fixture.clone();
    long_audio.samples = fixture.samples.repeat(8);

    let output_dir = unique_output_dir();
    cleanup_dir(&output_dir);

    let normalized = separate_four_stem_to_dir(long_audio, &output_dir);

    // Verify all four stems exist and have approximately the right length.
    let expected_frames = normalized.samples.len() / normalized.channels;
    for stem_name in ["vocals", "drums", "bass", "other"] {
        let stem_path = output_dir.join(format!("{stem_name}.ogg"));
        assert!(stem_path.exists(), "{} should exist", stem_path.display());
        let stem_audio = decode::decode_file(&stem_path).expect("stem should decode");
        let actual_frames = stem_audio.samples.len() / stem_audio.channels;
        let diff = expected_frames.abs_diff(actual_frames);
        let tolerance = (expected_frames / 100).max(1);
        assert!(
            diff <= tolerance,
            "{stem_name} frame count {actual_frames} differs from expected {expected_frames} by {diff} (tolerance {tolerance})"
        );
    }

    cleanup_dir(&output_dir);
}
