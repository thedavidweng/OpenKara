//! Release-CI-only smoke mode for the installed Windows application.
//!
//! The executable is built into an NSIS installer and then invoked from its
//! installed location. It deliberately uses the production app-data runtime
//! and model bootstrap helpers instead of `src-tauri/generated` or
//! `src-tauri/models`, which catches first-install and cold-restart regressions
//! that a source-tree smoke cannot observe.

use crate::{
    commands::{bootstrap, runtime_bootstrap, unix_timestamp},
    config,
    separator::{self, model},
    services::separation,
    smoke::{
        run_local_audio_smoke, LocalAudioSmokeConfig, LocalAudioSmokeReport, SeparationSmokeMode,
    },
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const REPORT_FILENAME: &str = "installed-app-smoke-report.json";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum AutomationSmokePhase {
    Prepare,
    Restart,
}

impl AutomationSmokePhase {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "prepare" => Ok(Self::Prepare),
            "restart" => Ok(Self::Restart),
            _ => bail!("automation smoke phase must be prepare or restart, got {value}"),
        }
    }
}

#[derive(Debug)]
struct AutomationSmokeConfig {
    phase: AutomationSmokePhase,
    app_data_dir: PathBuf,
    input_dir: PathBuf,
    output_dir: PathBuf,
}

#[derive(Debug, Serialize)]
struct BootstrapEvent<T> {
    event: &'static str,
    snapshot: T,
}

#[derive(Debug, Serialize)]
struct InstalledAppSmokeReport {
    generated_at: i64,
    phase: AutomationSmokePhase,
    app_data_dir: String,
    runtime: runtime_bootstrap::RuntimeBootstrapStatusSnapshot,
    runtime_events: Vec<BootstrapEvent<runtime_bootstrap::RuntimeBootstrapStatusSnapshot>>,
    model: bootstrap::ModelBootstrapStatusSnapshot,
    model_events: Vec<BootstrapEvent<bootstrap::ModelBootstrapStatusSnapshot>>,
    model_path: String,
    local_audio_smoke: Option<LocalAudioSmokeReport>,
}

/// Runs only when the feature-gated marker is the first command line option.
/// Returning `Ok(false)` keeps every normal Tauri invocation unchanged.
pub fn maybe_run_from_cli() -> Result<bool> {
    let mut arguments = env::args();
    let _program = arguments.next();
    if arguments.next().as_deref() != Some("--automation-smoke") {
        return Ok(false);
    }

    run(parse_config(arguments)?)?;
    Ok(true)
}

fn parse_config(mut arguments: impl Iterator<Item = String>) -> Result<AutomationSmokeConfig> {
    let phase = AutomationSmokePhase::parse(
        &arguments
            .next()
            .context("missing automation smoke phase after --automation-smoke")?,
    )?;
    let mut app_data_dir = None;
    let mut input_dir = None;
    let mut output_dir = None;

    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .with_context(|| format!("missing value for {argument}"))
        };
        match argument.as_str() {
            "--app-data-dir" => app_data_dir = Some(PathBuf::from(value()?)),
            "--input-dir" => input_dir = Some(PathBuf::from(value()?)),
            "--output-dir" => output_dir = Some(PathBuf::from(value()?)),
            _ => bail!("unknown automation smoke argument {argument}"),
        }
    }

    Ok(AutomationSmokeConfig {
        phase,
        app_data_dir: app_data_dir.context("--app-data-dir is required")?,
        input_dir: input_dir.context("--input-dir is required")?,
        output_dir: output_dir.context("--output-dir is required")?,
    })
}

