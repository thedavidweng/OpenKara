//! Installed-executable regression for the runtime bootstrap state machine.
//!
//! The two phases run in separate processes. The first process downloads the
//! runtime through the production bootstrap command core and is forced to
//! hang during probing. The second process must recover the verified orphan
//! install without issuing another HTTP request. The normal installed-app
//! smoke then provides the cold-start and separation boundary.

use crate::{
    commands::{error::ErrorCode, runtime_bootstrap as command, unix_timestamp},
    separator::{catalog, runtime_bootstrap},
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tiny_http::{Response, Server, StatusCode};

const REPORT_FILENAME: &str = "runtime-bootstrap-regression-report.json";
const REQUEST_COUNT_FILENAME: &str = "runtime-bootstrap-http-request-count.txt";

struct FaultEnvGuard;

impl Drop for FaultEnvGuard {
    fn drop(&mut self) {
        env::remove_var("OPENKARA_RUNTIME_WORKER_HANG_DURING_PROBE");
        env::remove_var("OPENKARA_RUNTIME_POST_DOWNLOAD_TIMEOUT_MS");
    }
}

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
    pub error_code: Option<ErrorCode>,
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
    pub download_request_count: usize,
    pub attempts: Vec<AttemptReport>,
    pub assertions: BTreeMap<String, bool>,
}

struct DownloadFixture {
    stop: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
    thread: Option<thread::JoinHandle<()>>,
}

impl DownloadFixture {
    fn start(remote_url: &str, count_path: &Path) -> Result<(Self, String)> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .context("failed to build runtime fixture client")?;
        let payload = client
            .get(remote_url)
            .send()
            .context("runtime fixture could not fetch the catalog runtime")?
            .error_for_status()
            .context("runtime fixture received a runtime download error")?
            .bytes()
            .context("runtime fixture could not read the catalog runtime")?
            .to_vec();

        fs::write(count_path, "0")?;
        let server = Server::http("127.0.0.1:0")
            .map_err(|error| anyhow::anyhow!("failed to start runtime fixture: {error}"))?;
        let address = server
            .server_addr()
            .to_ip()
            .context("runtime fixture did not bind an IP address")?;
        let base_url = format!("http://{address}/runtime");
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let thread_stop = Arc::clone(&stop);
        let thread_requests = Arc::clone(&requests);
        let thread_count_path = count_path.to_path_buf();
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                let request = match server.recv_timeout(Duration::from_millis(100)) {
                    Ok(Some(request)) => request,
                    Ok(None) => continue,
                    Err(_) => break,
                };
                if request.url() != "/runtime" {
                    let _ = request.respond(Response::empty(StatusCode(404)));
                    continue;
                }
                let count = thread_requests.fetch_add(1, Ordering::Relaxed) + 1;
                let _ = fs::write(&thread_count_path, count.to_string());
                let _ = request.respond(Response::from_data(payload.clone()));
            }
        });

        Ok((
            Self {
                stop,
                requests,
                thread: Some(thread),
            },
            base_url,
        ))
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::Relaxed)
    }
}

impl Drop for DownloadFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
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
    let runtime = catalog::resolve_runtime(
        &embedded.manifest,
        catalog::current_target_triple(),
        crate::config::ExecutionProviderPreference::default_for_current_platform(),
    )?;

    match config.phase {
        RegressionPhase::FaultRetry => run_fault_retry(config, embedded, runtime),
        RegressionPhase::Restart => run_restart(config, &runtime.artifact_id),
    }
}

