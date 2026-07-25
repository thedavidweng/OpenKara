//! End-to-end spectral session path (issue #172 PR 2).
//!
//! Runs the full streaming separation through a spectral-core model and,
//! when the dev waveform model is present, compares the decoded stem output
//! against the waveform path on the same audio (codec-tolerant thresholds:
//! both paths encode with identical Vorbis settings, so a composition bug —
//! wrong stem order, missing time branch, broken premix — shows up orders
//! of magnitude above the tolerance).
//!
//! Gated on `OPENKARA_SPECTRAL_MODEL`: the spectral-core artifact is not in
//! the stable catalog yet, so CI cannot provision it. Locally:
//!
//! ```text
//! OPENKARA_SPECTRAL_MODEL=/path/to/htdemucs.spectral.onnx \
//!     cargo test --test phase7_spectral_session
//! ```

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

mod support;

use openkara_lib::audio::encode::StreamingOggWriter;
use openkara_lib::{
    audio::decode,
    config::{ExecutionProviderPreference, StemMode},
    separator::{
        inference::{self, StemWriters},
        model::{self, TensorInterface},
        preprocess,
        workspace::SeparationWorkspace,
    },
};

fn spectral_model_path() -> Option<PathBuf> {
    let path = std::env::var_os("OPENKARA_SPECTRAL_MODEL")?;
    Some(PathBuf::from(path))
}

fn fixture_path(directory: &str, filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(directory)
        .join(filename)
}

fn initialize_test_runtime() {
    let runtime_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("generated")
        .join("onnxruntime")
        .join(model::ORT_RUNTIME_FILENAME);
    model::ensure_runtime_loaded_from_path(&runtime_path)
        .expect("dev runtime should initialize for spectral session tests");
}

/// Run streaming separation with the given model and mode into `output_dir`.
fn separate_to_dir(
    model_path: &Path,
    stem_mode: StemMode,
    output_dir: &Path,
) -> openkara_lib::separator::inference::SeparationOutcome {
    let loaded_model = model::load_from_path(model_path, ExecutionProviderPreference::Cpu)
        .expect("model should load");

    let decoded = decode::decode_file(&fixture_path("audio", "fixture.wav"))
        .expect("fixture audio should decode");
    let normalized =
        preprocess::normalize_audio_for_model(decoded).expect("audio should normalize");

    let channels = normalized.channels;
    let input_frame_count = normalized.samples.len() / channels;
    let chunk_size =
        preprocess::target_frame_count(&loaded_model, input_frame_count).expect("chunk size");
    let hop_size = chunk_size / 2;

    fs::create_dir_all(output_dir).expect("output dir should be created");
    let sample_rate = normalized.sample_rate;

    let writer = |name: &str| {
        StreamingOggWriter::new(&output_dir.join(name), sample_rate, channels, None, None)
            .expect("stem writer")
    };
    let mut writers = match stem_mode {
        StemMode::TwoStem => StemWriters {
            mode: StemMode::TwoStem,
            vocals: writer("vocals.ogg"),
            accompaniment: Some(writer("accompaniment.ogg")),
            drums: None,
            bass: None,
            other: None,
        },
        StemMode::FourStem => StemWriters {
            mode: StemMode::FourStem,
            vocals: writer("vocals.ogg"),
            accompaniment: None,
            drums: Some(writer("drums.ogg")),
            bass: Some(writer("bass.ogg")),
            other: Some(writer("other.ogg")),
        },
    };

    let mut workspace =
        SeparationWorkspace::new(stem_mode, channels, chunk_size, hop_size, input_frame_count);

    let outcome = inference::separate_streaming(
        &loaded_model,
        &normalized,
        stem_mode,
        &mut writers,
        &mut workspace,
        &AtomicBool::new(false),
        |_, _| {},
    )
    .expect("streaming separation should succeed");

    writers.finish_all().expect("writers should finalize");
    outcome
}

fn decoded_samples(path: &Path) -> Vec<f32> {
    decode::decode_file(path)
        .expect("stem output should decode")
        .samples
}

