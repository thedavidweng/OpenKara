use crate::{
    automation_faults::FaultScenario,
    automation_report::{
        AccessibilitySummary, ApplicationIdentity, Artifact, Assertion, AssertionResult,
        AudioSummary, AutomationReport, DatabaseSummary, Environment, ModelIdentity, ReportError,
        ReportStatus, RuntimeIdentity, Step, StepStatus,
    },
    automation_smoke::{AutomationSmokeConfig, AutomationSmokePhase, InstalledAppSmokeReport},
    commands::unix_timestamp,
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub struct ScenarioConfig {
    pub scenario: String,
    pub app_data_dir: PathBuf,
    pub input_dir: PathBuf,
    pub output_dir: PathBuf,
    pub installed_exe: Option<PathBuf>,
    pub execution_provider: Option<String>,
    pub locale: Option<String>,
    pub theme: Option<String>,
    pub seek_iterations: usize,
    pub injected_faults: Vec<FaultScenario>,
}

pub fn run_scenario(config: &ScenarioConfig) -> Result<AutomationReport> {
    let mut builder = ReportBuilder::new(&config.scenario);

    std::fs::create_dir_all(&config.output_dir).with_context(|| {
        format!(
            "failed to create scenario output directory {}",
            config.output_dir.display()
        )
    })?;

    let prepare_output = config.output_dir.join("prepare");
    let restart_output = config.output_dir.join("restart");

    let prepare_step = builder.begin_step("prepare", "First-install runtime and model bootstrap");
    let prepare_report = run_phase(config, AutomationSmokePhase::Prepare, &prepare_output);
    let prepare_ok = prepare_report.is_ok();
    builder.end_step(
        prepare_step,
        if prepare_ok {
            StepStatus::Passed
        } else {
            StepStatus::Failed
        },
        prepare_report.as_ref().err().map(|e| e.to_string()),
    );

    if let Ok(report) = prepare_report.as_ref() {
        record_smoke_assertions(&mut builder, "prepare", report);
    }

    let restart_step = builder.begin_step("restart", "Cold restart with managed runtime and model");
    let restart_report = if prepare_ok {
        run_phase(config, AutomationSmokePhase::Restart, &restart_output)
    } else {
        Err(anyhow::anyhow!("prepare step failed; skipping restart"))
    };
    let restart_ok = restart_report.is_ok();
    builder.end_step(
        restart_step,
        if !prepare_ok {
            StepStatus::Skipped
        } else if restart_ok {
            StepStatus::Passed
        } else {
            StepStatus::Failed
        },
        restart_report.as_ref().err().map(|e| e.to_string()),
    );

    if let Ok(report) = restart_report.as_ref() {
        record_smoke_assertions(&mut builder, "restart", report);
    }

    let identity_step = builder.begin_step("identity", "Runtime and model identity");
    let runtime_identity = compute_runtime_identity(&config.app_data_dir);
    let model_identity = compute_model_identity(&config.app_data_dir);
    let identity_ok = runtime_identity.is_ok() && model_identity.is_ok();
    builder.end_step(
        identity_step,
        if identity_ok {
            StepStatus::Passed
        } else {
            StepStatus::Failed
        },
        runtime_identity
            .as_ref()
            .err()
            .or(model_identity.as_ref().err())
            .map(|e| e.to_string()),
    );

    if let (Ok(runtime), Ok(model)) = (&runtime_identity, &model_identity) {
        if let Err(error) =
            record_oka284_assertions(&mut builder, &config.app_data_dir, runtime, model)
        {
            tracing::error!("OKA-284 cross-check failed: {error:#}");
            builder.add_error(
                &format!("OKA-284 cross-check failed: {error:#}"),
                Some("identity".into()),
                None,
            );
        }
    }

    let audio_step = builder.begin_step("audio", "Audio output summary");
    let (audio_summary, audio_status, audio_error) = match restart_report.as_ref() {
        Err(error) => (
            AudioSummary::default(),
            StepStatus::Skipped,
            Some(format!("restart step failed: {error}")),
        ),
        Ok(report) => match report.local_audio_smoke.as_ref() {
            None => (
                AudioSummary::default(),
                StepStatus::Failed,
                Some("no local audio smoke produced on restart".into()),
            ),
            Some(smoke) => {
                let summary = build_audio_summary(smoke);
                if summary.sample_rate > 0
                    && summary.channel_count > 0
                    && summary.non_silent_samples
                {
                    (summary, StepStatus::Passed, None)
                } else {
                    (
                        summary,
                        StepStatus::Failed,
                        Some("audio output is missing, silent, or has no valid WAV header".into()),
                    )
                }
            }
        },
    };
    builder.audio = audio_summary;
    builder.end_step(audio_step, audio_status, audio_error);

    let database_path = config.app_data_dir.join("openkara.sqlite3");
    builder.database = DatabaseSummary {
        schema_version: crate::cache::schema_version(),
        path: database_path.display().to_string(),
    };

    builder.runtime = runtime_identity.unwrap_or_default();
    builder.model = model_identity.unwrap_or_default();

    if !prepare_ok || !restart_ok {
        builder.add_error(
            if !prepare_ok {
                "prepare step failed"
            } else {
                "restart step failed"
            },
            None,
            None,
        );
    }

    if let Ok(_prepare) = prepare_report.as_ref() {
        builder.add_artifact(
            &prepare_output.join("installed-app-smoke-report.json"),
            "installed-app-smoke-report",
            Some("first-install bootstrap report".into()),
        );
    }
    if let Ok(restart) = restart_report.as_ref() {
        builder.add_artifact(
            &restart_output.join("installed-app-smoke-report.json"),
            "installed-app-smoke-report",
            Some("cold-restart separation report".into()),
        );
        if let Some(smoke) = restart.local_audio_smoke.as_ref() {
            builder.add_artifact(&smoke.report_json_path, "local-audio-smoke-report", None);
            builder.add_artifact(
                &smoke.report_markdown_path,
                "local-audio-smoke-report-md",
                None,
            );
        }
    }
    builder.add_artifact(
        &crate::automation_report::AutomationReport::report_path(&config.output_dir),
        "automation-report",
        Some("canonical scenario report".into()),
    );

    let report = builder.finish(&config.execution_provider, &config.locale, &config.theme);
    let report_path = AutomationReport::report_path(&config.output_dir);
    report.write(&report_path).with_context(|| {
        format!(
            "failed to write canonical automation report {}",
            report_path.display()
        )
    })?;

    if report.status != ReportStatus::Passed {
        bail!(
            "scenario {} failed with {} errors",
            report.scenario,
            report.errors.len()
        );
    }

    Ok(report)
}

fn run_phase(
    config: &ScenarioConfig,
    phase: AutomationSmokePhase,
    output_dir: &Path,
) -> Result<InstalledAppSmokeReport> {
    std::fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "failed to create {phase:?} output directory {}",
            output_dir.display()
        )
    })?;

    let smoke_config = AutomationSmokeConfig {
        phase,
        app_data_dir: config.app_data_dir.clone(),
        input_dir: config.input_dir.clone(),
        output_dir: output_dir.to_path_buf(),
    };

    if let Some(installed_exe) = config.installed_exe.as_ref() {
        let mut cmd = Command::new(installed_exe);
        let phase_str = match phase {
            AutomationSmokePhase::Prepare => "prepare",
            AutomationSmokePhase::Restart => "restart",
        };
        cmd.arg("--automation-smoke")
            .arg(phase_str)
            .arg("--app-data-dir")
            .arg(&config.app_data_dir)
            .arg("--input-dir")
            .arg(&config.input_dir)
            .arg("--output-dir")
            .arg(output_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd.output().with_context(|| {
            format!(
                "failed to spawn installed app for {phase:?} from {}",
                installed_exe.display()
            )
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("installed app {phase:?} failed: {stderr}");
        }

        let report_path = output_dir.join("installed-app-smoke-report.json");
        let contents = std::fs::read_to_string(&report_path).with_context(|| {
            format!(
                "installed app did not write {} for {phase:?}",
                report_path.display()
            )
        })?;
        serde_json::from_str(&contents).with_context(|| {
            format!(
                "failed to parse installed app report {} for {phase:?}",
                report_path.display()
            )
        })
    } else {
        crate::automation_smoke::run_phase(&smoke_config)
    }
}

fn record_smoke_assertions(
    builder: &mut ReportBuilder,
    label: &str,
    report: &InstalledAppSmokeReport,
) {
    builder.add_assertion(
        &format!("OKA-PHASE-{label}"),
        match report.phase {
            AutomationSmokePhase::Prepare => "prepare",
            AutomationSmokePhase::Restart => "restart",
        },
        match report.phase {
            AutomationSmokePhase::Prepare => "prepare",
            AutomationSmokePhase::Restart => "restart",
        },
        true,
        "",
    );
    let expected_runtime = "ready";
    let expected_model = "ready";
    let runtime_ready =
        report.runtime.state == crate::commands::runtime_bootstrap::RuntimeBootstrapState::Ready;
    let model_ready = report.model.state == crate::commands::bootstrap::ModelBootstrapState::Ready;
    builder.add_assertion(
        &format!("OKA-RUNTIME-READY-{label}"),
        expected_runtime,
        &state_string(&report.runtime.state),
        runtime_ready,
        &report.app_data_dir,
    );
    builder.add_assertion(
        &format!("OKA-MODEL-READY-{label}"),
        expected_model,
        &state_string(&report.model.state),
        model_ready,
        &report.app_data_dir,
    );
    builder.add_assertion(
        &format!("OKA-MANAGED-MODEL-PATH-{label}"),
        "inside app data",
        &report.model_path,
        report
            .model_path
            .to_lowercase()
            .starts_with(&report.app_data_dir.to_lowercase()),
        &report.model_path,
    );
    builder.add_assertion(
        &format!("OKA-MANAGED-RUNTIME-PATH-{label}"),
        "inside app data",
        &report.runtime.runtime_path,
        report
            .runtime
            .runtime_path
            .to_lowercase()
            .starts_with(&report.app_data_dir.to_lowercase()),
        &report.runtime.runtime_path,
    );

    if let Some(smoke) = report.local_audio_smoke.as_ref() {
        builder.add_assertion(
            &format!("OKA-LOCAL-AUDIO-SMOKE-{label}"),
            "present",
            "present",
            true,
            &smoke.report_json_path.display().to_string(),
        );
        builder.add_assertion(
            "OKA-SMOKE-SEPARATION-PASSED",
            ">= 1",
            &smoke.summary.separation_passed.to_string(),
            smoke.summary.separation_passed >= 1,
            &smoke.report_json_path.display().to_string(),
        );
    }

    if matches!(report.phase, AutomationSmokePhase::Restart) {
        let unexpected_runtime_download =
            has_event(&report.runtime_events, "runtime-bootstrap-progress");
        builder.add_assertion(
            "OKA-284-RUNTIME-COLD-RESTART",
            "no runtime-bootstrap-progress on restart",
            if unexpected_runtime_download {
                "unexpected download"
            } else {
                "no unexpected download"
            },
            !unexpected_runtime_download,
            &report.app_data_dir,
        );

        let unexpected_model_download = has_event(&report.model_events, "model-bootstrap-progress");
        builder.add_assertion(
            "OKA-284-MODEL-COLD-RESTART",
            "no model-bootstrap-progress on restart",
            if unexpected_model_download {
                "unexpected download"
            } else {
                "no unexpected download"
            },
            !unexpected_model_download,
            &report.app_data_dir,
        );
    }
}

fn has_event<T>(events: &[crate::automation_smoke::BootstrapEvent<T>], name: &str) -> bool {
    events.iter().any(|event| event.event == name)
}

fn state_string<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_default()
        .trim_matches('"')
        .to_owned()
}