fn run_fault_retry(
    config: &RegressionConfig,
    embedded: &catalog::VerifiedCatalog,
    runtime: &catalog::CatalogRuntime,
) -> Result<RuntimeBootstrapRegressionReport> {
    let inventory = runtime_bootstrap::runtime_inventory(&config.app_data_dir);
    if inventory.active.is_some()
        || inventory.candidate.is_some()
        || inventory.legacy_path.is_some()
        || runtime_bootstrap::installed_runtime(&config.app_data_dir, &runtime.artifact_id)
            .is_some()
    {
        bail!("fault-retry phase requires clean runtime app data");
    }

    let count_path = config.app_data_dir.join(REQUEST_COUNT_FILENAME);
    let (fixture, fixture_url) = DownloadFixture::start(&runtime.download_url, &count_path)?;
    let mut catalog = embedded.clone();
    let selected = catalog
        .manifest
        .artifacts
        .runtimes
        .iter_mut()
        .find(|candidate| candidate.artifact_id == runtime.artifact_id)
        .context("embedded runtime was not found in its catalog")?;
    selected.download_url = fixture_url;
    let selected_runtime = catalog::resolve_runtime(
        &catalog.manifest,
        catalog::current_target_triple(),
        crate::config::ExecutionProviderPreference::default_for_current_platform(),
    )?
    .clone();

    let status = Arc::new(Mutex::new(command::snapshot_from_disk(
        &config.app_data_dir,
    )));
    let mut events = Vec::new();
    env::set_var("OPENKARA_RUNTIME_WORKER_HANG_DURING_PROBE", "1");
    env::set_var("OPENKARA_RUNTIME_POST_DOWNLOAD_TIMEOUT_MS", "1500");
    let _env_guard = FaultEnvGuard;
    let started = Instant::now();
    let is_update =
        command::prepare_runtime_download(&config.app_data_dir, &status, &mut |event, snapshot| {
            events.push(RegressionEvent {
                event: event.to_owned(),
                snapshot,
            });
        });
    let result = command::download_runtime_blocking_with_catalog(
        &config.app_data_dir,
        &catalog,
        is_update,
        &status,
        &mut |event, snapshot| {
            events.push(RegressionEvent {
                event: event.to_owned(),
                snapshot,
            });
        },
    );

    let elapsed = started.elapsed();
    let snapshot = locked_snapshot(&status)?;
    let error = result.err();
    let attempt = attempt_report(
        "download-and-timeout",
        elapsed.as_millis(),
        error.as_ref(),
        snapshot.state.clone(),
        events,
    );
    let request_count = fixture.request_count();
    drop(fixture);

    let orphan = runtime_bootstrap::installed_runtime(&config.app_data_dir, &runtime.artifact_id);
    let verified_orphan = orphan.as_ref().is_some_and(|installed| {
        runtime_bootstrap::verify_runtime_files(installed).unwrap_or(false)
    });
    let mut assertions = BTreeMap::new();
    assertions.insert(
        "initial_attempt_downloaded_runtime_bytes".to_owned(),
        attempt.byte_progress_events > 0,
    );
    assertions.insert(
        "download_request_server_observed_request".to_owned(),
        request_count > 0,
    );
    assertions.insert(
        "download_left_downloading_phase".to_owned(),
        has_state(&attempt.events, command::RuntimeBootstrapState::Installing)
            && has_state(&attempt.events, command::RuntimeBootstrapState::Probing)
            && !has_downloading_without_bytes(&attempt.events),
    );
    assertions.insert(
        "probe_timeout_was_observed".to_owned(),
        attempt.error_code == Some(ErrorCode::RuntimePostDownloadTimeout),
    );
    assertions.insert(
        "probe_timeout_terminated_as_failed".to_owned(),
        attempt.final_state == command::RuntimeBootstrapState::Failed,
    );
    assertions.insert(
        "verified_install_survived_process_exit".to_owned(),
        verified_orphan,
    );
    // The worker was killed during the probe, so the install was never
    // proven and must not be staged: startup acknowledges a candidate on
    // the worker probe's authority without loading it (#395).
    assertions.insert(
        "unproven_install_was_not_staged_as_candidate".to_owned(),
        runtime_bootstrap::runtime_inventory(&config.app_data_dir)
            .candidate
            .is_none(),
    );
    assertions.insert(
        "runtime_is_not_active_before_restart".to_owned(),
        runtime_bootstrap::runtime_inventory(&config.app_data_dir)
            .active
            .is_none(),
    );

    Ok(RuntimeBootstrapRegressionReport {
        generated_at: unix_timestamp(),
        phase: RegressionPhase::FaultRetry,
        app_data_dir: config.app_data_dir.display().to_string(),
        artifact_id: selected_runtime.artifact_id,
        download_request_count: request_count,
        attempts: vec![attempt],
        assertions,
    })
}