fn assert_sane_stem(path: &Path) {
    let samples = decoded_samples(path);
    assert!(!samples.is_empty(), "{} must not be empty", path.display());
    assert!(
        samples.iter().all(|s| s.is_finite()),
        "{} must be finite",
        path.display()
    );
    let rms = (samples
        .iter()
        .map(|s| (*s as f64) * (*s as f64))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt();
    assert!(rms.is_finite(), "{} rms must be finite", path.display());
}

/// Codec-tolerant comparison of two decoded stems. Vorbis noise is bounded
/// well below real composition errors (stem swap / missing time branch are
/// full-signal-scale differences).
fn assert_stems_close(label: &str, a: &Path, b: &Path) {
    let sa = decoded_samples(a);
    let sb = decoded_samples(b);
    let n = sa.len().min(sb.len());
    assert!(n > 0, "{label}: no overlapping samples");
    let mut sq = 0.0f64;
    let mut energy = 0.0f64;
    for i in 0..n {
        let d = (sa[i] - sb[i]) as f64;
        sq += d * d;
        energy += (sa[i] as f64) * (sa[i] as f64);
    }
    let rms = (sq / n as f64).sqrt();
    let signal_rms = (energy / n as f64).sqrt();
    eprintln!("{label}: diff rms {rms:.4e}, signal rms {signal_rms:.4e}");
    assert!(
        rms < 0.02,
        "{label}: spectral and waveform paths diverge (diff rms {rms:.4e})"
    );
}

#[test]
fn spectral_streaming_end_to_end_four_stem() {
    let Some(model_path) = spectral_model_path() else {
        eprintln!("skipping: OPENKARA_SPECTRAL_MODEL is not set");
        return;
    };
    initialize_test_runtime();

    let loaded = model::load_from_path(&model_path, ExecutionProviderPreference::Cpu)
        .expect("spectral model should load");
    assert_eq!(loaded.tensor_interface, TensorInterface::SpectralCore);
    assert!(loaded.spectral.is_some());
    drop(loaded);

    let out_dir = support::unique_temp_path("phase7-spectral-four");
    let outcome = separate_to_dir(&model_path, StemMode::FourStem, &out_dir);
    assert_eq!(outcome.stem_mode, StemMode::FourStem);

    for stem in ["vocals.ogg", "drums.ogg", "bass.ogg", "other.ogg"] {
        assert_sane_stem(&out_dir.join(stem));
    }

    // Cross-path equivalence when the dev waveform model is available.
    let waveform_model = model::default_model_path();
    if waveform_model.is_file() {
        let ref_dir = support::unique_temp_path("phase7-waveform-four");
        separate_to_dir(&waveform_model, StemMode::FourStem, &ref_dir);
        for stem in ["vocals.ogg", "drums.ogg", "bass.ogg", "other.ogg"] {
            assert_stems_close(stem, &ref_dir.join(stem), &out_dir.join(stem));
        }
        fs::remove_dir_all(&ref_dir).ok();
    } else {
        eprintln!("no dev waveform model; skipping cross-path comparison");
    }
    fs::remove_dir_all(&out_dir).ok();
}

#[test]
fn spectral_streaming_end_to_end_two_stem() {
    let Some(model_path) = spectral_model_path() else {
        eprintln!("skipping: OPENKARA_SPECTRAL_MODEL is not set");
        return;
    };
    initialize_test_runtime();

    let out_dir = support::unique_temp_path("phase7-spectral-two");
    let outcome = separate_to_dir(&model_path, StemMode::TwoStem, &out_dir);
    assert_eq!(outcome.stem_mode, StemMode::TwoStem);

    assert_sane_stem(&out_dir.join("vocals.ogg"));
    assert_sane_stem(&out_dir.join("accompaniment.ogg"));

    let waveform_model = model::default_model_path();
    if waveform_model.is_file() {
        let ref_dir = support::unique_temp_path("phase7-waveform-two");
        separate_to_dir(&waveform_model, StemMode::TwoStem, &ref_dir);
        for stem in ["vocals.ogg", "accompaniment.ogg"] {
            assert_stems_close(stem, &ref_dir.join(stem), &out_dir.join(stem));
        }
        fs::remove_dir_all(&ref_dir).ok();
    } else {
        eprintln!("no dev waveform model; skipping cross-path comparison");
    }
    fs::remove_dir_all(&out_dir).ok();
}

/// Interruption contract (issue #172 PR 4): a cancelled spectral run
/// publishes nothing and a fresh run restarts from chunk 0 successfully.
/// No checkpoint state exists by design (#171).
#[test]
fn spectral_cancellation_publishes_nothing_and_restarts_from_zero() {
    use openkara_lib::audio::decode::DecodedAudio;
    use std::sync::atomic::Ordering;

    let Some(model_path) = spectral_model_path() else {
        eprintln!("skipping: OPENKARA_SPECTRAL_MODEL is not set");
        return;
    };
    initialize_test_runtime();

    let loaded = model::load_from_path(&model_path, ExecutionProviderPreference::Cpu)
        .expect("spectral model should load");
    let segment = loaded
        .spectral
        .as_ref()
        .expect("verified interface")
        .segment_frames;

    // 1.5 windows -> two chunks at 50% overlap.
    let channels = 2usize;
    let frames = segment + segment / 2;
    let samples: Vec<f32> = (0..frames * channels)
        .map(|i| 0.1 * ((i as f32) * 0.001).sin())
        .collect();
    let audio = DecodedAudio {
        sample_rate: 44_100,
        channels,
        duration_ms: ((frames as f64 / 44_100.0) * 1000.0).round() as u64,
        samples,
    };

    let chunk_size = preprocess::target_frame_count(&loaded, frames).expect("chunk size");
    let hop_size = chunk_size / 2;
    let out_dir = support::unique_temp_path("phase7-spectral-cancel");
    fs::create_dir_all(&out_dir).expect("output dir");

    let make_writers = |dir: &Path| StemWriters {
        mode: StemMode::TwoStem,
        vocals: StreamingOggWriter::new(&dir.join("vocals.ogg"), 44_100, channels, None, None)
            .expect("vocals writer"),
        accompaniment: Some(
            StreamingOggWriter::new(&dir.join("accompaniment.ogg"), 44_100, channels, None, None)
                .expect("accompaniment writer"),
        ),
        drums: None,
        bass: None,
        other: None,
    };

    // First run: cancel after the first chunk completes.
    let cancel = AtomicBool::new(false);
    let mut writers = make_writers(&out_dir);
    let mut workspace =
        SeparationWorkspace::new(StemMode::TwoStem, channels, chunk_size, hop_size, frames);
    let error = inference::separate_streaming(
        &loaded,
        &audio,
        StemMode::TwoStem,
        &mut writers,
        &mut workspace,
        &cancel,
        |done, _| {
            if done >= 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
    )
    .expect_err("cancelled run must not complete");
    assert!(
        openkara_lib::separator::error::is_cancelled(&error),
        "error must be the cancellation sentinel, got {error:#}"
    );
    drop(writers); // writers drop without finish_all -> temp files cleaned up

    assert!(
        !out_dir.join("vocals.ogg").exists() && !out_dir.join("accompaniment.ogg").exists(),
        "a cancelled run must publish no stem files"
    );

    // Second run: from zero, to completion, same directory.
    let mut writers = make_writers(&out_dir);
    let mut workspace =
        SeparationWorkspace::new(StemMode::TwoStem, channels, chunk_size, hop_size, frames);
    let outcome = inference::separate_streaming(
        &loaded,
        &audio,
        StemMode::TwoStem,
        &mut writers,
        &mut workspace,
        &AtomicBool::new(false),
        |_, _| {},
    )
    .expect("restarted run must succeed");
    writers.finish_all().expect("writers finalize");
    assert_eq!(outcome.frames_written, frames);
    assert_sane_stem(&out_dir.join("vocals.ogg"));
    assert_sane_stem(&out_dir.join("accompaniment.ogg"));
    fs::remove_dir_all(&out_dir).ok();
}