fn record_oka284_assertions(
    builder: &mut ReportBuilder,
    app_data_dir: &std::path::Path,
    runtime: &RuntimeIdentity,
    model: &ModelIdentity,
) -> anyhow::Result<()> {
    use crate::separator::artifacts::sha256_file;
    use crate::separator::bootstrap::{descriptor_for, managed_model_path_for};
    use crate::separator::catalog::{embedded_catalog, read_installed_identity, resolve_model};
    use crate::separator::runtime_bootstrap::runtime_inventory;
    use crate::separator::verified_manifest::verified_manifest_matches;

    // -----------------------------------------------------------------------
    // Runtime cross-checks
    // -----------------------------------------------------------------------
    let inventory = runtime_inventory(app_data_dir);
    let active = inventory
        .active
        .as_ref()
        .context("no active runtime installed for OKA-284 cross-check")?;

    let record = &active.record;

    let catalog = embedded_catalog();
    let catalog_runtime = catalog
        .manifest
        .artifacts
        .runtimes
        .iter()
        .find(|r| r.artifact_id == record.artifact_id)
        .with_context(|| format!("no catalog runtime for artifact {}", record.artifact_id))?;

    builder.add_assertion(
        "OKA-284-RUNTIME-ARTIFACT-ID",
        &catalog_runtime.artifact_id,
        &record.artifact_id,
        catalog_runtime.artifact_id == record.artifact_id,
        "",
    );
    builder.add_assertion(
        "OKA-284-RUNTIME-CATALOG-SCHEMA",
        &catalog.manifest.schema_version,
        &record.catalog_schema,
        catalog.manifest.schema_version == record.catalog_schema,
        "",
    );
    builder.add_assertion(
        "OKA-284-RUNTIME-GENERATION",
        &catalog.generation.to_string(),
        &record.generation.to_string(),
        catalog.generation == record.generation,
        "",
    );
    builder.add_assertion(
        "OKA-284-RUNTIME-RELEASE-ID",
        &catalog.release_id,
        &record.release_id,
        catalog.release_id == record.release_id,
        "",
    );
    builder.add_assertion(
        "OKA-284-RUNTIME-ARCHIVE-DIGEST",
        &catalog_runtime.archive_digest,
        &record.archive_sha256,
        catalog_runtime.archive_digest == record.archive_sha256,
        "",
    );
    builder.add_assertion(
        "OKA-284-RUNTIME-ARCHIVE-REPORT-CONSISTENCY",
        &runtime.archive_sha256,
        &record.archive_sha256,
        runtime.archive_sha256 == record.archive_sha256,
        "",
    );

    let library_name = active
        .library_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("<unknown>");
    let actual_library_digest = sha256_file(&active.library_path).with_context(|| {
        format!(
            "failed to hash runtime library {}",
            active.library_path.display()
        )
    })?;

    builder.add_assertion(
        "OKA-284-RUNTIME-LIBRARY-REPORT-CONSISTENCY",
        &runtime.extracted_library_sha256,
        &actual_library_digest,
        runtime.extracted_library_sha256 == actual_library_digest,
        &active.library_path.display().to_string(),
    );

    let catalog_library_digest = catalog_runtime
        .extracted_file_digests
        .get(library_name)
        .map(|d| d.sha256.as_str())
        .unwrap_or("<missing in catalog>");
    let record_library_digest = record
        .files
        .iter()
        .find(|f| f.path == library_name)
        .map(|f| f.sha256.as_str())
        .unwrap_or("<missing in record>");

    // actual vs catalog
    builder.add_assertion(
        "OKA-284-RUNTIME-FILE-DIGEST-CATALOG",
        catalog_library_digest,
        &actual_library_digest,
        catalog_library_digest == actual_library_digest,
        &active.library_path.display().to_string(),
    );
    // actual vs record
    builder.add_assertion(
        "OKA-284-RUNTIME-FILE-DIGEST",
        record_library_digest,
        &actual_library_digest,
        record_library_digest == actual_library_digest,
        &active.library_path.display().to_string(),
    );
    // record vs catalog
    builder.add_assertion(
        "OKA-284-RUNTIME-FILE-DIGEST-CROSS",
        catalog_library_digest,
        record_library_digest,
        catalog_library_digest == record_library_digest,
        &active.library_path.display().to_string(),
    );

    let active_library_name = library_name.to_owned();
    for file in &record.files {
        if file.path == active_library_name {
            continue;
        }
        let file_path = active.dir.join(&file.path);
        let catalog_digest = catalog_runtime
            .extracted_file_digests
            .get(&file.path)
            .map(|d| d.sha256.as_str());
        let expected = match catalog_digest {
            Some(d) if d == file.sha256 => file.sha256.clone(),
            Some(d) => format!("record:{} catalog:{}", file.sha256, d),
            None => file.sha256.clone(),
        };
        let (observed, pass) = if file_path.exists() {
            match sha256_file(&file_path) {
                Ok(digest) => {
                    let ok = digest == file.sha256
                        && catalog_runtime
                            .extracted_file_digests
                            .get(&file.path)
                            .map_or(true, |d| d.sha256 == digest);
                    (digest, ok)
                }
                Err(_) => (String::new(), false),
            }
        } else {
            ("missing".to_owned(), false)
        };
        builder.add_assertion(
            &format!("OKA-284-RUNTIME-COMPANION-{}", file.path),
            &expected,
            &observed,
            pass,
            &file_path.display().to_string(),
        );
    }

    // -----------------------------------------------------------------------
    // Model cross-checks
    // -----------------------------------------------------------------------
    let variant = crate::config::load_config(app_data_dir)
        .ok()
        .flatten()
        .map(|c| c.effective_model_variant())
        .unwrap_or_default();
    let descriptor = descriptor_for(variant);
    let model_path = managed_model_path_for(app_data_dir, descriptor);

    let record = read_installed_identity(&model_path).with_context(|| {
        format!(
            "model identity is missing or invalid at {}",
            model_path.display()
        )
    })?;
    let catalog_model = resolve_model(&catalog.manifest, variant)
        .with_context(|| format!("no catalog model for variant {}", variant.as_str()))?;

    let actual_onnx_digest = sha256_file(&model_path)
        .with_context(|| format!("failed to hash model {}", model_path.display()))?;

    builder.add_assertion(
        "OKA-284-MODEL-ONNX-REPORT-CONSISTENCY",
        &model.extracted_onnx_sha256,
        &actual_onnx_digest,
        model.extracted_onnx_sha256 == actual_onnx_digest,
        &model_path.display().to_string(),
    );

    builder.add_assertion(
        "OKA-284-MODEL-ARTIFACT-ID",
        &catalog_model.artifact_id,
        &record.artifact_id,
        catalog_model.artifact_id == record.artifact_id,
        &model_path.display().to_string(),
    );
    builder.add_assertion(
        "OKA-284-MODEL-CATALOG-SCHEMA",
        &catalog.manifest.schema_version,
        &record.catalog_schema,
        catalog.manifest.schema_version == record.catalog_schema,
        &model_path.display().to_string(),
    );
    builder.add_assertion(
        "OKA-284-MODEL-GENERATION",
        &catalog.generation.to_string(),
        &record.generation.to_string(),
        catalog.generation == record.generation,
        &model_path.display().to_string(),
    );
    builder.add_assertion(
        "OKA-284-MODEL-RELEASE-ID",
        &catalog.release_id,
        &record.release_id,
        catalog.release_id == record.release_id,
        &model_path.display().to_string(),
    );

    let expected_archive = format!(
        "catalog:{} descriptor:{}",
        catalog_model.archive_digest, descriptor.download_sha256
    );
    builder.add_assertion(
        "OKA-284-MODEL-ARCHIVE-DIGEST",
        &expected_archive,
        &record.archive_sha256,
        catalog_model.archive_digest == record.archive_sha256
            && descriptor.download_sha256 == record.archive_sha256,
        &model_path.display().to_string(),
    );
    builder.add_assertion(
        "OKA-284-MODEL-ARCHIVE-REPORT-CONSISTENCY",
        &model.archive_sha256,
        &record.archive_sha256,
        model.archive_sha256 == record.archive_sha256,
        &model_path.display().to_string(),
    );

    let (primary_path, primary_digest) = catalog_model.primary_model_file()?;
    let record_file_digest = record
        .files
        .iter()
        .find(|f| f.path == primary_path || f.path == descriptor.filename)
        .map(|f| f.sha256.as_str())
        .unwrap_or("<missing in record>");
    let expected_file = format!(
        "catalog:{} descriptor:{} record:{}",
        primary_digest.sha256, descriptor.file_sha256, record_file_digest
    );
    let file_pass = primary_digest.sha256 == actual_onnx_digest
        && descriptor.file_sha256 == actual_onnx_digest
        && record_file_digest == actual_onnx_digest;
    let record_present = record_file_digest != "<missing in record>";
    builder.add_assertion(
        "OKA-284-MODEL-FILE-DIGEST",
        &expected_file,
        &actual_onnx_digest,
        file_pass && record_present,
        &model_path.display().to_string(),
    );

    let manifest_ok = verified_manifest_matches(&model_path, &actual_onnx_digest)
        .with_context(|| format!("failed to verify manifest for {}", model_path.display()))?;
    let manifest_path = crate::separator::verified_manifest::verified_manifest_path(&model_path)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    builder.add_assertion(
        "OKA-284-MODEL-VERIFICATION-MANIFEST",
        "true",
        &manifest_ok.to_string(),
        manifest_ok,
        &manifest_path,
    );

    Ok(())
}