fn run(config: AutomationSmokeConfig) -> Result<()> {
    fs::create_dir_all(&config.app_data_dir).with_context(|| {
        format!(
            "failed to create automation app-data directory {}",
            config.app_data_dir.display()
        )
    })?;
    fs::create_dir_all(&config.output_dir).with_context(|| {
        format!(
            "failed to create automation output directory {}",
            config.output_dir.display()
        )
    })?;

    let active_variant = config::load_config(&config.app_data_dir)
        .context("failed to load app config for automation smoke")?
        .unwrap_or_default()
        .effective_model_variant();
    let descriptor = separator::bootstrap::descriptor_for(active_variant);
    let managed_model_path =
        separator::bootstrap::managed_model_path_for(&config.app_data_dir, descriptor);

    if matches!(config.phase, AutomationSmokePhase::Restart) {
        verify_cold_start_state(
            &config.app_data_dir,
            active_variant,
            descriptor.file_sha256.as_str(),
        )?;
    }

    let runtime_status = Arc::new(Mutex::new(runtime_bootstrap::snapshot_from_disk(
        &config.app_data_dir,
    )));
    let model_status = Arc::new(Mutex::new(bootstrap::pending_status(
        managed_model_path.display().to_string(),
    )));
    let mut runtime_events: Vec<BootstrapEvent<runtime_bootstrap::RuntimeBootstrapStatusSnapshot>> =
        Vec::new();
    let mut model_events: Vec<BootstrapEvent<bootstrap::ModelBootstrapStatusSnapshot>> = Vec::new();

    let model_path = separation::ensure_runtime_and_managed_model_blocking(
        &config.app_data_dir,
        &runtime_status,
        &model_status,
        &mut |event, snapshot| {
            runtime_events.push(BootstrapEvent { event, snapshot });
        },
        &mut |event, snapshot| {
            model_events.push(BootstrapEvent { event, snapshot });
        },
    )
    .map_err(|error| anyhow::anyhow!(error.message))?;

    if model_path != managed_model_path {
        bail!(
            "automation smoke resolved model outside app data: {}",
            model_path.display()
        );
    }

    let local_audio_smoke = match config.phase {
        AutomationSmokePhase::Prepare => None,
        AutomationSmokePhase::Restart => Some(
            run_local_audio_smoke(LocalAudioSmokeConfig {
                input_dir: config.input_dir.clone(),
                output_dir: config.output_dir.clone(),
                separation_mode: SeparationSmokeMode::Auto,
                model_path: Some(model_path.clone()),
                seek_iterations: 32,
            })
            .context("installed app could not import, play, and separate the smoke fixture")?,
        ),
    };

    let runtime = runtime_status
        .lock()
        .map_err(|_| anyhow::anyhow!("runtime bootstrap status lock was poisoned"))?
        .clone();
    let model = model_status
        .lock()
        .map_err(|_| anyhow::anyhow!("model bootstrap status lock was poisoned"))?
        .clone();
    let report = InstalledAppSmokeReport {
        generated_at: unix_timestamp(),
        phase: config.phase,
        app_data_dir: config.app_data_dir.display().to_string(),
        runtime,
        runtime_events,
        model,
        model_events,
        model_path: model_path.display().to_string(),
        local_audio_smoke,
    };
    let report_path = config.output_dir.join(REPORT_FILENAME);
    fs::write(
        &report_path,
        serde_json::to_string_pretty(&report)
            .context("failed to serialize installed app report")?,
    )
    .with_context(|| {
        format!(
            "failed to write installed app report {}",
            report_path.display()
        )
    })?;

    Ok(())
}

/// Mirrors the runtime/model portion of `app_runtime::setup_app` in a fresh
/// process, before the blocking separation bootstrap executes. This verifies
/// the persisted slot state and managed model are usable after a restart.
fn verify_cold_start_state(
    app_data_dir: &Path,
    active_variant: config::ModelVariant,
    expected_model_sha256: &str,
) -> Result<()> {
    let startup = separator::runtime_bootstrap::begin_startup(app_data_dir)
        .context("failed to derive runtime startup plan")?
        .context("installed runtime was absent on cold restart")?;
    model::ensure_runtime_loaded_from_path(&startup.library_path).with_context(|| {
        format!(
            "installed runtime could not load on cold restart from {}",
            startup.library_path.display()
        )
    })?;
    if startup.proving_candidate {
        separator::runtime_bootstrap::finish_activation_success(app_data_dir)
            .context("failed to finalize a runtime activation after cold restart")?;
    }

    let no_development_fallback = app_data_dir.join("__automation_smoke_no_dev_model__");
    let model_startup = crate::derive_startup_model_bootstrap(
        app_data_dir,
        &no_development_fallback,
        active_variant,
        expected_model_sha256,
    )
    .context("failed to derive managed-model startup state")?;
    if model_startup.should_spawn_bootstrap_worker {
        bail!("installed model was absent on cold restart");
    }
    if model_startup.model_path != model_startup.managed_model_path {
        bail!(
            "cold restart selected a non-managed model {}",
            model_startup.model_path.display()
        );
    }

    Ok(())
}
