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
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const RUNTIME_WORKER_ARG: &str = "--runtime-bootstrap-worker";
const POST_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const RUNTIME_DOWNLOAD_CACHE_DIR: &str = "runtime-download-cache";
#[cfg(feature = "automation-smoke")]
const RUNTIME_POST_DOWNLOAD_TIMEOUT_ENV: &str = "OPENKARA_RUNTIME_POST_DOWNLOAD_TIMEOUT_MS";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeWorkerProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

impl RuntimeWorkerProgress {
    pub fn download_complete(self) -> bool {
        self.total_bytes
            .is_some_and(|total| total > 0 && self.downloaded_bytes >= total)
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

    let app_data_dir = args
        .next()
        .map(PathBuf::from)
        .context("runtime bootstrap worker requires an app-data directory")?;
    let progress_path = args
        .next()
        .map(PathBuf::from)
        .context("runtime bootstrap worker requires a progress path")?;
    if args.next().is_some() {
        bail!("runtime bootstrap worker received unexpected arguments");
    }

    run_worker(&app_data_dir, &progress_path)?;
    Ok(true)
}

fn run_worker(app_data_dir: &Path, progress_path: &Path) -> Result<()> {
    let catalog = catalog::embedded_catalog();
    let runtime = catalog::resolve_runtime(&catalog.manifest, catalog::current_target_triple())?;
    let installed = install_runtime_with_verified_archive_cache(
        app_data_dir,
        runtime,
        catalog,
        |downloaded_bytes, total_bytes| {
            let _ = write_progress(
                progress_path,
                RuntimeWorkerProgress {
                    downloaded_bytes,
                    total_bytes,
                },
            );
        },
    )?;

    // A cached or already-installed runtime may not produce download callbacks.
    // Publish the phase boundary unconditionally so the parent starts its
    // post-download watchdog before dynamic loading and activation.
    write_progress(
        progress_path,
        RuntimeWorkerProgress {
            downloaded_bytes: runtime.byte_size,
            total_bytes: Some(runtime.byte_size),
        },
    )?;

    #[cfg(feature = "automation-smoke")]
    if std::env::var_os("OPENKARA_RUNTIME_WORKER_HANG_AFTER_DOWNLOAD").is_some() {
        thread::sleep(Duration::from_secs(10 * 60));
    }

    // Persist the verified install before the potentially unbounded dynamic
    // load. If this worker is terminated, startup promotes the candidate and
    // retries the exact bytes instead of downloading the archive again.
    runtime_bootstrap::stage_candidate(app_data_dir, &installed.record.artifact_id)?;

    crate::separator::model::ensure_runtime_loaded_from_path(&installed.library_path)
        .with_context(|| {
            format!(
                "failed to load ONNX Runtime from {}",
                installed.library_path.display()
            )
        })?;
    runtime_bootstrap::activate_first_install(app_data_dir, &installed.record.artifact_id)?;
    remove_cached_archive(app_data_dir, runtime);
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

fn ensure_cached_archive(
    app_data_dir: &Path,
    runtime: &CatalogRuntime,
    progress: impl FnMut(u64, Option<u64>),
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
    let downloaded = artifacts::download_verified_to_temp(
        &runtime.download_url,
        runtime.byte_size,
        &runtime.archive_digest,
        cache_dir,
        progress,
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
    progress: impl FnMut(u64, Option<u64>),
) -> Result<runtime_bootstrap::InstalledRuntime> {
    if let Some(existing) = runtime_bootstrap::installed_runtime(app_data_dir, &runtime.artifact_id)
    {
        if runtime_bootstrap::verify_runtime_files(&existing)? {
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
        let archive = ensure_cached_archive(app_data_dir, runtime, progress)?;
        fs::create_dir_all(&staging)
            .with_context(|| format!("failed to create staging directory {}", staging.display()))?;
        let kind = artifacts::archive_kind_for_filename(&runtime.filename)?;
        let extracted = artifacts::extract_archive_safely(&archive, kind, &staging)?;
        artifacts::verify_extracted_files(
            &staging,
            &runtime.extracted_file_digests,
            &extracted,
        )?;
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

fn remove_cached_archive(app_data_dir: &Path, runtime: &CatalogRuntime) {
    let cache_path = runtime_cache_path(app_data_dir, runtime);
    let cache_dir = cache_path.parent().map(Path::to_path_buf);
    let root = app_data_dir.join(RUNTIME_DOWNLOAD_CACHE_DIR);
    let _ = fs::remove_file(cache_path);
    if let Some(cache_dir) = cache_dir {
        let _ = fs::remove_dir(cache_dir);
    }
    let _ = fs::remove_dir(root);
}

fn write_progress(path: &Path, progress: RuntimeWorkerProgress) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, serde_json::to_vec(&progress)?)
        .with_context(|| format!("failed to write {}", temp.display()))?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&temp, path).with_context(|| format!("failed to promote {}", path.display()))?;
    Ok(())
}

fn read_progress(path: &Path) -> Option<RuntimeWorkerProgress> {
    let contents = fs::read(path).ok()?;
    serde_json::from_slice(&contents).ok()
}

fn parse_post_download_timeout(raw: Option<&str>) -> Duration {
    raw.and_then(|value| value.parse::<u64>().ok())
        .filter(|milliseconds| *milliseconds > 0)
        .map(Duration::from_millis)
        .unwrap_or(POST_DOWNLOAD_TIMEOUT)
}

fn post_download_timeout() -> Duration {
    #[cfg(feature = "automation-smoke")]
    {
        return parse_post_download_timeout(
            std::env::var(RUNTIME_POST_DOWNLOAD_TIMEOUT_ENV)
                .ok()
                .as_deref(),
        );
    }
    #[cfg(not(feature = "automation-smoke"))]
    {
        POST_DOWNLOAD_TIMEOUT
    }
}

pub fn install_first_runtime(
    app_data_dir: &Path,
    mut on_progress: impl FnMut(RuntimeWorkerProgress, bool),
) -> Result<runtime_bootstrap::InstalledRuntime> {
    let embedded = catalog::embedded_catalog();
    let runtime = catalog::resolve_runtime(&embedded.manifest, catalog::current_target_triple())?;
    let root = runtime_bootstrap::runtimes_root(app_data_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create runtimes directory {}", root.display()))?;

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let progress_path = root.join(format!("worker-{nonce}.progress.json"));
    let stderr_path = root.join(format!("worker-{nonce}.stderr.log"));
    let stderr_file = fs::File::create(&stderr_path)
        .with_context(|| format!("failed to create {}", stderr_path.display()))?;

    let executable = std::env::current_exe().context("failed to resolve OpenKara executable")?;
    let mut child = Command::new(executable)
        .arg(RUNTIME_WORKER_ARG)
        .arg(app_data_dir)
        .arg(&progress_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .context("failed to start runtime bootstrap worker")?;

    let timeout = post_download_timeout();
    let mut last_progress = None;
    let mut post_download_started = None;
    let status = loop {
        if let Some(progress) = read_progress(&progress_path) {
            if last_progress != Some(progress) {
                let complete = progress.download_complete();
                on_progress(progress, complete);
                last_progress = Some(progress);
                if complete && post_download_started.is_none() {
                    post_download_started = Some(Instant::now());
                }
            }
        }

        if let Some(status) = child
            .try_wait()
            .context("failed to poll runtime bootstrap worker")?
        {
            break status;
        }

        if post_download_started.is_some_and(|started| started.elapsed() > timeout) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&progress_path);
            let details = fs::read_to_string(&stderr_path).unwrap_or_default();
            let _ = fs::remove_file(&stderr_path);
            bail!(
                "runtime installation did not finish within {} seconds after download completed{}",
                timeout.as_secs_f64(),
                if details.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", details.trim())
                }
            );
        }

        thread::sleep(POLL_INTERVAL);
    };

    let _ = fs::remove_file(&progress_path);
    let details = fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = fs::remove_file(&stderr_path);
    if !status.success() {
        bail!(
            "runtime bootstrap worker failed with {status}{}",
            if details.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", details.trim())
            }
        );
    }

    let installed = runtime_bootstrap::installed_runtime(app_data_dir, &runtime.artifact_id)
        .context("runtime worker exited successfully without an installed runtime")?;
    if !runtime_bootstrap::verify_runtime_files(&installed)? {
        bail!("runtime worker produced an unverifiable runtime install");
    }
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_completion_requires_a_positive_total() {
        assert!(!RuntimeWorkerProgress {
            downloaded_bytes: 10,
            total_bytes: None,
        }
        .download_complete());
        assert!(!RuntimeWorkerProgress {
            downloaded_bytes: 0,
            total_bytes: Some(0),
        }
        .download_complete());
        assert!(RuntimeWorkerProgress {
            downloaded_bytes: 10,
            total_bytes: Some(10),
        }
        .download_complete());
    }

    #[test]
    fn progress_file_round_trips_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("progress.json");
        let progress = RuntimeWorkerProgress {
            downloaded_bytes: 128,
            total_bytes: Some(256),
        };
        write_progress(&path, progress).expect("write progress");
        assert_eq!(read_progress(&path), Some(progress));
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
    fn invalid_cached_archive_is_not_reused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let catalog = catalog::embedded_catalog();
        let runtime = catalog::resolve_runtime(&catalog.manifest, catalog::current_target_triple())
            .expect("runtime");
        let path = runtime_cache_path(dir.path(), runtime);
        fs::create_dir_all(path.parent().expect("parent")).expect("cache dir");
        fs::write(&path, b"not the runtime archive").expect("cache file");
        assert!(!cached_archive_is_valid(&path, runtime).expect("validate cache"));
    }
}