fn compute_runtime_identity(app_data_dir: &Path) -> Result<RuntimeIdentity> {
    use crate::separator::catalog::read_artifact_record;
    use crate::separator::runtime_bootstrap::runtime_inventory;

    let inventory = runtime_inventory(app_data_dir);
    let active = inventory
        .active
        .as_ref()
        .context("no managed runtime is installed")?;

    let record_path = active
        .library_path
        .parent()
        .context("runtime has no parent directory")?
        .join("record.json");
    let record =
        read_artifact_record(&record_path).context("runtime record.json is missing or invalid")?;

    let active_library_name = active
        .library_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let library_record = record
        .files
        .iter()
        .find(|f| f.path == active_library_name)
        .context("runtime record does not list the active library file")?;

    let library_digest = crate::separator::artifacts::sha256_file(&active.library_path)
        .with_context(|| {
            format!(
                "failed to hash runtime library {}",
                active.library_path.display()
            )
        })?;

    let companion_digests: Vec<String> = record
        .files
        .iter()
        .filter(|f| f.path != library_record.path)
        .map(|f| {
            let path = active
                .library_path
                .parent()
                .unwrap_or(active.library_path.as_ref())
                .join(&f.path);
            if path.exists() {
                crate::separator::artifacts::sha256_file(&path).unwrap_or_default()
            } else {
                String::new()
            }
        })
        .collect();

    Ok(RuntimeIdentity {
        archive_sha256: record.archive_sha256,
        extracted_library_sha256: library_digest,
        companion_dll_sha256s: companion_digests,
    })
}

