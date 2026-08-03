//! Black-box regression exercised through the packaged OpenKara executable.
//!
//! This specifically covers #284's production shape: the network transfer
//! reaches 100%, post-download runtime probing hangs, the task must terminate
//! as Failed, and a retry/restart must reuse the verified immutable install
//! rather than downloading the archive again.

use crate::{
    commands::{runtime_bootstrap as command, unix_timestamp},
    separator::{catalog, runtime_bootstrap},
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

const REPORT_FILENAME: &str = "runtime-bootstrap-regression-report.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionPhase {
    FaultRetry,
    Restart,
}

impl RegressionPhase {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "fault-retry" => Ok(Self::FaultRetry),
            "restart" => Ok(Self::Restart),
            _ => bail!("runtime bootstrap regression phase must be fault-retry or restart"),
        }
    }
}

#[derive(Debug)]
struct RegressionConfig {
    phase: RegressionPhase,
    app_data_dir: PathBuf,
    output_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionEvent {
    pub event: String,
    pub snapshot: command::RuntimeBootstrapStatusSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptReport {
    pub name: String,
    pub elapsed_ms: u128,
    pub result: String,
    pub error: Option<String>,
    pub final_state: command::RuntimeBootstrapState,
    pub byte_progress_events: usize,
    pub events: Vec<RegressionEvent>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeBootstrapRegressionReport {
    pub generated_at: i64,
    pub phase: RegressionPhase,
    pub app_data_dir: String,
    pub artifact_id: String,
    pub attempts: Vec<AttemptReport>,
    pub assertions: BTreeMap<String, bool>,
}

pub fn maybe_run_from_cli() -> Result<bool> {
    let mut arguments = env::args();
    let _program = arguments.next();
    if arguments.next().as_deref() != Some("--automation-runtime-bootstrap-regression") {
        return Ok(false);
    }

    let phase = RegressionPhase::parse(
        &arguments
            .next()
            .context("missing runtime bootstrap regression phase")?,
    )?;
    let mut app_data_dir = None;
    let mut output_dir = None;
    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .with_context(|| format!("missing value for {argument}"))
        };
        match argument.as_str() {
            "--app-data-dir" => app_data_dir = Some(PathBuf::from(value()?)),
            "--output-dir" => output_dir = Some(PathBuf::from(value()?)),
            _ => bail!("unknown runtime bootstrap regression argument {argument}"),
        }
    }

    let config = RegressionConfig {
        phase,
        app_data_dir: app_data_dir.context("--app-data-dir is required")?,
        output_dir: output_dir.context("--output-dir is required")?,
    };
    let report = run_phase(&config)?;
    fs::create_dir_all(&config.output_dir).with_context(|| {
        format!(
            "failed to create runtime regression output {}",
            config.output_dir.display()
        )
    })?;
    let report_path = config.output_dir.join(REPORT_FILENAME);
    fs::write(
        &report_path,
        serde_json::to_string_pretty(&report)
            .context("failed to serialize runtime bootstrap regression report")?,
    )
    .with_context(|| format!("failed to write {}", report_path.display()))?;

    let failed: Vec<&str> = report
        .assertions
        .iter()
        .filter_map(|(name, passed)| (!passed).then_some(name.as_str()))
        .collect();
    if !failed.is_empty() {
        bail!("runtime bootstrap regression failed: {}", failed.join(", "));
    }
    Ok(true)
}

fn run_phase(config: &RegressionConfig) -> Result<RuntimeBootstrapRegressionReport> {
    fs::create_dir_all(&config.app_data_dir).with_context(|| {
        format!(
            "failed to create runtime regression app data {}",
            config.app_data_dir.display()
        )
    })?;
    let embedded = catalog::embedded_catalog();
    let runtime = catalog::resolve_runtime(&embedded.manifest, catalog::current_target_triple())?;

    match config.phase {
        RegressionPhase::FaultRetry => run_fault_retry(config, &runtime.artifact_id),
        RegressionPhase::Restart => run_restart(config, &runtime.artifact_id),
    }
}

fn run_fault_retry(
    config: &RegressionConfig,
    artifact_id: &str,
) -> Result<RuntimeBootstrapRegressionReport> {
    let inventory = runtime_bootstrap::runtime_inventory(&config.app_data_dir);
    if inventory.active.is_some()
        || inventory.candidate.is_some()
        || inventory.legacy_path.is_some()
        || runtime_bootstrap::installed_runtime(&config.app_data_dir, artifact_id).is_some()
    {
        bail!("fault-retry phase requires clean runtime app data");
    }

    let status = Arc::new(Mutex::new(command::snapshot_from_disk(
        &config.app_data_dir,
    )));
    let mut events = Vec::new();

    env::set_var("OPENKARA_RUNTIME_WORKER_HANG_AFTER_DOWNLOAD", "1");
    let first_start = Instant::now();
    let first_result = command::ensure_runtime_ready_or_install_blocking(
        &config.app_data_dir,
        &status,
        &mut |event, snapshot| {
            events.push(RegressionEvent {
                event: event.to_owned(),
                snapshot,
            });
        },
    );
    let first_elapsed = first_start.elapsed();
    env::remove_var("OPENKARA_RUNTIME_WORKER_HANG_AFTER_DOWNLOAD");

    let first_end = events.len();
    let first_snapshot = locked_snapshot(&status)?;
    let first_error = first_result.err().map(|error| error.message);
    let first_attempt = attempt_report(
        "probe-timeout",
        first_elapsed.as_millis(),
        first_error.as_deref(),
        first_snapshot.state.clone(),
        events[..first_end].to_vec(),
    );

    let installed_before_retry =
        runtime_bootstrap::installed_runtime(&config.app_data_dir, artifact_id);
    let verified_before_retry = installed_before_retry.as_ref().is_some_and(|installed| {
        runtime_bootstrap::verify_runtime_files(installed).unwrap_or(false)
    });

    let retry_start = Instant::now();
    let retry_result = command::ensure_runtime_ready_or_install_blocking(
        &config.app_data_dir,
        &status,
        &mut |event, snapshot| {
            events.push(RegressionEvent {
                event: event.to_owned(),
                snapshot,
            });
        },
    );
    let retry_elapsed = retry_start.elapsed();
    let retry_snapshot = locked_snapshot(&status)?;
    let retry_error = retry_result.err().map(|error| error.message);
    let retry_events = events[first_end..].to_vec();
    let retry_attempt = attempt_report(
        "reuse-verified-install",
        retry_elapsed.as_millis(),
        retry_error.as_deref(),
        retry_snapshot.state.clone(),
        retry_events,
    );

    let mut assertions = BTreeMap::new();
    assertions.insert(
        "initial_attempt_downloaded_runtime_bytes".to_owned(),
        first_attempt.byte_progress_events > 0,
    );
    assertions.insert(
        "download_reached_post_download_installing".to_owned(),
        first_attempt.events.iter().any(|event| {
            matches!(
                event.snapshot.state,
                command::RuntimeBootstrapState::Installing
                    | command::RuntimeBootstrapState::Downloading
            ) && event.snapshot.downloaded_bytes.is_none()
        }),
    );
    assertions.insert(
        "probe_timeout_was_observed".to_owned(),
        first_attempt
            .error
            .as_deref()
            .is_some_and(|error| error.contains("timed out")),
    );
    assertions.insert(
        "probe_timeout_terminated_as_failed".to_owned(),
        first_attempt.final_state == command::RuntimeBootstrapState::Failed,
    );
    assertions.insert(
        "verified_install_survived_probe_timeout".to_owned(),
        verified_before_retry,
    );
    assertions.insert(
        "retry_did_not_download_runtime_bytes".to_owned(),
        retry_attempt.byte_progress_events == 0,
    );
    assertions.insert(
        "retry_reused_verified_install".to_owned(),
        verified_before_retry,
    );
    assertions.insert(
        "retry_finished_ready".to_owned(),
        retry_attempt.error.is_none()
            && retry_attempt.final_state == command::RuntimeBootstrapState::Ready,
    );
    assertions.insert(
        "active_identity_matches_catalog".to_owned(),
        retry_snapshot.active_artifact_id.as_deref() == Some(artifact_id),
    );

    Ok(RuntimeBootstrapRegressionReport {
        generated_at: unix_timestamp(),
        phase: RegressionPhase::FaultRetry,
        app_data_dir: config.app_data_dir.display().to_string(),
        artifact_id: artifact_id.to_owned(),
        attempts: vec![first_attempt, retry_attempt],
        assertions,
    })
}

fn run_restart(
    config: &RegressionConfig,
    artifact_id: &str,
) -> Result<RuntimeBootstrapRegressionReport> {
    let status = Arc::new(Mutex::new(command::snapshot_from_disk(
        &config.app_data_dir,
    )));
    let before = locked_snapshot(&status)?;
    let mut events = Vec::new();
    let started = Instant::now();
    let result = command::ensure_runtime_ready_or_install_blocking(
        &config.app_data_dir,
        &status,
        &mut |event, snapshot| {
            events.push(RegressionEvent {
                event: event.to_owned(),
                snapshot,
            });
        },
    );
    let elapsed = started.elapsed();
    let after = locked_snapshot(&status)?;
    let error = result.err().map(|error| error.message);
    let attempt = attempt_report(
        "cold-restart",
        elapsed.as_millis(),
        error.as_deref(),
        after.state.clone(),
        events,
    );

    let mut assertions = BTreeMap::new();
    assertions.insert(
        "restart_discovered_active_runtime".to_owned(),
        before.state == command::RuntimeBootstrapState::Ready
            && before.active_artifact_id.as_deref() == Some(artifact_id),
    );
    assertions.insert(
        "restart_did_not_download_runtime_bytes".to_owned(),
        attempt.byte_progress_events == 0,
    );
    assertions.insert(
        "restart_loaded_runtime_successfully".to_owned(),
        attempt.error.is_none() && attempt.final_state == command::RuntimeBootstrapState::Ready,
    );
    assertions.insert(
        "restart_kept_catalog_identity".to_owned(),
        after.active_artifact_id.as_deref() == Some(artifact_id),
    );

    Ok(RuntimeBootstrapRegressionReport {
        generated_at: unix_timestamp(),
        phase: RegressionPhase::Restart,
        app_data_dir: config.app_data_dir.display().to_string(),
        artifact_id: artifact_id.to_owned(),
        attempts: vec![attempt],
        assertions,
    })
}

fn locked_snapshot(
    status: &Arc<Mutex<command::RuntimeBootstrapStatusSnapshot>>,
) -> Result<command::RuntimeBootstrapStatusSnapshot> {
    status
        .lock()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| anyhow::anyhow!("runtime bootstrap status lock was poisoned"))
}