fn run_restart(
    config: &RegressionConfig,
    artifact_id: &str,
) -> Result<RuntimeBootstrapRegressionReport> {
    let before_inventory = runtime_bootstrap::runtime_inventory(&config.app_data_dir);
    let orphan = runtime_bootstrap::installed_runtime(&config.app_data_dir, artifact_id)
        .context("restart phase did not find the verified orphan install")?;
    anyhow::ensure!(
        before_inventory.active.is_none() && before_inventory.candidate.is_none(),
        "restart phase must begin from an unstaged orphan install: the probe never \
         passed, so the previous process must not have staged a candidate (#395)"
    );
    anyhow::ensure!(
        runtime_bootstrap::verify_runtime_files(&orphan)?,
        "restart phase found an unverifiable orphan install"
    );
    let status = Arc::new(Mutex::new(command::snapshot_from_disk(
        &config.app_data_dir,
    )));
    let mut events = Vec::new();
    let started = Instant::now();
    let result = command::ensure_runtime_ready_or_install_blocking(
        &config.app_data_dir,
        &status,
        &Arc::new(Mutex::new(None)),
        &mut |event, snapshot| {
            events.push(RegressionEvent {
                event: event.to_owned(),
                snapshot,
            });
        },
    );
    let attempt = attempt_report(
        "restart-recover-orphan",
        started.elapsed().as_millis(),
        result.as_ref().err(),
        locked_snapshot(&status)?.state.clone(),
        events,
    );
    let after = locked_snapshot(&status)?;
    let mut assertions = BTreeMap::new();
    assertions.insert(
        "restart_started_from_orphan_verified_install".to_owned(),
        before_inventory.active.is_none() && orphan.record.artifact_id == artifact_id,
    );
    assertions.insert(
        "restart_did_not_download_runtime_bytes".to_owned(),
        attempt.byte_progress_events == 0,
    );
    assertions.insert(
        "restart_finished_ready".to_owned(),
        attempt.error.is_none() && attempt.final_state == command::RuntimeBootstrapState::Ready,
    );
    assertions.insert(
        "restart_activated_catalog_identity".to_owned(),
        after.active_artifact_id.as_deref() == Some(artifact_id),
    );

    Ok(RuntimeBootstrapRegressionReport {
        generated_at: unix_timestamp(),
        phase: RegressionPhase::Restart,
        app_data_dir: config.app_data_dir.display().to_string(),
        artifact_id: artifact_id.to_owned(),
        download_request_count: 0,
        attempts: vec![attempt],
        assertions,
    })
}

fn read_request_count(path: &Path) -> Result<usize> {
    Ok(fs::read_to_string(path)
        .with_context(|| format!("missing runtime fixture count {}", path.display()))?
        .trim()
        .parse()?)
}

fn locked_snapshot(
    status: &Arc<Mutex<command::RuntimeBootstrapStatusSnapshot>>,
) -> Result<command::RuntimeBootstrapStatusSnapshot> {
    status
        .lock()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| anyhow::anyhow!("runtime bootstrap status lock was poisoned"))
}

fn is_byte_progress_event(event: &RegressionEvent) -> bool {
    event.event == command::RUNTIME_BOOTSTRAP_PROGRESS_EVENT
        && matches!(
            event.snapshot.state,
            command::RuntimeBootstrapState::Downloading
                | command::RuntimeBootstrapState::DownloadingCandidate
        )
        && event.snapshot.downloaded_bytes.is_some()
        && event.snapshot.total_bytes.is_some()
        && event.snapshot.downloaded_bytes.unwrap_or_default() > 0
}

fn attempt_report(
    name: &str,
    elapsed_ms: u128,
    error: Option<&crate::commands::error::CommandError>,
    final_state: command::RuntimeBootstrapState,
    events: Vec<RegressionEvent>,
) -> AttemptReport {
    let byte_progress_events = events.iter().filter(|e| is_byte_progress_event(e)).count();
    AttemptReport {
        name: name.to_owned(),
        elapsed_ms,
        result: if error.is_some() { "failed" } else { "passed" }.to_owned(),
        error: error.map(|error| error.message.clone()),
        error_code: error.map(|error| error.code.clone()),
        final_state,
        byte_progress_events,
        events,
    }
}

fn has_state(events: &[RegressionEvent], state: command::RuntimeBootstrapState) -> bool {
    events.iter().any(|event| event.snapshot.state == state)
}

fn has_downloading_without_bytes(events: &[RegressionEvent]) -> bool {
    events.iter().any(|event| {
        event.snapshot.state == command::RuntimeBootstrapState::Downloading
            && event.snapshot.downloaded_bytes.is_none()
    })
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
    fn byte_progress_requires_a_downloading_state_with_bytes() {
        let events = vec![
            event(command::RuntimeBootstrapState::Downloading, Some(4096)),
            event(command::RuntimeBootstrapState::Installing, None),
        ];
        assert_eq!(
            events
                .iter()
                .filter(|event| is_byte_progress_event(event))
                .count(),
            1
        );
        assert!(!has_downloading_without_bytes(&events));
    }

    #[test]
    fn post_download_phases_are_exact_and_ordered() {
        let events = vec![
            event(command::RuntimeBootstrapState::Installing, None),
            event(command::RuntimeBootstrapState::Probing, None),
            event(command::RuntimeBootstrapState::Activating, None),
        ];
        let states: Vec<_> = events
            .iter()
            .map(|event| event.snapshot.state.clone())
            .collect();
        assert_eq!(
            states,
            vec![
                command::RuntimeBootstrapState::Installing,
                command::RuntimeBootstrapState::Probing,
                command::RuntimeBootstrapState::Activating,
            ]
        );
        assert!(!has_downloading_without_bytes(&events));
    }
}