fn compute_model_identity(app_data_dir: &Path) -> Result<ModelIdentity> {
    use crate::separator::catalog::read_installed_identity;
    use crate::separator::verified_manifest::verified_manifest_path;

    let active_variant = crate::config::load_config(app_data_dir)
        .ok()
        .flatten()
        .map(|c| c.effective_model_variant())
        .unwrap_or_default();
    let descriptor = crate::separator::bootstrap::descriptor_for(active_variant);
    let model_path = crate::separator::bootstrap::managed_model_path_for(app_data_dir, descriptor);

    let record = read_installed_identity(&model_path)
        .context("model identity record is missing or invalid")?;
    let onnx_digest = crate::separator::artifacts::sha256_file(&model_path)
        .with_context(|| format!("failed to hash model {}", model_path.display()))?;

    let manifest_path = verified_manifest_path(&model_path)
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    Ok(ModelIdentity {
        archive_sha256: record.archive_sha256,
        extracted_onnx_sha256: onnx_digest,
        verification_manifest: manifest_path,
        catalog_generation: record.generation.to_string(),
        release_id: record.release_id,
        artifact_id: record.artifact_id,
        selected_variant: active_variant.as_str().to_owned(),
    })
}

fn build_audio_summary(smoke: &crate::smoke::LocalAudioSmokeReport) -> AudioSummary {
    let song = smoke
        .songs
        .iter()
        .find(|s| s.separation_status == crate::smoke::SmokeStepStatus::Passed)
        .and_then(|s| s.vocals_path.as_ref().map(|p| (s, p)));

    let mut summary = AudioSummary {
        sample_rate: 0,
        channel_count: 0,
        non_silent_samples: false,
        input_duration_seconds: None,
        output_duration_seconds: None,
        duration_delta_seconds: None,
        vocals_path: None,
        accompaniment_path: None,
    };

    if let Some((song, vocals_path)) = song {
        summary.vocals_path = Some(vocals_path.clone());
        summary.accompaniment_path = song.accompaniment_path.clone();
        if let Ok(info) = read_wav_header(vocals_path) {
            summary.sample_rate = i64::from(info.sample_rate);
            summary.channel_count = i64::from(info.channels);
            summary.non_silent_samples = info.has_non_silent;
            summary.output_duration_seconds = Some(info.duration_seconds);
        }
    }

    summary
}

