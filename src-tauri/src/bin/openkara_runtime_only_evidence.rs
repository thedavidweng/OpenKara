use anyhow::{bail, Context, Result};
use openkara_lib::{
    config::ExecutionProviderPreference,
    separator::{
        catalog,
        runtime_only_evidence::{produce, EvidenceConfig},
    },
};
use std::{env, path::PathBuf};

fn main() {
    if let Err(error) = run() {
        eprintln!("OpenKara runtime-only app evidence failed: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut pointer = None;
    let mut manifest = None;
    let mut artifacts_dir = None;
    let mut audio = None;
    let mut runtime = None;
    let mut provider = None;
    let mut target = None;
    let mut report = None;
    let mut work_dir = None;

    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--pointer" => pointer = Some(next_value(&mut arguments, "--pointer")?),
            "--manifest" => {
                manifest = Some(PathBuf::from(next_value(&mut arguments, "--manifest")?))
            }
            "--artifacts-dir" => {
                artifacts_dir = Some(PathBuf::from(next_value(
                    &mut arguments,
                    "--artifacts-dir",
                )?))
            }
            "--audio" => audio = Some(PathBuf::from(next_value(&mut arguments, "--audio")?)),
            "--runtime" => runtime = Some(next_value(&mut arguments, "--runtime")?),
            "--provider" => provider = Some(next_value(&mut arguments, "--provider")?),
            "--target" => target = Some(next_value(&mut arguments, "--target")?),
            "--report" => report = Some(PathBuf::from(next_value(&mut arguments, "--report")?)),
            "--work-dir" => {
                work_dir = Some(PathBuf::from(next_value(&mut arguments, "--work-dir")?))
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => bail!("unknown argument {other:?}\n\n{USAGE}"),
        }
    }

    let pointer = pointer.context("--pointer is required")?;
    let artifacts_dir = artifacts_dir.context("--artifacts-dir is required")?;
    let audio = audio.context("--audio is required")?;
    let runtime = runtime.context("--runtime is required")?;
    let provider_value = provider.context("--provider is required")?;
    let provider = ExecutionProviderPreference::parse(&provider_value)
        .with_context(|| format!("unsupported provider {provider_value:?}"))?;
    let target = target.unwrap_or_else(|| catalog::current_target_triple().to_owned());
    let report = report.context("--report is required")?;

    let outcome = produce(EvidenceConfig {
        pointer,
        manifest_path: manifest,
        artifacts_dir,
        audio_path: audio,
        runtime_artifact_id: runtime,
        provider,
        target_triple: target,
        report_path: report,
        work_dir,
    })?;

    println!(
        "wrote {} (status {}, {} measured edge(s), {} not app-applicable)",
        outcome.report_path.display(),
        outcome.status,
        outcome.measured_edges,
        outcome.not_applicable_edges.len()
    );
    for edge in &outcome.not_applicable_edges {
        println!("not app-applicable: {edge}");
    }
    if !outcome.failed_edges.is_empty() {
        for edge in &outcome.failed_edges {
            eprintln!("failed edge: {edge}");
        }
        bail!(
            "app evidence report status is {}; {} app-applicable edge(s) failed",
            outcome.status,
            outcome.failed_edges.len()
        );
    }
    if outcome.status != "passed" {
        bail!("app evidence report status is {}", outcome.status);
    }
    Ok(())
}

fn next_value(arguments: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    arguments
        .next()
        .with_context(|| format!("{flag} requires a value"))
}

const USAGE: &str = "\
OpenKara runtime-only app-integration evidence producer

Consumes a published candidate channel pointer, loads the candidate runtime's
exact shared library, and runs every supported (model, runtime, target,
provider) edge through the application's own audio and separation path.

usage:
  openkara_runtime_only_evidence \\
    --pointer <candidate-channel.json|https-url> \\
    [--manifest <local-release-manifest.json>] \\
    --artifacts-dir <dir-with-candidate-archives> \\
    --audio <real-audio-file> \\
    --runtime <candidate-runtime-artifact-id> \\
    --provider <cpu|coreml|xnnpack|directml> \\
    [--target <target-triple>] \\
    --report <app-report.json> \\
    [--work-dir <dir>]

notes:
  - one process loads one ONNX Runtime; produce one report per runtime artifact
  - one report covers one target/provider group
  - the audio file is decoded and normalized through the app path, then the
    producer runs exactly one full model window (343980 frames at 44100 Hz)
  - the report status is failed when any supported edge is not measurable by
    this OpenKara build (for example a waveform-interface catalog artifact)
";
