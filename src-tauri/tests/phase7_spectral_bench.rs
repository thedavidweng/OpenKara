//! Cross-target measurement harness for the spectral-core candidate
//! (issue #172 PR 4).
//!
//! Records, per target: artifact size, cold session load, first-window
//! latency, warm median/p95 latency, real-time factor, and peak RSS (unix;
//! reported as null on Windows). Emits one JSON object to stdout (marked
//! with a `SPECTRAL_BENCH_JSON:` prefix) and, when `OPENKARA_BENCH_OUT` is
//! set, writes it to that path for CI artifact upload.
//!
//! Gated on `OPENKARA_SPECTRAL_MODEL`; runs the full streaming path in both
//! stem modes on a synthetic two-window program so the numbers include the
//! app-side transforms, composition, and OLA — the product path, not a bare
//! session benchmark.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Instant,
};

mod support;

use openkara_lib::audio::decode::DecodedAudio;
use openkara_lib::audio::encode::StreamingOggWriter;
use openkara_lib::{
    config::{ExecutionProviderPreference, StemMode},
    separator::{
        inference::{self, StemWriters},
        model::{self, TensorInterface},
        preprocess,
        workspace::SeparationWorkspace,
    },
};

fn initialize_test_runtime() {
    let runtime_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("generated")
        .join("onnxruntime")
        .join(model::ORT_RUNTIME_FILENAME);
    model::ensure_runtime_loaded_from_path(&runtime_path)
        .expect("dev runtime should initialize for the spectral bench");
}

/// Peak resident set size in kilobytes, when the platform exposes it.
fn peak_rss_kb() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        // SAFETY: getrusage writes a plain struct for the calling process.
        let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
        if rc == 0 {
            let usage = unsafe { usage.assume_init() };
            // macOS reports bytes, Linux kilobytes.
            let raw = usage.ru_maxrss as u64;
            #[cfg(target_os = "macos")]
            return Some(raw / 1024);
            #[cfg(not(target_os = "macos"))]
            return Some(raw);
        }
        None
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn synthetic_audio(frames: usize) -> DecodedAudio {
    // Deterministic band-limited-ish program material.
    let channels = 2usize;
    let mut samples = vec![0.0f32; frames * channels];
    for (i, s) in samples.iter_mut().enumerate() {
        let t = (i / channels) as f32 / 44_100.0;
        let ch = (i % channels) as f32;
        *s = 0.15 * (2.0 * std::f32::consts::PI * (220.0 + 55.0 * ch) * t).sin()
            + 0.05 * (2.0 * std::f32::consts::PI * 3_520.0 * t).sin();
    }
    DecodedAudio {
        sample_rate: 44_100,
        channels,
        duration_ms: ((frames as f64 / 44_100.0) * 1000.0).round() as u64,
        samples,
    }
}

fn median_and_p95(mut values: Vec<f64>) -> (f64, f64) {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = values[values.len() / 2];
    let p95_index = ((values.len() as f64 * 0.95).ceil() as usize).clamp(1, values.len()) - 1;
    (median, values[p95_index])
}

fn run_mode(
    model: &openkara_lib::separator::model::LoadedModel,
    audio: &DecodedAudio,
    stem_mode: StemMode,
    out_dir: &Path,
) -> (usize, Vec<f64>) {
    let channels = audio.channels;
    let input_frame_count = audio.samples.len() / channels;
    let chunk_size = preprocess::target_frame_count(model, input_frame_count).expect("chunk size");
    let hop_size = chunk_size / 2;

    fs::create_dir_all(out_dir).expect("bench output dir");
    let writer = |name: &str| {
        StreamingOggWriter::new(&out_dir.join(name), audio.sample_rate, channels, None, None)
            .expect("bench writer")
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

    let mut chunk_times = Vec::new();
    let mut last = Instant::now();
    let outcome = inference::separate_streaming(
        model,
        audio,
        stem_mode,
        &mut writers,
        &mut workspace,
        &AtomicBool::new(false),
        |_, _| {
            chunk_times.push(last.elapsed().as_secs_f64());
            last = Instant::now();
        },
    )
    .expect("bench separation should succeed");
    writers.finish_all().expect("bench writers finalize");

    (outcome.frames_written, chunk_times)
}

#[test]
fn spectral_candidate_bench() {
    let Some(model_path) = std::env::var_os("OPENKARA_SPECTRAL_MODEL") else {
        eprintln!("skipping spectral bench: OPENKARA_SPECTRAL_MODEL is not set");
        return;
    };
    let model_path = PathBuf::from(model_path);
    initialize_test_runtime();

    let artifact_bytes = fs::metadata(&model_path).expect("model metadata").len();

    let cold_start = Instant::now();
    let model = model::load_from_path(&model_path, ExecutionProviderPreference::Cpu)
        .expect("spectral model should load");
    let cold_load_s = cold_start.elapsed().as_secs_f64();
    assert_eq!(model.tensor_interface, TensorInterface::SpectralCore);
    let segment = model
        .spectral
        .as_ref()
        .expect("verified interface")
        .segment_frames;

    // Two full windows (three chunks at 50% overlap) of program material.
    let audio = synthetic_audio(segment * 2);
    let audio_seconds = (segment * 2) as f64 / 44_100.0;

    let four_dir = support::unique_temp_path("phase7-bench-four");
    let four_start = Instant::now();
    let (_, four_chunks) = run_mode(&model, &audio, StemMode::FourStem, &four_dir);
    let four_wall = four_start.elapsed().as_secs_f64();
    fs::remove_dir_all(&four_dir).ok();

    let two_dir = support::unique_temp_path("phase7-bench-two");
    let two_start = Instant::now();
    let (_, two_chunks) = run_mode(&model, &audio, StemMode::TwoStem, &two_dir);
    let two_wall = two_start.elapsed().as_secs_f64();
    fs::remove_dir_all(&two_dir).ok();

    let first_window_s = four_chunks.first().copied().unwrap_or(f64::NAN);
    let warm: Vec<f64> = four_chunks
        .iter()
        .skip(1)
        .chain(two_chunks.iter().skip(1))
        .copied()
        .collect();
    let (warm_median_s, warm_p95_s) = if warm.is_empty() {
        (f64::NAN, f64::NAN)
    } else {
        median_and_p95(warm)
    };

    let report = serde_json::json!({
        "schema_version": "openkara.spectral-bench/v1",
        "target": format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        "model_path": model_path.display().to_string(),
        "artifact_bytes": artifact_bytes,
        "segment_frames": segment,
        "cold_load_s": cold_load_s,
        "first_window_s": first_window_s,
        "warm_median_s": warm_median_s,
        "warm_p95_s": warm_p95_s,
        "rtf_four_stem": four_wall / audio_seconds,
        "rtf_two_stem": two_wall / audio_seconds,
        "peak_rss_kb": peak_rss_kb(),
    });
    let line = serde_json::to_string(&report).expect("bench json");
    println!("SPECTRAL_BENCH_JSON: {line}");
    if let Some(out) = std::env::var_os("OPENKARA_BENCH_OUT") {
        fs::write(&out, format!("{line}\n")).expect("bench report write");
    }
}