#[derive(Debug, Default)]
struct WavHeaderInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_seconds: f64,
    pub has_non_silent: bool,
}

fn read_wav_header(path: &str) -> Result<WavHeaderInfo> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).with_context(|| format!("failed to open {path}"))?;
    let mut header = [0u8; 12];
    file.read_exact(&mut header)?;

    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        bail!("{path}: not a RIFF/WAVE file");
    }

    let mut fmt = None;
    let mut data_size: u64 = 0;
    let mut bytes_per_sample = 0u16;

    loop {
        let mut chunk_id = [0u8; 4];
        if file.read_exact(&mut chunk_id).is_err() {
            break;
        }
        let mut chunk_size_buf = [0u8; 4];
        file.read_exact(&mut chunk_size_buf)?;
        let chunk_size = u32::from_le_bytes(chunk_size_buf) as u64;

        if &chunk_id == b"fmt " {
            let mut fmt_buf = [0u8; 16];
            file.read_exact(&mut fmt_buf)?;
            let format_tag = u16::from_le_bytes([fmt_buf[0], fmt_buf[1]]);
            let channels = u16::from_le_bytes([fmt_buf[2], fmt_buf[3]]);
            let sample_rate = u32::from_le_bytes([fmt_buf[4], fmt_buf[5], fmt_buf[6], fmt_buf[7]]);
            let block_align = u16::from_le_bytes([fmt_buf[12], fmt_buf[13]]);
            let bits_per_sample = u16::from_le_bytes([fmt_buf[14], fmt_buf[15]]);

            bytes_per_sample = bits_per_sample / 8 * channels;
            if bytes_per_sample == 0 {
                bytes_per_sample = block_align;
            }
            fmt = Some((channels, sample_rate, bits_per_sample, format_tag));
            if chunk_size > 16 {
                file.seek(SeekFrom::Current((chunk_size - 16) as i64))?;
            }
        } else if &chunk_id == b"data" {
            data_size = chunk_size;
            break;
        } else {
            file.seek(SeekFrom::Current(chunk_size as i64))?;
        }
    }

    let (channels, sample_rate, bits_per_sample, format_tag) =
        fmt.context("missing fmt chunk in WAV")?;

    if bytes_per_sample == 0 || sample_rate == 0 {
        bail!("invalid WAV header");
    }

    let total_samples = data_size / u64::from(bytes_per_sample);
    let duration_seconds = total_samples as f64 / f64::from(sample_rate);

    let mut has_non_silent = false;
    let silence_threshold: f64 = 1e-4;
    let sample_count = (data_size / u64::from(bytes_per_sample / channels)).min(4096) as usize;
    for _ in 0..sample_count {
        let mut sample_buf = vec![0u8; (bits_per_sample / 8) as usize];
        if file.read_exact(&mut sample_buf).is_err() {
            break;
        }
        let value = match (format_tag, bits_per_sample) {
            (1, 16) => {
                let raw = i16::from_le_bytes([sample_buf[0], sample_buf[1]]) as f64 / 32768.0;
                raw
            }
            (3, 32) => {
                f32::from_le_bytes([sample_buf[0], sample_buf[1], sample_buf[2], sample_buf[3]])
                    as f64
            }
            _ => 0.0,
        };
        if value.abs() > silence_threshold {
            has_non_silent = true;
            break;
        }
    }

    Ok(WavHeaderInfo {
        sample_rate,
        channels,
        duration_seconds,
        has_non_silent,
    })
}