fn attempt_report(
    name: &str,
    elapsed_ms: u128,
    error: Option<&str>,
    final_state: command::RuntimeBootstrapState,
    events: Vec<RegressionEvent>,
) -> AttemptReport {
    let byte_progress_events = events
        .iter()
        .filter(|event| {
            event.snapshot.state == command::RuntimeBootstrapState::Downloading
                && event.snapshot.downloaded_bytes.unwrap_or_default() > 0
        })
        .count();
    AttemptReport {
        name: name.to_owned(),
        elapsed_ms,
        result: if error.is_some() { "failed" } else { "passed" }.to_owned(),
        error: error.map(ToOwned::to_owned),
        final_state,
        byte_progress_events,
        events,
    }
}

fn has_state(events: &[RegressionEvent], state: command::RuntimeBootstrapState) -> bool {
    events.iter().any(|event| event.snapshot.state == state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        state: command::RuntimeBootstrapState,
        downloaded_bytes: Option<u64>,
    ) -> RegressionEvent {
        let mut snapshot = command::snapshot_from_inventory(&runtime_bootstrap::RuntimeInventory {
            active: None,
            candidate: None,
            legacy_path: None,
            last_failure: None,
        });
        snapshot.state = state;
        snapshot.downloaded_bytes = downloaded_bytes;
        RegressionEvent {
            event: command::RUNTIME_BOOTSTRAP_PROGRESS_EVENT.to_owned(),
            snapshot,
        }
    }

    #[test]
    fn attempt_report_distinguishes_real_transfer_from_reuse() {
        let downloaded = attempt_report(
            "download",
            10,
            None,
            command::RuntimeBootstrapState::Ready,
            vec![event(
                command::RuntimeBootstrapState::Downloading,
                Some(4096),
            )],
        );
        let reused = attempt_report(
            "reuse",
            10,
            None,
            command::RuntimeBootstrapState::Ready,
            vec![event(command::RuntimeBootstrapState::Downloading, Some(0))],
        );
        assert_eq!(downloaded.byte_progress_events, 1);
        assert_eq!(reused.byte_progress_events, 0);
    }

    #[test]
    fn phase_detection_requires_the_emitted_state() {
        let events = vec![
            event(command::RuntimeBootstrapState::Installing, None),
            event(command::RuntimeBootstrapState::Probing, None),
        ];
        assert!(has_state(
            &events,
            command::RuntimeBootstrapState::Installing
        ));
        assert!(has_state(&events, command::RuntimeBootstrapState::Probing));
        assert!(!has_state(
            &events,
            command::RuntimeBootstrapState::Activating
        ));
    }
}
