use anyhow::{bail, Context, Result};
use openkara_lib::smoke::{run_local_audio_smoke, LocalAudioSmokeConfig, SeparationSmokeMode};
use std::{env, path::PathBuf};

fn main() -> Result<()> {
    let mut input_dir = None;
    let mut output_dir = None;
    let mut separation_mode = SeparationSmokeMode::Auto;

    for argument in env::args().skip(1) {
        match argument.as_str() {
            "--skip-separation" => separation_mode = SeparationSmokeMode::Disabled,
            _ if input_dir.is_none() => input_dir = Some(PathBuf::from(argument)),
            _ if output_dir.is_none() => output_dir = Some(PathBuf::from(argument)),
            _ => bail!(usage()),
        }
    }

    // Load the staged development runtime explicitly so the smoke exercises
    // the exact catalog runtime `scripts/prepare-onnx-runtime.mjs` verified,
    // on every platform, instead of relying on dynamic-loader defaults.
    if separation_mode != SeparationSmokeMode::Disabled {
        let runtime_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("generated")
            .join("onnxruntime")
            .join(openkara_lib::separator::model::ORT_RUNTIME_FILENAME);
        openkara_lib::separator::model::ensure_runtime_loaded_from_path(&runtime_path)
            .with_context(|| {
                format!(
                    "failed to load the staged ONNX Runtime from {}; run scripts/setup.sh first",
                    runtime_path.display()
                )
            })?;
    }

    let input_dir = input_dir.ok_or_else(usage_error)?;
    let output_dir = output_dir.ok_or_else(usage_error)?;
    let report = run_local_audio_smoke(LocalAudioSmokeConfig {
        input_dir,
        output_dir,
        separation_mode,
        seek_iterations: 32,
    })?;

    println!(
        "local audio smoke complete\njson: {}\nmarkdown: {}",
        report.report_json_path.display(),
        report.report_markdown_path.display()
    );

    Ok(())
}

fn usage() -> String {
    "usage: cargo run --example local-audio-smoke -- <input-dir> <output-dir> [--skip-separation]"
        .to_string()
}

fn usage_error() -> anyhow::Error {
    anyhow::anyhow!(usage())
}