struct ReportBuilder {
    scenario: String,
    started_at: i64,
    application: ApplicationIdentity,
    steps: Vec<Step>,
    assertions: Vec<Assertion>,
    artifacts: Vec<Artifact>,
    errors: Vec<ReportError>,
    runtime: RuntimeIdentity,
    model: ModelIdentity,
    database: DatabaseSummary,
    audio: AudioSummary,
}

impl Default for RuntimeIdentity {
    fn default() -> Self {
        Self {
            archive_sha256: String::new(),
            extracted_library_sha256: String::new(),
            companion_dll_sha256s: Vec::new(),
        }
    }
}

impl Default for ModelIdentity {
    fn default() -> Self {
        Self {
            archive_sha256: String::new(),
            extracted_onnx_sha256: String::new(),
            verification_manifest: String::new(),
            catalog_generation: String::new(),
            release_id: String::new(),
            artifact_id: String::new(),
            selected_variant: String::new(),
        }
    }
}

impl AudioSummary {
    fn default() -> Self {
        Self {
            sample_rate: 0,
            channel_count: 0,
            non_silent_samples: false,
            input_duration_seconds: None,
            output_duration_seconds: None,
            duration_delta_seconds: None,
            vocals_path: None,
            accompaniment_path: None,
        }
    }
}

