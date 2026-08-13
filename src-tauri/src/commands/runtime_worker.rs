use crate::separator::{
    artifacts,
    catalog::{
        self, record_from_catalog_runtime, write_artifact_record, CatalogRuntime, VerifiedCatalog,
    },
    runtime_bootstrap,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const RUNTIME_WORKER_ARG: &str = "--runtime-bootstrap-worker";
pub const RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER: &str = "runtime_post_download_timeout";

#[cfg(target_os = "windows")]
pub const RUNTIME_POST_DOWNLOAD_TIMEOUT_HINT: &str = "On a VM/server this is usually antivirus or a slow virtual disk. In an Administrator PowerShell, temporarily disable Defender real-time monitoring (`Set-MpPreference -DisableRealtimeMonitoring $true`) and retry; afterwards re-enable it with `Set-MpPreference -DisableRealtimeMonitoring $false`. If it then loads, add a permanent exclusion with `Add-MpPreference -ExclusionPath \"$env:APPDATA\\com.openkara.desktop\\runtimes\"`.";

#[cfg(not(target_os = "windows"))]
pub const RUNTIME_POST_DOWNLOAD_TIMEOUT_HINT: &str = "On a VM/server this is usually antivirus scanning or a slow virtual disk. Temporarily disable real-time antivirus scanning and retry; if it then loads, add a permanent exclusion for the runtimes directory.";

const POST_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const RUNTIME_DOWNLOAD_CACHE_DIR: &str = "runtime-download-cache";
const RUNTIME_DOWNLOAD_TEMP_PREFIX: &str = "artifact.download.";
#[cfg(feature = "automation-smoke")]
const RUNTIME_POST_DOWNLOAD_TIMEOUT_ENV: &str = "OPENKARA_RUNTIME_POST_DOWNLOAD_TIMEOUT_MS";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeWorkerPhase {
    Downloading,
    Installing,
    Probing,
    Activating,
}

impl RuntimeWorkerPhase {
    pub(crate) fn failure_phase(
        self,
    ) -> crate::commands::runtime_bootstrap::RuntimeBootstrapFailurePhase {
        use crate::commands::runtime_bootstrap::RuntimeBootstrapFailurePhase;
        match self {
            RuntimeWorkerPhase::Downloading => RuntimeBootstrapFailurePhase::Download,
            RuntimeWorkerPhase::Installing => RuntimeBootstrapFailurePhase::Install,
            RuntimeWorkerPhase::Probing => RuntimeBootstrapFailurePhase::Probe,
            RuntimeWorkerPhase::Activating => RuntimeBootstrapFailurePhase::Activate,
        }
    }
}

/// Tag a worker error with the last phase the worker reported, so the UI can
/// tell a download failure from an install or load failure. Errors before the
/// first progress entry stay untagged.
fn tag_with_last_phase(
    error: anyhow::Error,
    last_phase: Option<RuntimeWorkerPhase>,
) -> anyhow::Error {
    match last_phase {
        Some(phase) => {
            crate::commands::runtime_bootstrap::with_failure_phase(error, phase.failure_phase())
        }
        None => error,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeWorkerRequest {
    app_data_dir: PathBuf,
    catalog: VerifiedCatalog,
    runtime: CatalogRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeWorkerProgress {
    pub phase: RuntimeWorkerPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

impl RuntimeWorkerProgress {
    fn downloading(downloaded_bytes: u64, total_bytes: Option<u64>) -> Self {
        Self {
            phase: RuntimeWorkerPhase::Downloading,
            downloaded_bytes,
            total_bytes,
        }
    }

    fn phase(phase: RuntimeWorkerPhase) -> Self {
        Self {
            phase,
            downloaded_bytes: 0,
            total_bytes: None,
        }
    }
}

pub fn maybe_run_from_cli() -> Result<bool> {
    let mut args = std::env::args_os().skip(1);
    let Some(mode) = args.next() else {
        return Ok(false);
    };
    if mode != OsStr::new(RUNTIME_WORKER_ARG) {
        return Ok(false);
    }

    let request_path = args
        .next()
        .map(PathBuf::from)
        .context("runtime bootstrap worker requires a request path")?;
    let progress_path = args
        .next()
        .map(PathBuf::from)
        .context("runtime bootstrap worker requires a progress path")?;
    if args.next().is_some() {
        bail!("runtime bootstrap worker received unexpected arguments");
    }

    run_worker(&request_path, &progress_path)?;
    Ok(true)
}

fn run_worker(request_path: &Path, progress_path: &Path) -> Result<()> {
    run_worker_with(request_path, progress_path, |library_path| {
        crate::separator::model::ensure_runtime_loaded_from_path(library_path).map(|_| ())
    })
}

/// Seam at the probe: production loads the runtime into this worker process;
/// tests substitute a probe that succeeds or fails on demand.
fn run_worker_with(
    request_path: &Path,
    progress_path: &Path,
    probe: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let request: RuntimeWorkerRequest =
        serde_json::from_slice(&fs::read(request_path).with_context(|| {
            format!("failed to read worker request {}", request_path.display())
        })?)
        .context("failed to parse runtime worker request")?;
    let resolved = catalog::resolve_runtime(
        &request.catalog.manifest,
        catalog::current_target_triple(),
        crate::config::effective_execution_provider_from_dir(&request.app_data_dir),
    )?;
    anyhow::ensure!(
        resolved.artifact_id == request.runtime.artifact_id
            && resolved.archive_digest == request.runtime.archive_digest,
        "runtime worker request does not match its verified catalog"
    );

    let installed = install_runtime_with_verified_archive_cache(
        &request.app_data_dir,
        &request.runtime,
        &request.catalog,
        |progress| {
            let _ = write_progress(progress_path, progress);
        },
    )?;

    #[cfg(feature = "automation-smoke")]
    if std::env::var_os("OPENKARA_RUNTIME_WORKER_HANG_AFTER_INSTALLING").is_some() {
        thread::sleep(Duration::from_secs(10 * 60));
    }

    anyhow::ensure!(
        installed.library_path.is_file(),
        "installed ONNX Runtime library is missing at {}",
        installed.library_path.display()
    );

    write_progress(
        progress_path,
        RuntimeWorkerProgress::phase(RuntimeWorkerPhase::Probing),
    )?;
    eprintln!(
        "probing ONNX Runtime at {}",
        installed.library_path.display()
    );
    #[cfg(feature = "automation-smoke")]
    if std::env::var_os("OPENKARA_RUNTIME_WORKER_HANG_DURING_PROBE").is_some() {
        thread::sleep(Duration::from_secs(10 * 60));
    }

    probe(&installed.library_path).with_context(|| {
        format!(
            "failed to load ONNX Runtime from {}",
            installed.library_path.display()
        )
    })?;
    eprintln!("probe succeeded for {}", installed.library_path.display());

    // Stage only after the probe proved the library loads: startup
    // acknowledges a promoted candidate on the worker probe's authority
    // (ADR-0023), so an unproven install must never become the candidate
    // (#395). A worker killed before this point loses nothing durable — the
    // verified archive cache and the install directory make the retry a
    // re-verification, not a re-download.
    runtime_bootstrap::stage_candidate(&request.app_data_dir, &installed.record.artifact_id)?;

    write_progress(
        progress_path,
        RuntimeWorkerProgress::phase(RuntimeWorkerPhase::Activating),
    )?;
    Ok(())
}

fn runtime_cache_path(app_data_dir: &Path, runtime: &CatalogRuntime) -> PathBuf {
    app_data_dir
        .join(RUNTIME_DOWNLOAD_CACHE_DIR)
        .join(&runtime.artifact_id)
        .join(format!("{}.verified", runtime.archive_digest))
}

fn cached_archive_is_valid(path: &Path, runtime: &CatalogRuntime) -> Result<bool> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(false);
    };
    if metadata.len() != runtime.byte_size {
        return Ok(false);
    }
    Ok(artifacts::sha256_file(path)? == runtime.archive_digest)
}

fn recover_verified_archive_temp(cache_path: &Path, runtime: &CatalogRuntime) -> Result<bool> {
    let Some(cache_dir) = cache_path.parent() else {
        return Ok(false);
    };
    let entries = fs::read_dir(cache_dir)
        .with_context(|| format!("failed to inspect runtime cache {}", cache_dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        let is_temp = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with(RUNTIME_DOWNLOAD_TEMP_PREFIX) && name.ends_with(".tmp")
            });
        if !is_temp {
            continue;
        }
        if cached_archive_is_valid(&path, runtime)? {
            fs::rename(&path, cache_path).with_context(|| {
                format!(
                    "failed to preserve recovered runtime archive at {}",
                    cache_path.display()
                )
            })?;
            return Ok(true);
        }
        fs::remove_file(&path).with_context(|| {
            format!("failed to remove stale runtime archive {}", path.display())
        })?;
    }
    Ok(false)
}

fn ensure_cached_archive(
    app_data_dir: &Path,
    runtime: &CatalogRuntime,
    mut progress: impl FnMut(RuntimeWorkerProgress),
) -> Result<PathBuf> {
    let cache_path = runtime_cache_path(app_data_dir, runtime);
    if cached_archive_is_valid(&cache_path, runtime)? {
        return Ok(cache_path);
    }
    if cache_path.exists() {
        fs::remove_file(&cache_path)
            .with_context(|| format!("failed to remove invalid cache {}", cache_path.display()))?;
    }

    let cache_dir = cache_path
        .parent()
        .context("runtime archive cache path has no parent")?;
    fs::create_dir_all(cache_dir)
        .with_context(|| format!("failed to create runtime cache {}", cache_dir.display()))?;
    if recover_verified_archive_temp(&cache_path, runtime)? {
        return Ok(cache_path);
    }
    let downloaded = artifacts::download_verified_to_temp(
        &runtime.download_url,
        runtime.byte_size,
        &runtime.archive_digest,
        cache_dir,
        |downloaded_bytes, total_bytes| {
            // Avoid publishing a "downloading" event at 100%; the next phase
            // (installing) will follow once the archive is verified and moved.
            if total_bytes.is_some_and(|total| downloaded_bytes == total) {
                return;
            }
            progress(RuntimeWorkerProgress::downloading(
                downloaded_bytes,
                total_bytes,
            ));
        },
    )?;
    fs::rename(&downloaded, &cache_path).with_context(|| {
        format!(
            "failed to preserve verified runtime archive at {}",
            cache_path.display()
        )
    })?;
    Ok(cache_path)
}

fn install_runtime_with_verified_archive_cache(
    app_data_dir: &Path,
    runtime: &CatalogRuntime,
    catalog: &VerifiedCatalog,
    mut progress: impl FnMut(RuntimeWorkerProgress),
) -> Result<runtime_bootstrap::InstalledRuntime> {
    if let Some(existing) = runtime_bootstrap::installed_runtime(app_data_dir, &runtime.artifact_id)
    {
        if runtime_bootstrap::verify_runtime_files(&existing)? {
            progress(RuntimeWorkerProgress::phase(RuntimeWorkerPhase::Installing));
            return Ok(existing);
        }
        fs::remove_dir_all(&existing.dir).with_context(|| {
            format!(
                "failed to remove unverifiable runtime install {}",
                existing.dir.display()
            )
        })?;
    }

    let root = runtime_bootstrap::runtimes_root(app_data_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create runtimes directory {}", root.display()))?;
    let staging = artifacts::unique_temp_path(&root, "staging");
    let result = (|| -> Result<runtime_bootstrap::InstalledRuntime> {
        if !runtime
            .extracted_file_digests
            .contains_key(runtime_bootstrap::ORT_RUNTIME_FILENAME)
        {
            bail!(
                "runtime artifact {} does not declare the platform library {}",
                runtime.artifact_id,
                runtime_bootstrap::ORT_RUNTIME_FILENAME
            );
        }
        let archive = ensure_cached_archive(app_data_dir, runtime, &mut progress)?;
        progress(RuntimeWorkerProgress::phase(RuntimeWorkerPhase::Installing));
        fs::create_dir_all(&staging)
            .with_context(|| format!("failed to create staging directory {}", staging.display()))?;
        let kind = artifacts::archive_kind_for_filename(&runtime.filename)?;
        let extracted = artifacts::extract_archive_safely(&archive, kind, &staging)?;
        artifacts::verify_extracted_files(&staging, &runtime.extracted_file_digests, &extracted)?;

        let record = record_from_catalog_runtime(runtime, catalog);
        write_artifact_record(
            &staging.join(runtime_bootstrap::RUNTIME_RECORD_FILENAME),
            &record,
        )?;
        let final_dir = runtime_bootstrap::runtime_artifact_dir(app_data_dir, &runtime.artifact_id);
        if final_dir.exists() {
            fs::remove_dir_all(&final_dir).with_context(|| {
                format!("failed to clear stale install {}", final_dir.display())
            })?;
        }
        fs::rename(&staging, &final_dir).with_context(|| {
            format!(
                "failed to activate runtime install at {}",
                final_dir.display()
            )
        })?;
        runtime_bootstrap::installed_runtime(app_data_dir, &runtime.artifact_id)
            .context("freshly installed runtime failed to resolve")
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn write_progress(path: &Path, progress: RuntimeWorkerProgress) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open runtime worker progress {}", path.display()))?;
    serde_json::to_writer(&mut file, &progress)?;
    file.write_all(b"\n").with_context(|| {
        format!(
            "failed to append runtime worker progress {}",
            path.display()
        )
    })?;
    Ok(())
}

fn read_progress(path: &Path, offset: &mut u64) -> Vec<RuntimeWorkerProgress> {
    let Ok(mut file) = fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(file_len) = file.metadata().map(|metadata| metadata.len()) else {
        return Vec::new();
    };
    if *offset > file_len {
        *offset = 0;
    }
    if file.seek(SeekFrom::Start(*offset)).is_err() {
        return Vec::new();
    }

    let mut reader = BufReader::new(file);
    let mut updates = Vec::new();
    while let Ok(line_start) = reader.stream_position() {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if !line.ends_with('\n') => break,
            Ok(_) => {
                if let Ok(progress) = serde_json::from_str(line.trim_end()) {
                    updates.push(progress);
                }
                if let Ok(position) = reader.stream_position() {
                    *offset = position;
                } else {
                    *offset = line_start;
                    break;
                }
            }
            Err(_) => break,
        }
    }
    updates
}

fn parse_post_download_timeout(raw: Option<&str>) -> Duration {
    raw.and_then(|value| value.parse::<u64>().ok())
        .filter(|milliseconds| *milliseconds > 0)
        .map(Duration::from_millis)
        .unwrap_or(POST_DOWNLOAD_TIMEOUT)
}

fn post_download_timeout() -> Duration {
    #[cfg(feature = "automation-smoke")]
    let raw = std::env::var(RUNTIME_POST_DOWNLOAD_TIMEOUT_ENV).ok();
    #[cfg(not(feature = "automation-smoke"))]
    let raw: Option<String> = None;
    parse_post_download_timeout(raw.as_deref())
}

struct WorkerGuard {
    child: Option<Child>,
    paths: Vec<PathBuf>,
    complete: bool,
}

impl WorkerGuard {
    fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            child: None,
            paths,
            complete: false,
        }
    }

    fn attach(&mut self, child: Child) {
        self.child = Some(child);
    }

    fn child(&mut self) -> Option<&mut Child> {
        self.child.as_mut()
    }

    fn complete(mut self) {
        self.complete = true;
    }
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if !self.complete {
            if let Some(child) = self.child.as_mut() {
                if child.try_wait().map_or(true, |s| s.is_none()) {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn install_runtime_with_worker(
    app_data_dir: &Path,
    catalog: &VerifiedCatalog,
    runtime: &CatalogRuntime,
    mut on_progress: impl FnMut(RuntimeWorkerProgress),
) -> Result<runtime_bootstrap::InstalledRuntime> {
    let root = runtime_bootstrap::runtimes_root(app_data_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create runtimes directory {}", root.display()))?;

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let request_path = root.join(format!("worker-{nonce}.request.json"));
    let progress_path = root.join(format!("worker-{nonce}.progress.json"));
    let stderr_path = root.join(format!("worker-{nonce}.stderr.log"));

    let mut guard = WorkerGuard::new(vec![
        request_path.clone(),
        progress_path.clone(),
        stderr_path.clone(),
    ]);

    let request = RuntimeWorkerRequest {
        app_data_dir: app_data_dir.to_path_buf(),
        catalog: catalog.clone(),
        runtime: runtime.clone(),
    };
    fs::write(&request_path, serde_json::to_vec(&request)?).with_context(|| {
        format!(
            "failed to write runtime worker request {}",
            request_path.display()
        )
    })?;
    let stderr_file = fs::File::create(&stderr_path)
        .with_context(|| format!("failed to create {}", stderr_path.display()))?;

    let executable = std::env::current_exe().context("failed to resolve OpenKara executable")?;
    let mut command = Command::new(executable);
    command
        .arg(RUNTIME_WORKER_ARG)
        .arg(&request_path)
        .arg(&progress_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let child = command
        .spawn()
        .context("failed to start runtime bootstrap worker")?;
    guard.attach(child);

    let timeout = post_download_timeout();
    let mut progress_offset = 0;
    let mut last_phase: Option<RuntimeWorkerPhase> = None;
    let mut post_download_started: Option<Instant> = None;
    let status = loop {
        for progress in read_progress(&progress_path, &mut progress_offset) {
            if last_phase != Some(progress.phase) {
                last_phase = Some(progress.phase);
                post_download_started = if progress.phase == RuntimeWorkerPhase::Downloading {
                    None
                } else {
                    Some(Instant::now())
                };
            }
            on_progress(progress);
        }

        if let Some(status) = guard
            .child()
            .context("runtime bootstrap worker handle is missing")?
            .try_wait()
            .context("failed to poll runtime bootstrap worker")?
        {
            break status;
        }

        if post_download_started.is_some_and(|started| started.elapsed() > timeout) {
            if let Some(child) = guard.child() {
                let _ = child.kill();
                let _ = child.wait();
            }
            let details = fs::read_to_string(&stderr_path).unwrap_or_default();
            let phase = last_phase
                .map(|phase| format!("{phase:?}").to_ascii_lowercase())
                .unwrap_or_else(|| "post-download".to_owned());
            guard.complete();
            let error = anyhow::anyhow!(
                "{RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER}: runtime {} did not finish within {} seconds after download (phase={phase}){}\n\n{RUNTIME_POST_DOWNLOAD_TIMEOUT_HINT}",
                runtime.artifact_id,
                timeout.as_secs_f64(),
                if details.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", details.trim())
                }
            );
            return Err(tag_with_last_phase(error, last_phase));
        }

        thread::sleep(POLL_INTERVAL);
    };

    let details = fs::read_to_string(&stderr_path).unwrap_or_default();
    guard.complete();
    if !status.success() {
        let error = anyhow::anyhow!(
            "runtime bootstrap worker failed with {status}{}",
            if details.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", details.trim())
            }
        );
        return Err(tag_with_last_phase(error, last_phase));
    }

    runtime_bootstrap::installed_runtime(app_data_dir, &runtime.artifact_id)
        .context("runtime worker exited successfully without an installed runtime")
        .map_err(|error| {
            crate::commands::runtime_bootstrap::with_failure_phase(
                error,
                crate::commands::runtime_bootstrap::RuntimeBootstrapFailurePhase::Install,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::separator::verified_manifest::sha256_hex;

    fn write_tgz(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("create archive");
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        for (name, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, name, *bytes)
                .expect("append entry");
        }
        builder
            .into_inner()
            .expect("finish tar")
            .finish()
            .expect("finish gzip");
    }

    /// A catalog whose only runtime for this target installs offline: its
    /// archive is pre-seeded into the verified download cache, so the worker
    /// never touches the (unreachable) download URL.
    fn offline_install_fixture(
        app_data: &Path,
        artifact_id: &str,
        library_bytes: &[u8],
    ) -> (VerifiedCatalog, CatalogRuntime) {
        let staging = app_data.join("fixture-archives");
        fs::create_dir_all(&staging).expect("fixture staging dir");
        let archive_path = staging.join(format!("{artifact_id}.tar.gz"));
        write_tgz(
            &archive_path,
            &[(runtime_bootstrap::ORT_RUNTIME_FILENAME, library_bytes)],
        );
        let archive_bytes = fs::read(&archive_path).expect("read archive");

        let runtime = CatalogRuntime {
            artifact_id: artifact_id.to_owned(),
            target_triple: Some(catalog::current_target_triple().to_owned()),
            filename: format!("{artifact_id}.tar.gz"),
            byte_size: archive_bytes.len() as u64,
            archive_digest: sha256_hex(&archive_bytes),
            download_url: "http://127.0.0.1:9/never-fetched.tar.gz".to_owned(),
            extracted_file_digests: [(
                runtime_bootstrap::ORT_RUNTIME_FILENAME.to_owned(),
                catalog::CatalogFileDigest {
                    sha256: sha256_hex(library_bytes),
                    size: library_bytes.len() as u64,
                },
            )]
            .into_iter()
            .collect(),
            runtime: catalog::CatalogRuntimeMetadata {
                version: "v1-test".to_owned(),
                ort_c_api_level: "27".to_owned(),
                execution_providers: vec!["cpu".to_owned()],
                supported_model_artifact_ids: vec![],
                companion_files: vec![],
            },
            deprecation: catalog::CatalogDeprecation::default(),
        };

        let cache_path = runtime_cache_path(app_data, &runtime);
        fs::create_dir_all(cache_path.parent().expect("cache parent")).expect("cache dir");
        fs::rename(&archive_path, &cache_path).expect("seed verified archive cache");

        let mut verified = catalog::embedded_catalog().clone();
        verified.manifest.artifacts.runtimes = vec![runtime.clone()];
        (verified, runtime)
    }

    fn write_worker_request(
        app_data: &Path,
        catalog: &VerifiedCatalog,
        runtime: &CatalogRuntime,
    ) -> (PathBuf, PathBuf) {
        let request = RuntimeWorkerRequest {
            app_data_dir: app_data.to_path_buf(),
            catalog: catalog.clone(),
            runtime: runtime.clone(),
        };
        let request_path = app_data.join("worker-test.request.json");
        fs::write(
            &request_path,
            serde_json::to_vec(&request).expect("serialize request"),
        )
        .expect("write request");
        (request_path, app_data.join("worker-test.progress.json"))
    }

    #[test]
    fn a_failed_probe_does_not_stage_the_install_as_candidate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (catalog, runtime) =
            offline_install_fixture(dir.path(), "rt-probe-fails", b"not-a-real-dylib");
        let (request_path, progress_path) = write_worker_request(dir.path(), &catalog, &runtime);

        let error = run_worker_with(&request_path, &progress_path, |_| {
            anyhow::bail!("scripted probe failure")
        })
        .expect_err("a failing probe must fail the worker");
        assert!(
            format!("{error:#}").contains("scripted probe failure"),
            "unexpected error: {error:#}"
        );

        let slots = runtime_bootstrap::read_slots(dir.path());
        assert_eq!(
            slots.candidate, None,
            "an unproven install must never be staged as the next-launch candidate"
        );
        // The verified install itself survives so a retry re-verifies
        // instead of re-downloading.
        let installed = runtime_bootstrap::installed_runtime(dir.path(), "rt-probe-fails")
            .expect("the verified install must survive the failed probe");
        assert!(runtime_bootstrap::verify_runtime_files(&installed).expect("verify"));
    }

    #[test]
    fn a_successful_probe_stages_the_candidate_only_after_probing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (catalog, runtime) =
            offline_install_fixture(dir.path(), "rt-probe-passes", b"probed-dylib");
        let (request_path, progress_path) = write_worker_request(dir.path(), &catalog, &runtime);

        let mut candidate_at_probe_time = None;
        run_worker_with(&request_path, &progress_path, |library_path| {
            assert!(
                library_path.is_file(),
                "probe must see the installed library"
            );
            candidate_at_probe_time = Some(runtime_bootstrap::read_slots(dir.path()).candidate);
            Ok(())
        })
        .expect("the worker must succeed when the probe passes");

        assert_eq!(
            candidate_at_probe_time,
            Some(None),
            "the candidate slot must still be empty while the probe runs"
        );
        assert_eq!(
            runtime_bootstrap::read_slots(dir.path())
                .candidate
                .as_deref(),
            Some("rt-probe-passes"),
            "a proven install must be staged as the next-launch candidate"
        );
    }

    #[test]
    fn progress_round_trips_with_an_explicit_phase() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("progress.json");
        let progress = [
            RuntimeWorkerProgress::downloading(128, Some(256)),
            RuntimeWorkerProgress::phase(RuntimeWorkerPhase::Installing),
            RuntimeWorkerProgress::phase(RuntimeWorkerPhase::Probing),
        ];
        for update in progress {
            write_progress(&path, update).expect("write progress");
        }
        let mut offset = 0;
        assert_eq!(read_progress(&path, &mut offset), progress);
        assert!(read_progress(&path, &mut offset).is_empty());
    }

    #[test]
    fn worker_phases_map_to_their_failure_phases() {
        use crate::commands::runtime_bootstrap::RuntimeBootstrapFailurePhase;
        assert_eq!(
            RuntimeWorkerPhase::Downloading.failure_phase(),
            RuntimeBootstrapFailurePhase::Download
        );
        assert_eq!(
            RuntimeWorkerPhase::Installing.failure_phase(),
            RuntimeBootstrapFailurePhase::Install
        );
        assert_eq!(
            RuntimeWorkerPhase::Probing.failure_phase(),
            RuntimeBootstrapFailurePhase::Probe
        );
        assert_eq!(
            RuntimeWorkerPhase::Activating.failure_phase(),
            RuntimeBootstrapFailurePhase::Activate
        );
    }

    #[test]
    fn tagging_without_an_observed_phase_leaves_the_error_unclassified() {
        use crate::commands::runtime_bootstrap::failure_phase_of;

        let untagged = tag_with_last_phase(anyhow::anyhow!("spawn failed"), None);
        assert_eq!(failure_phase_of(&untagged), None);
        assert_eq!(untagged.to_string(), "spawn failed");

        let tagged = tag_with_last_phase(
            anyhow::anyhow!("worker died mid-extract"),
            Some(RuntimeWorkerPhase::Installing),
        );
        assert_eq!(
            failure_phase_of(&tagged),
            Some(crate::commands::runtime_bootstrap::RuntimeBootstrapFailurePhase::Install)
        );
        assert_eq!(tagged.to_string(), "worker died mid-extract");
    }

    #[test]
    fn post_download_timeout_override_must_be_positive() {
        assert_eq!(parse_post_download_timeout(None), POST_DOWNLOAD_TIMEOUT);
        assert_eq!(
            parse_post_download_timeout(Some("2500")),
            Duration::from_millis(2_500)
        );
        assert_eq!(
            parse_post_download_timeout(Some("0")),
            POST_DOWNLOAD_TIMEOUT
        );
        assert_eq!(
            parse_post_download_timeout(Some("invalid")),
            POST_DOWNLOAD_TIMEOUT
        );
    }

    #[test]
    fn post_download_phase_is_not_inferred_from_bytes() {
        let progress = RuntimeWorkerProgress {
            phase: RuntimeWorkerPhase::Downloading,
            downloaded_bytes: 128,
            total_bytes: Some(128),
        };
        assert_eq!(progress.phase, RuntimeWorkerPhase::Downloading);
        assert_eq!(
            RuntimeWorkerProgress::phase(RuntimeWorkerPhase::Installing).total_bytes,
            None
        );
    }

    #[test]
    fn verified_archive_temp_is_recovered_after_an_interrupted_install() {
        let dir = tempfile::tempdir().expect("tempdir");
        let catalog = catalog::embedded_catalog();
        let embedded_runtime = catalog::resolve_runtime(
            &catalog.manifest,
            catalog::current_target_triple(),
            crate::config::ExecutionProviderPreference::default_for_current_platform(),
        )
        .expect("runtime");
        let payload = b"verified runtime archive";
        let mut runtime = embedded_runtime.clone();
        runtime.byte_size = payload.len() as u64;

        let temp_path = dir.path().join("payload.tmp");
        fs::write(&temp_path, payload).expect("write payload");
        runtime.archive_digest = artifacts::sha256_file(&temp_path).expect("hash payload");

        let cache_path = runtime_cache_path(dir.path(), &runtime);
        fs::create_dir_all(cache_path.parent().expect("cache directory")).expect("cache dir");
        let interrupted_path = cache_path
            .parent()
            .expect("cache directory")
            .join("artifact.download.interrupted.tmp");
        fs::rename(&temp_path, &interrupted_path).expect("move interrupted archive");

        assert!(recover_verified_archive_temp(&cache_path, &runtime).expect("recover archive"));
        assert!(cached_archive_is_valid(&cache_path, &runtime).expect("validate archive"));
        assert!(!interrupted_path.exists());
    }
}