impl ReportBuilder {
    fn new(scenario: &str) -> Self {
        Self {
            scenario: scenario.into(),
            started_at: unix_timestamp(),
            application: ApplicationIdentity {
                name: env!("CARGO_PKG_NAME").into(),
                version: env!("CARGO_PKG_VERSION").into(),
                commit_sha: commit_sha(),
            },
            steps: Vec::new(),
            assertions: Vec::new(),
            artifacts: Vec::new(),
            errors: Vec::new(),
            runtime: RuntimeIdentity::default(),
            model: ModelIdentity::default(),
            database: DatabaseSummary {
                schema_version: 0,
                path: String::new(),
            },
            audio: AudioSummary::default(),
        }
    }

    fn begin_step(&mut self, id: &str, name: &str) -> (String, String, i64) {
        (id.into(), name.into(), unix_timestamp())
    }

    fn end_step(&mut self, step: (String, String, i64), status: StepStatus, error: Option<String>) {
        let finished_at = unix_timestamp();
        self.steps.push(Step {
            id: step.0,
            name: step.1,
            status,
            started_at: step.2,
            finished_at,
            duration_ms: finished_at - step.2,
            output: None,
            error,
        });
    }

    fn add_assertion(
        &mut self,
        id: &str,
        expected: &str,
        observed: &str,
        pass: bool,
        artifact_path: &str,
    ) {
        self.assertions.push(Assertion {
            id: id.into(),
            expected: expected.into(),
            observed: observed.into(),
            result: if pass {
                AssertionResult::Pass
            } else {
                AssertionResult::Fail
            },
            artifact_path: artifact_path.into(),
        });
        if !pass {
            self.errors.push(ReportError {
                message: format!("assertion {id} failed: expected {expected}, observed {observed}"),
                step_id: Some(self.steps.last().map(|s| s.id.clone()).unwrap_or_default()),
                assertion_id: Some(id.into()),
            });
        }
    }

    fn add_artifact(&mut self, path: &Path, kind: &str, description: Option<String>) {
        self.artifacts.push(Artifact {
            path: path.display().to_string(),
            kind: kind.into(),
            description,
        });
    }

    fn add_error(&mut self, message: &str, step_id: Option<String>, assertion_id: Option<String>) {
        self.errors.push(ReportError {
            message: message.into(),
            step_id,
            assertion_id,
        });
    }

    fn finish(
        self,
        execution_provider: &Option<String>,
        locale: &Option<String>,
        theme: &Option<String>,
    ) -> AutomationReport {
        let finished_at = unix_timestamp();
        let status = if self.errors.is_empty() {
            ReportStatus::Passed
        } else {
            ReportStatus::Failed
        };

        AutomationReport {
            scenario: self.scenario,
            status,
            started_at: self.started_at,
            finished_at,
            duration_ms: finished_at - self.started_at,
            application: self.application,
            environment: Environment {
                os_version: os_version(),
                webview2_version: webview2_version(),
                selected_execution_provider: execution_provider.clone().unwrap_or_else(|| {
                    crate::config::ExecutionProviderPreference::default_for_current_platform()
                        .as_str()
                        .to_owned()
                }),
                locale: locale.clone(),
                theme: theme.clone(),
            },
            steps: self.steps,
            assertions: self.assertions,
            artifacts: self.artifacts,
            runtime: self.runtime,
            model: self.model,
            database: self.database,
            accessibility: AccessibilitySummary {
                violations_count: 0,
                keyboard_trap_count: 0,
                ui_automation_errors_count: 0,
                zoom_levels_tested: None,
            },
            audio: self.audio,
            errors: self.errors,
        }
    }
}

fn commit_sha() -> String {
    if let Ok(sha) = std::env::var("OPENKARA_COMMIT_SHA") {
        return sha;
    }
    if let Ok(output) = Command::new("git").args(["rev-parse", "HEAD"]).output() {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_owned();
        }
    }
    "unknown".into()
}

fn os_version() -> String {
    if let Ok(version) = std::env::var("OPENKARA_OS_VERSION") {
        return version;
    }

    let fallback = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);

    #[cfg(target_os = "macos")]
    if let Ok(output) = Command::new("sw_vers").arg("-productVersion").output() {
        if output.status.success() {
            return format!("macOS {}", String::from_utf8_lossy(&output.stdout).trim());
        }
    }

    #[cfg(target_os = "linux")]
    if let Ok(output) = Command::new("uname").args(["-r"]).output() {
        if output.status.success() {
            return format!(
                "Linux kernel {}",
                String::from_utf8_lossy(&output.stdout).trim()
            );
        }
    }

    #[cfg(target_os = "windows")]
    if let Ok(output) = Command::new("cmd").args(["/c", "ver"]).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        if output.status.success() {
            return text.trim().to_owned();
        }
    }

    fallback
}

fn webview2_version() -> String {
    if let Ok(version) = std::env::var("OPENKARA_WEBVIEW2_VERSION") {
        return version;
    }

    #[cfg(target_os = "windows")]
    {
        // WebView2 version is stored in the registry under the Microsoft Edge Update client key.
        // Reading it via reg.exe avoids a new dependency on the winreg crate.
        let key = r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00D3D05741D2}";
        if let Ok(output) = Command::new("reg")
            .args(["query", key, "/v", "pv"])
            .output()
        {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if let Some(pos) = line.find("REG_SZ") {
                    return line[pos + 6..].trim().to_owned();
                }
            }
        }
    }

    "n/a".into()
}
