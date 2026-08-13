//! Runtime activation: the single owner of "select a runtime, activate its
//! slot, load the ONNX Runtime library, fall back to CPU on a DirectML
//! timeout, and report which phase failed".
//!
//! Two entries serve every caller:
//!
//! - [`resolve_and_load`] walks the slot state ([`runtime_bootstrap`]) and
//!   commits the resulting runtime. Startup calls this and nothing else.
//! - [`load_with_watchdog`] commits an install the caller already resolved
//!   (a fresh worker install, a staged candidate, the active slot, or a
//!   legacy install).
//!
//! Behind them this module owns the artifact directory layout, the Windows
//! DLL search directory and probe load (ADR-0022, ADR-0024), the DirectML
//! companion preload (ADR-0019), the load watchdog and its process latches,
//! the persisted CPU fallback after a DirectML load timeout (ADR-0023), and
//! the failure phase the UI renders.
//!
//! [`RuntimeLoader`] is the seam at the load strategy:
//! [`InstallDirectoryLoader`] is the production adapter and the tests
//! substitute an adapter that fails or stalls on demand.

use crate::commands::runtime_worker::{
    RUNTIME_POST_DOWNLOAD_TIMEOUT_HINT, RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER,
};
use crate::separator::catalog;
use crate::separator::runtime_bootstrap;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::Duration,
};

#[cfg(target_os = "windows")]
pub const ORT_RUNTIME_FILENAME: &str = "onnxruntime.dll";
#[cfg(target_os = "linux")]
pub const ORT_RUNTIME_FILENAME: &str = "libonnxruntime.so";
#[cfg(target_vendor = "apple")]
pub const ORT_RUNTIME_FILENAME: &str = "libonnxruntime.dylib";

// ---------------------------------------------------------------------------
// Process commitment
// ---------------------------------------------------------------------------

static ORT_RUNTIME_PATH: OnceLock<PathBuf> = OnceLock::new();
static ORT_RUNTIME_INIT_LOCK: Mutex<()> = Mutex::new(());

/// The runtime library committed into this process, when one is loaded.
/// ORT cannot be unloaded or swapped in place — a different runtime only
/// takes effect after a restart.
pub fn loaded_runtime_path() -> Option<&'static Path> {
    ORT_RUNTIME_PATH.get().map(|path| path.as_path())
}

/// The committed runtime, or the error model loading reports when the
/// managed bootstrap has not finished yet.
pub(crate) fn ensure_activated() -> Result<&'static Path> {
    loaded_runtime_path().context(
        "ONNX Runtime is not initialized; the managed runtime bootstrap must complete before model loading",
    )
}

pub fn ensure_runtime_loaded_from_path(runtime_path: &Path) -> Result<&'static Path> {
    if let Some(path) = ORT_RUNTIME_PATH.get() {
        // Committed runtime is process-final; do not report a different path.
        anyhow::ensure!(
            path.as_path() == runtime_path,
            "a different ONNX Runtime is already loaded from {}; restart to use {}",
            path.display(),
            runtime_path.display()
        );
        return Ok(path.as_path());
    }

    let _init_guard = ORT_RUNTIME_INIT_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("onnx runtime initialization lock was poisoned"))?;
    if let Some(path) = ORT_RUNTIME_PATH.get() {
        anyhow::ensure!(
            path.as_path() == runtime_path,
            "a different ONNX Runtime is already loaded from {}; restart to use {}",
            path.display(),
            runtime_path.display()
        );
        return Ok(path.as_path());
    }

    init_ort_from_path(runtime_path)?;
    Ok(ORT_RUNTIME_PATH
        .get()
        .expect("runtime path should be stored after successful initialization")
        .as_path())
}

fn init_ort_from_path(runtime_path: &Path) -> Result<()> {
    anyhow::ensure!(
        runtime_path.is_file(),
        "ONNX Runtime library is missing at {}",
        runtime_path.display()
    );

    #[cfg(target_os = "windows")]
    {
        prepare_windows_runtime_dll_search(runtime_path)?;
        // Probe-load the runtime DLL before `ort::init_from` runs so a load
        // failure carries the real `GetLastError` code instead of the opaque
        // "LoadLibraryExW failed" string `ort`/`libloading` produce.
        probe_load_windows_runtime(runtime_path).map_err(|message| anyhow::anyhow!(message))?;
    }

    let committed = ort::init_from(runtime_path)?.with_name("openkara").commit();
    anyhow::ensure!(
        committed,
        "failed to initialize ONNX Runtime from {} before another ORT environment was configured",
        runtime_path.display()
    );

    let _ = ORT_RUNTIME_PATH.set(runtime_path.to_path_buf());
    Ok(())
}

// ---------------------------------------------------------------------------
// Install directory (ADR-0019, ADR-0022, ADR-0024)
// ---------------------------------------------------------------------------

#[cfg(any(test, target_os = "windows"))]
pub(crate) fn runtime_dll_search_dir(runtime_path: &Path) -> Option<&Path> {
    runtime_path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
}

#[cfg(target_os = "windows")]
fn prepare_windows_runtime_dll_search(runtime_path: &Path) -> Result<()> {
    use windows::{core::HSTRING, Win32::System::LibraryLoader::SetDllDirectoryW};

    let search_dir = runtime_dll_search_dir(runtime_path).with_context(|| {
        format!(
            "ONNX Runtime path has no parent directory: {}",
            runtime_path.display()
        )
    })?;
    let path = HSTRING::from(search_dir.as_os_str());
    // SAFETY: `path` is a valid UTF-16 directory string owned by `HSTRING` for
    // the duration of this call. SetDllDirectoryW copies the path.
    unsafe { SetDllDirectoryW(&path) }.with_context(|| {
        format!(
            "failed to set Windows DLL search directory to {}",
            search_dir.display()
        )
    })?;
    Ok(())
}

/// Map a Win32 error code returned by `LoadLibraryExW` to a short human
/// description for the runtime-load error message. The numeric and hex codes
/// are always appended because the table is intentionally narrow — unknown
/// codes still surface enough detail for triage.
#[cfg(target_os = "windows")]
fn describe_win32_load_error(code: u32) -> String {
    // System error constants from winerror.h.
    const ERROR_MOD_NOT_FOUND: u32 = 126;
    const ERROR_BAD_EXE_FORMAT: u32 = 193;
    const ERROR_INVALID_PARAMETER: u32 = 87;
    const ERROR_FILE_NOT_FOUND: u32 = 2;
    const ERROR_ACCESS_DENIED: u32 = 5;
    const ERROR_SHARING_VIOLATION: u32 = 32;
    const ERROR_SXS_DLL_NOT_FOUND: u32 = 14090;
    const ERROR_SXS_SYSTEM_DEFAULT_ACTIVATION_CONTEXT_EMPTY: u32 = 14002;

    let hint = match code {
        ERROR_MOD_NOT_FOUND => {
            "a DLL that onnxruntime.dll depends on is missing. The VC++ CRT \
             DLLs it needs (vcruntime140, vcruntime140_1, msvcp140, \
             msvcp140_1) ship next to openkara.exe, so this usually means an \
             incomplete app install"
        }
        ERROR_FILE_NOT_FOUND => "the runtime file was not found at the given path",
        ERROR_BAD_EXE_FORMAT => {
            "the runtime DLL is for a different architecture than the app \
             (e.g. x86_64 vs arm64) or is corrupt"
        }
        ERROR_INVALID_PARAMETER => "Windows rejected the load flags or path",
        ERROR_ACCESS_DENIED => "the app does not have permission to read the runtime DLL",
        ERROR_SHARING_VIOLATION => {
            "the runtime DLL is locked by another process or an antivirus scan"
        }
        ERROR_SXS_DLL_NOT_FOUND | ERROR_SXS_SYSTEM_DEFAULT_ACTIVATION_CONTEXT_EMPTY => {
            "a side-by-side (WinSxS) dependency of onnxruntime.dll is missing"
        }
        _ => "Windows did not provide a recognized reason for this code",
    };
    format!("{hint} (Win32 error {code} / 0x{code:08X})")
}

/// Probe-load the runtime DLL once before handing the path to `ort`. This
/// supersedes the page-cache warmup that preceded it and does two things the
/// earlier `fs::read` could not:
///
/// 1. It resolves the imports and runs `DllMain`, so the first-touch cost (disk
///    reads, antivirus scan) is paid under our control rather than inside
///    `ort`'s load watchdog. The probe releases its reference before returning,
///    so `ort::init_from` still performs a full load; what carries over is the
///    warm file cache, not a loaded module.
/// 2. On failure it captures the real `GetLastError` code. `ort` wraps
///    `libloading`, whose `Display` impl drops the OS error and prints only
///    "LoadLibraryExW failed" (see libloading `error.rs`). Calling the loader
///    ourselves lets us attach the actual code so the failure isn't opaque.
///
/// The call mirrors the load `ort` performs — `libloading`'s `Library::new`
/// is `LoadLibraryExW(path, NULL, 0)` — so both loads resolve dependencies
/// through the same standard search order (application directory carrying the
/// app-local VC++ CRT first, then the `SetDllDirectoryW` runtime directory
/// carrying DirectML.dll) and the probe fails exactly when the real load
/// would fail.
///
/// Returns `Ok(())` on a successful probe-load or `Err(message)` with a rich
/// diagnostic otherwise.
#[cfg(target_os = "windows")]
fn probe_load_windows_runtime(runtime_path: &Path) -> std::result::Result<(), String> {
    use windows::{
        core::HSTRING,
        Win32::Foundation::{FreeLibrary, GetLastError},
        Win32::System::LibraryLoader::{LoadLibraryExW, LOAD_LIBRARY_FLAGS},
    };

    let path = HSTRING::from(runtime_path.as_os_str());
    // SAFETY: `path` is owned by HSTRING for the call. Null file handle and
    // zero flags replicate libloading's `Library::new`, keeping the probe's
    // dependency resolution identical to the real `ort` load.
    let handle = unsafe { LoadLibraryExW(&path, None, LOAD_LIBRARY_FLAGS(0)) };
    match handle {
        Ok(module) => {
            // Release the probe's reference so `ort::init_from` holds the only
            // one. The refcount can reach zero here, in which case Windows
            // unloads the module and `ort` performs a fresh load — normal
            // loader work and `DllMain` included — off the warm file cache.
            // SAFETY: `module` was just returned by a successful LoadLibraryExW.
            let _ = unsafe { FreeLibrary(module) };
            Ok(())
        }
        Err(_) => {
            // `LoadLibraryExW` returns a `windows_core::Error` whose embedded
            // code matches GetLastError; read it directly to avoid relying on
            // its formatting.
            let code = unsafe { GetLastError() }.0;
            Err(format!(
                "failed to load ONNX Runtime DLL at {}: {}",
                runtime_path.display(),
                describe_win32_load_error(code)
            ))
        }
    }
}

/// Load the bundled DirectML companion only when a DirectML session is
/// requested. ORT resolves provider libraries by module name, so the exact
/// artifact path must be preloaded before provider registration.
#[cfg(target_os = "windows")]
pub(crate) fn preload_directml_companion() -> Result<()> {
    let runtime_path =
        ensure_activated().context("ONNX Runtime path is not available for DirectML setup")?;
    let runtime_dir = runtime_path
        .parent()
        .context("ONNX Runtime path has no parent directory")?;
    let directml_path = runtime_dir.join("DirectML.dll");
    ort::util::preload_dylib(&directml_path).with_context(|| {
        format!(
            "failed to preload bundled DirectML companion {}",
            directml_path.display()
        )
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Load strategy seam
// ---------------------------------------------------------------------------

/// What "commit the ONNX Runtime library at this path to this process"
/// means. Production loads from the runtime install directory; tests
/// substitute an adapter that fails or stalls on demand.
pub trait RuntimeLoader: Send + Sync + 'static {
    fn load(&self, library_path: &Path) -> Result<()>;

    /// The library already committed to this process. ORT cannot be
    /// unloaded, so the answer is final for the process lifetime.
    fn committed(&self) -> Option<PathBuf>;
}

/// Production adapter: load the library from the artifact directory it was
/// installed into, with the Windows DLL search directory pointed at that
/// same directory (ADR-0022).
pub struct InstallDirectoryLoader;

impl RuntimeLoader for InstallDirectoryLoader {
    fn load(&self, library_path: &Path) -> Result<()> {
        ensure_runtime_loaded_from_path(library_path).map(|_| ())
    }

    fn committed(&self) -> Option<PathBuf> {
        loaded_runtime_path().map(Path::to_path_buf)
    }
}

// ---------------------------------------------------------------------------
// Load watchdog
// ---------------------------------------------------------------------------

const RUNTIME_LOAD_TIMEOUT: Duration = Duration::from_secs(120);
const RUNTIME_LOAD_IN_PROGRESS_MARKER: &str = "runtime_parent_load_in_progress";

/// A timed-out load poisons the process: the abandoned loader thread may
/// still be inside `DllMain`, so no further load may start before a restart.
#[derive(Default)]
struct LoadLatches {
    in_progress: AtomicBool,
    timed_out: AtomicBool,
}

#[derive(Clone)]
pub struct LoadStrategy {
    loader: Arc<dyn RuntimeLoader>,
    latches: Arc<LoadLatches>,
    timeout: Duration,
}

impl LoadStrategy {
    pub fn production() -> Self {
        static PROCESS_LATCHES: OnceLock<Arc<LoadLatches>> = OnceLock::new();
        Self {
            loader: Arc::new(InstallDirectoryLoader),
            latches: Arc::clone(PROCESS_LATCHES.get_or_init(|| Arc::new(LoadLatches::default()))),
            timeout: RUNTIME_LOAD_TIMEOUT,
        }
    }

    pub fn new(loader: Arc<dyn RuntimeLoader>, timeout: Duration) -> Self {
        Self {
            loader,
            latches: Arc::new(LoadLatches::default()),
            timeout,
        }
    }

    fn commit(&self, library_path: &Path) -> LoadOutcome {
        if self.latches.timed_out.load(Ordering::SeqCst) {
            return LoadOutcome::ProcessUnavailable(anyhow::anyhow!(
                "{RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER}: ONNX Runtime load already timed out; restart OpenKara before retrying"
            ));
        }

        if let Some(loaded) = self.loader.committed() {
            if loaded == library_path {
                return LoadOutcome::Committed;
            }
            return LoadOutcome::ProcessUnavailable(anyhow::anyhow!(
                "a different ONNX Runtime is already loaded from {}; restart to use {}",
                loaded.display(),
                library_path.display()
            ));
        }

        if self.latches.in_progress.swap(true, Ordering::SeqCst) {
            return LoadOutcome::ProcessUnavailable(anyhow::anyhow!(
                "{RUNTIME_LOAD_IN_PROGRESS_MARKER}: ONNX Runtime load is already in progress; restart OpenKara before retrying"
            ));
        }

        let (sender, receiver) = mpsc::sync_channel(1);
        let loader = Arc::clone(&self.loader);
        let latches = Arc::clone(&self.latches);
        let runtime_path = library_path.to_path_buf();
        thread::spawn(move || {
            // A panicking load must not unwind past the latch release below:
            // `in_progress` would stay set for the process lifetime and every
            // retry would be refused until a restart.
            let result = catch_unwind(AssertUnwindSafe(|| {
                loader
                    .load(&runtime_path)
                    .map_err(|error| format!("{error:#}"))
            }))
            .unwrap_or_else(|payload| Err(load_panic_message(payload.as_ref())));
            latches.in_progress.store(false, Ordering::SeqCst);
            let _ = sender.send(result);
        });

        match receiver.recv_timeout(self.timeout) {
            Ok(Ok(())) => LoadOutcome::Committed,
            Ok(Err(error)) => LoadOutcome::ArtifactFailed(anyhow::anyhow!(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.latches.timed_out.store(true, Ordering::SeqCst);
                LoadOutcome::ArtifactFailed(anyhow::anyhow!(load_timeout_message(self.timeout)))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => LoadOutcome::ArtifactFailed(
                anyhow::anyhow!("ONNX Runtime load watchdog exited before reporting a result"),
            ),
        }
    }
}

enum LoadOutcome {
    Committed,
    /// The artifact itself failed to load or stalled past the watchdog, so
    /// the slot that names it is suspect.
    ArtifactFailed(anyhow::Error),
    /// This process can no longer load any runtime. No artifact is
    /// implicated, so no slot is rolled back and no generation is deleted.
    ProcessUnavailable(anyhow::Error),
}

fn load_timeout_message(timeout: Duration) -> String {
    format!(
        "{}: ONNX Runtime load did not finish within {} seconds\n\n{}",
        RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER,
        timeout.as_secs(),
        RUNTIME_POST_DOWNLOAD_TIMEOUT_HINT,
    )
}

fn load_panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    let detail = payload
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic");
    format!("ONNX Runtime loader thread panicked: {detail}")
}

#[cfg(test)]
pub(crate) fn runtime_load_timeout_message() -> String {
    load_timeout_message(RUNTIME_LOAD_TIMEOUT)
}

/// Commit a runtime under the load watchdog with no slot or configuration
/// side effect. Callers that own a slot state use [`load_with_watchdog`].
#[cfg(feature = "automation-smoke")]
pub(crate) fn commit_with_watchdog(library_path: &Path) -> Result<()> {
    match LoadStrategy::production().commit(library_path) {
        LoadOutcome::Committed => Ok(()),
        LoadOutcome::ArtifactFailed(error) | LoadOutcome::ProcessUnavailable(error) => Err(error),
    }
}

// ---------------------------------------------------------------------------
// Failure phase
// ---------------------------------------------------------------------------

/// Bootstrap step where a `Failed` snapshot's error originated. Lets the UI
/// stop blaming the network for install, load-probe, and activation failures
/// (#284 was a `LoadLibraryExW` failure presented as a download failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBootstrapFailurePhase {
    Download,
    Install,
    Probe,
    Activate,
}

/// Transparent wrapper that tags a bootstrap error with the phase it came
/// from. Display forwards to the wrapped error, so user-visible messages and
/// the marker-based `CommandError` mapping are unchanged.
#[derive(Debug)]
pub(crate) struct PhasedBootstrapError {
    phase: RuntimeBootstrapFailurePhase,
    source: anyhow::Error,
}

impl std::fmt::Display for PhasedBootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.source, f)
    }
}

impl std::error::Error for PhasedBootstrapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

pub(crate) fn with_failure_phase(
    error: anyhow::Error,
    phase: RuntimeBootstrapFailurePhase,
) -> anyhow::Error {
    anyhow::Error::new(PhasedBootstrapError {
        phase,
        source: error,
    })
}

pub(crate) fn failure_phase_of(error: &anyhow::Error) -> Option<RuntimeBootstrapFailurePhase> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<PhasedBootstrapError>())
        .map(|phased| phased.phase)
}

// ---------------------------------------------------------------------------
// Activation
// ---------------------------------------------------------------------------

/// Identity of the install a caller wants committed.
#[derive(Debug, Clone, Copy, Default)]
pub struct ActivationTarget<'a> {
    /// Slot artifact id. `None` for a legacy (pre-slot) install, which owns
    /// no slot and therefore has nothing to roll back to.
    pub artifact_id: Option<&'a str>,
    /// Execution providers the artifact advertises, when the caller already
    /// resolved them from a catalog newer than the embedded one. `None`
    /// resolves them from the embedded catalog by `artifact_id`.
    pub execution_providers: Option<&'a [String]>,
}

/// How a freshly promoted candidate is proved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateProof {
    /// ADR-0023: the bootstrap worker's probe is authoritative, so the
    /// promoted candidate is acknowledged without loading it here.
    WorkerProbe,
    /// The caller needs a usable runtime now, so the candidate is proved by
    /// loading it in this process.
    LoadHere,
}

/// Resolve the runtime slot state and commit the resulting runtime.
///
/// Returns the committed library path, or `None` when nothing was loaded:
/// either no runtime is installed, or a promoted candidate was acknowledged
/// under [`CandidateProof::WorkerProbe`].
pub fn resolve_and_load(app_data_dir: &Path, proof: CandidateProof) -> Result<Option<PathBuf>> {
    resolve_and_load_with(&LoadStrategy::production(), app_data_dir, proof)
}

/// Commit an install the caller already resolved.
///
/// Records the persisted DirectML CPU fallback when the load times out,
/// rolls the active slot back to the previous verified generation when the
/// artifact itself failed, and tags the returned error with its bootstrap
/// phase. The returned path is the previous generation after a rollback.
pub fn load_with_watchdog(
    app_data_dir: &Path,
    library_path: &Path,
    target: ActivationTarget<'_>,
) -> Result<PathBuf> {
    load_with_watchdog_using(
        &LoadStrategy::production(),
        app_data_dir,
        library_path,
        target,
    )
}

fn resolve_and_load_with(
    strategy: &LoadStrategy,
    app_data_dir: &Path,
    proof: CandidateProof,
) -> Result<Option<PathBuf>> {
    let Some(plan) = runtime_bootstrap::begin_startup(app_data_dir)? else {
        return Ok(None);
    };

    if plan.proving_candidate && proof == CandidateProof::WorkerProbe {
        runtime_bootstrap::finish_activation_success(app_data_dir)
            .map_err(|error| with_failure_phase(error, RuntimeBootstrapFailurePhase::Activate))?;
        return Ok(None);
    }

    let artifact_id = plan
        .record
        .as_ref()
        .map(|record| record.artifact_id.as_str())
        .filter(|artifact_id| !artifact_id.is_empty());
    load_with_watchdog_using(
        strategy,
        app_data_dir,
        &plan.library_path,
        ActivationTarget {
            artifact_id,
            execution_providers: None,
        },
    )
    .map(Some)
}

fn load_with_watchdog_using(
    strategy: &LoadStrategy,
    app_data_dir: &Path,
    library_path: &Path,
    target: ActivationTarget<'_>,
) -> Result<PathBuf> {
    let error = match strategy.commit(library_path) {
        LoadOutcome::Committed => {
            acknowledge_pending_activation(app_data_dir, target.artifact_id)?;
            return Ok(library_path.to_path_buf());
        }
        LoadOutcome::ProcessUnavailable(error) => {
            record_directml_timeout(app_data_dir, target, &error);
            return Err(with_failure_phase(
                error,
                RuntimeBootstrapFailurePhase::Probe,
            ));
        }
        LoadOutcome::ArtifactFailed(error) => error,
    };

    tracing::warn!(
        "failed to load ONNX Runtime from {}: {error:#}",
        library_path.display()
    );
    record_directml_timeout(app_data_dir, target, &error);

    match restore_previous_generation(strategy, app_data_dir, target.artifact_id, &error) {
        Some(previous) => Ok(previous),
        None => Err(with_failure_phase(
            error,
            RuntimeBootstrapFailurePhase::Probe,
        )),
    }
}

fn acknowledge_pending_activation(app_data_dir: &Path, artifact_id: Option<&str>) -> Result<()> {
    let Some(artifact_id) = artifact_id else {
        return Ok(());
    };
    let slots = runtime_bootstrap::read_slots(app_data_dir);
    if slots.activation_pending && slots.active.as_deref() == Some(artifact_id) {
        runtime_bootstrap::finish_activation_success(app_data_dir)
            .map_err(|error| with_failure_phase(error, RuntimeBootstrapFailurePhase::Activate))?;
    }
    Ok(())
}

/// Roll a failed active runtime back to the previous verified generation and
/// commit that instead. Only the artifact currently occupying the active
/// slot has a generation to fall back to; a fresh install is not referenced
/// by any slot yet and must not disturb the slot file.
fn restore_previous_generation(
    strategy: &LoadStrategy,
    app_data_dir: &Path,
    artifact_id: Option<&str>,
    error: &anyhow::Error,
) -> Option<PathBuf> {
    let artifact_id = artifact_id?;
    if runtime_bootstrap::read_slots(app_data_dir)
        .active
        .as_deref()
        != Some(artifact_id)
    {
        return None;
    }

    let restored = match runtime_bootstrap::rollback_failed_activation(
        app_data_dir,
        artifact_id,
        &format!("{error:#}"),
    ) {
        Ok(restored) => restored?,
        Err(rollback_error) => {
            tracing::warn!("failed to record runtime load failure: {rollback_error:#}");
            return None;
        }
    };

    match strategy.commit(&restored.library_path) {
        LoadOutcome::Committed => Some(restored.library_path),
        LoadOutcome::ArtifactFailed(previous_error)
        | LoadOutcome::ProcessUnavailable(previous_error) => {
            tracing::warn!(
                "failed to load previous ONNX Runtime {}: {previous_error:#}",
                restored.library_path.display()
            );
            None
        }
    }
}

/// ADR-0023: a DirectML-linked runtime that timed out disables DirectML for
/// this host so the next selection resolves the CPU-only artifact.
fn record_directml_timeout(
    app_data_dir: &Path,
    target: ActivationTarget<'_>,
    error: &anyhow::Error,
) {
    let from_catalog = target.execution_providers.is_none().then(|| {
        target.artifact_id.and_then(|artifact_id| {
            catalog::runtime_by_artifact_id(&catalog::embedded_catalog().manifest, artifact_id)
                .map(|runtime| runtime.runtime.execution_providers.as_slice())
        })
    });
    let Some(execution_providers) = target.execution_providers.or(from_catalog.flatten()) else {
        return;
    };
    if let Err(record_error) = crate::config::record_directml_unavailable_on_timeout(
        app_data_dir,
        execution_providers,
        &format!("{error:#}"),
    ) {
        tracing::warn!("failed to record directml timeout disable: {record_error:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::separator::catalog::{
        embedded_catalog, record_from_catalog_runtime, resolve_runtime, write_artifact_record,
        CatalogRuntime, InstalledFileRecord, VerifiedCatalog,
    };
    use crate::separator::runtime_bootstrap::{
        read_slots, runtime_artifact_dir, write_slots, RuntimeSlots, RUNTIME_RECORD_FILENAME,
    };
    use crate::separator::verified_manifest::sha256_hex;
    use std::collections::HashMap;
    use std::fs;

    // record_directml_unavailable_on_timeout flips a process-wide override,
    // so tests that can reach it run under this lock and reset it.
    static DIRECTML_TIMEOUT_TEST_LOCK: Mutex<()> = Mutex::new(());

    const STALL: Duration = Duration::from_millis(750);
    const WATCHDOG: Duration = Duration::from_millis(60);

    #[derive(Clone)]
    enum Behaviour {
        Succeed,
        Fail(&'static str),
        Stall,
        Panic,
    }

    /// Test adapter at the load-strategy seam: every path succeeds unless
    /// the script says it fails or stalls past the watchdog.
    struct ScriptedLoader {
        script: Mutex<HashMap<PathBuf, Behaviour>>,
        committed: Mutex<Option<PathBuf>>,
        attempts: Mutex<Vec<PathBuf>>,
    }

    impl ScriptedLoader {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                script: Mutex::new(HashMap::new()),
                committed: Mutex::new(None),
                attempts: Mutex::new(Vec::new()),
            })
        }

        fn script(self: &Arc<Self>, path: &Path, behaviour: Behaviour) -> Arc<Self> {
            self.script
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), behaviour);
            Arc::clone(self)
        }

        fn attempts(&self) -> Vec<PathBuf> {
            self.attempts.lock().unwrap().clone()
        }
    }

    impl RuntimeLoader for ScriptedLoader {
        fn load(&self, library_path: &Path) -> Result<()> {
            self.attempts.lock().unwrap().push(library_path.to_owned());
            let behaviour = self
                .script
                .lock()
                .unwrap()
                .get(library_path)
                .cloned()
                .unwrap_or(Behaviour::Succeed);
            match behaviour {
                Behaviour::Succeed => {
                    *self.committed.lock().unwrap() = Some(library_path.to_owned());
                    Ok(())
                }
                Behaviour::Fail(message) => anyhow::bail!(message),
                Behaviour::Stall => {
                    thread::sleep(STALL);
                    Ok(())
                }
                Behaviour::Panic => panic!("scripted load panic"),
            }
        }

        fn committed(&self) -> Option<PathBuf> {
            self.committed.lock().unwrap().clone()
        }
    }

    fn strategy(loader: &Arc<ScriptedLoader>) -> LoadStrategy {
        LoadStrategy::new(Arc::clone(loader) as Arc<dyn RuntimeLoader>, WATCHDOG)
    }

    fn catalog_runtime() -> (&'static VerifiedCatalog, &'static CatalogRuntime) {
        let catalog = embedded_catalog();
        let runtime = resolve_runtime(
            &catalog.manifest,
            crate::separator::catalog::current_target_triple(),
            crate::config::ExecutionProviderPreference::default_for_current_platform(),
        )
        .expect("embedded catalog must resolve the current target runtime");
        (catalog, runtime)
    }

    /// Write a fake installed runtime whose record digests match its files.
    fn install(app_data: &Path, artifact_id: &str) -> PathBuf {
        let (catalog, runtime) = catalog_runtime();
        let dir = runtime_artifact_dir(app_data, artifact_id);
        fs::create_dir_all(&dir).expect("create artifact dir");
        let bytes = artifact_id.as_bytes();
        let library_path = dir.join(ORT_RUNTIME_FILENAME);
        fs::write(&library_path, bytes).expect("write library");

        let mut record = record_from_catalog_runtime(runtime, catalog);
        record.artifact_id = artifact_id.to_owned();
        record.files = vec![InstalledFileRecord {
            path: ORT_RUNTIME_FILENAME.to_owned(),
            size: bytes.len() as u64,
            sha256: sha256_hex(bytes),
        }];
        write_artifact_record(&dir.join(RUNTIME_RECORD_FILENAME), &record).expect("write record");
        library_path
    }

    #[test]
    fn resolve_and_load_reports_nothing_when_no_runtime_is_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = ScriptedLoader::new();

        let loaded =
            resolve_and_load_with(&strategy(&loader), tmp.path(), CandidateProof::LoadHere)
                .expect("resolution should succeed");

        assert_eq!(loaded, None);
        assert!(loader.attempts().is_empty());
    }

    #[test]
    fn startup_acknowledges_a_promoted_candidate_without_loading_it() {
        let tmp = tempfile::tempdir().unwrap();
        install(tmp.path(), "rt-old");
        install(tmp.path(), "rt-new");
        write_slots(
            tmp.path(),
            &RuntimeSlots {
                active: Some("rt-old".to_owned()),
                candidate: Some("rt-new".to_owned()),
                ..RuntimeSlots::default()
            },
        )
        .expect("write slots");
        let loader = ScriptedLoader::new();

        let loaded =
            resolve_and_load_with(&strategy(&loader), tmp.path(), CandidateProof::WorkerProbe)
                .expect("startup activation should succeed");

        assert_eq!(loaded, None, "the worker probe is authoritative");
        assert!(
            loader.attempts().is_empty(),
            "startup must not probe a promoted candidate in this process"
        );
        let slots = read_slots(tmp.path());
        assert_eq!(slots.active.as_deref(), Some("rt-new"));
        assert!(!slots.activation_pending);
    }

    #[test]
    fn recovery_proves_a_promoted_candidate_by_loading_it() {
        let tmp = tempfile::tempdir().unwrap();
        let candidate = install(tmp.path(), "rt-new");
        write_slots(
            tmp.path(),
            &RuntimeSlots {
                candidate: Some("rt-new".to_owned()),
                ..RuntimeSlots::default()
            },
        )
        .expect("write slots");
        let loader = ScriptedLoader::new();

        let loaded =
            resolve_and_load_with(&strategy(&loader), tmp.path(), CandidateProof::LoadHere)
                .expect("recovery should succeed");

        assert_eq!(loaded, Some(candidate.clone()));
        assert_eq!(loader.attempts(), vec![candidate]);
        let slots = read_slots(tmp.path());
        assert_eq!(slots.active.as_deref(), Some("rt-new"));
        assert!(
            !slots.activation_pending,
            "a proved candidate is acknowledged"
        );
    }

    #[test]
    fn a_failed_active_runtime_rolls_back_to_the_previous_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let broken = install(tmp.path(), "rt-new");
        let previous = install(tmp.path(), "rt-old");
        write_slots(
            tmp.path(),
            &RuntimeSlots {
                active: Some("rt-new".to_owned()),
                previous: Some("rt-old".to_owned()),
                ..RuntimeSlots::default()
            },
        )
        .expect("write slots");
        let loader =
            ScriptedLoader::new().script(&broken, Behaviour::Fail("LoadLibraryExW failed"));

        let loaded =
            resolve_and_load_with(&strategy(&loader), tmp.path(), CandidateProof::WorkerProbe)
                .expect("rollback should yield a usable runtime");

        assert_eq!(loaded, Some(previous.clone()));
        assert_eq!(loader.attempts(), vec![broken, previous]);
        let slots = read_slots(tmp.path());
        assert_eq!(slots.active.as_deref(), Some("rt-old"));
        assert_eq!(
            slots.last_failure.as_ref().map(|f| f.artifact_id.as_str()),
            Some("rt-new")
        );
        assert!(!runtime_artifact_dir(tmp.path(), "rt-new").exists());
    }

    #[test]
    fn a_failed_active_runtime_without_a_previous_generation_reports_the_probe_phase() {
        let tmp = tempfile::tempdir().unwrap();
        let broken = install(tmp.path(), "rt-only");
        write_slots(
            tmp.path(),
            &RuntimeSlots {
                active: Some("rt-only".to_owned()),
                ..RuntimeSlots::default()
            },
        )
        .expect("write slots");
        let loader =
            ScriptedLoader::new().script(&broken, Behaviour::Fail("LoadLibraryExW failed"));

        let error =
            resolve_and_load_with(&strategy(&loader), tmp.path(), CandidateProof::WorkerProbe)
                .expect_err("a load failure with no fallback must surface");

        assert_eq!(
            failure_phase_of(&error),
            Some(RuntimeBootstrapFailurePhase::Probe),
            "a load failure is never a download failure"
        );
        assert_eq!(error.to_string(), "LoadLibraryExW failed");
        assert_eq!(read_slots(tmp.path()).active, None);
    }

    #[test]
    fn a_fresh_install_that_fails_to_load_leaves_the_slots_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let active = install(tmp.path(), "rt-active");
        let fresh = install(tmp.path(), "rt-fresh");
        write_slots(
            tmp.path(),
            &RuntimeSlots {
                active: Some("rt-active".to_owned()),
                candidate: Some("rt-fresh".to_owned()),
                ..RuntimeSlots::default()
            },
        )
        .expect("write slots");
        let loader = ScriptedLoader::new().script(&fresh, Behaviour::Fail("bad image"));

        let error = load_with_watchdog_using(
            &strategy(&loader),
            tmp.path(),
            &fresh,
            ActivationTarget {
                artifact_id: Some("rt-fresh"),
                execution_providers: None,
            },
        )
        .expect_err("a fresh install that cannot load must surface");

        assert_eq!(
            failure_phase_of(&error),
            Some(RuntimeBootstrapFailurePhase::Probe)
        );
        assert_eq!(loader.attempts(), vec![fresh]);
        let slots = read_slots(tmp.path());
        assert_eq!(slots.active.as_deref(), Some("rt-active"));
        assert_eq!(slots.candidate.as_deref(), Some("rt-fresh"));
        assert!(runtime_artifact_dir(tmp.path(), "rt-fresh").exists());
        assert!(active.is_file());
    }

    #[test]
    fn a_load_that_outlives_the_watchdog_reports_a_probe_timeout() {
        let tmp = tempfile::tempdir().unwrap();
        let stalled = install(tmp.path(), "rt-stalled");
        let loader = ScriptedLoader::new().script(&stalled, Behaviour::Stall);

        let error = load_with_watchdog_using(
            &strategy(&loader),
            tmp.path(),
            &stalled,
            ActivationTarget::default(),
        )
        .expect_err("a stalled load must time out");

        assert_eq!(
            failure_phase_of(&error),
            Some(RuntimeBootstrapFailurePhase::Probe)
        );
        assert!(
            error
                .to_string()
                .starts_with(RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER),
            "unexpected message: {error}"
        );
    }

    #[test]
    fn a_panicking_load_releases_the_latch_so_a_retry_can_proceed() {
        let tmp = tempfile::tempdir().unwrap();
        let flaky = install(tmp.path(), "rt-panics");
        let loader = ScriptedLoader::new().script(&flaky, Behaviour::Panic);
        let strategy = strategy(&loader);

        let error =
            load_with_watchdog_using(&strategy, tmp.path(), &flaky, ActivationTarget::default())
                .expect_err("a panicking load must surface an error");
        let message = error.to_string();
        assert!(
            message.contains("panicked") && message.contains("scripted load panic"),
            "unexpected message: {error}"
        );
        assert_eq!(
            failure_phase_of(&error),
            Some(RuntimeBootstrapFailurePhase::Probe)
        );

        loader.script(&flaky, Behaviour::Succeed);
        let loaded =
            load_with_watchdog_using(&strategy, tmp.path(), &flaky, ActivationTarget::default())
                .expect("the in-progress latch must be released after a panicking load");
        assert_eq!(loaded, flaky);
    }

    #[test]
    fn a_timed_out_process_refuses_further_loads_without_deleting_a_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let stalled = install(tmp.path(), "rt-stalled");
        let active = install(tmp.path(), "rt-active");
        install(tmp.path(), "rt-previous");
        write_slots(
            tmp.path(),
            &RuntimeSlots {
                active: Some("rt-active".to_owned()),
                previous: Some("rt-previous".to_owned()),
                ..RuntimeSlots::default()
            },
        )
        .expect("write slots");
        let loader = ScriptedLoader::new().script(&stalled, Behaviour::Stall);
        let strategy = strategy(&loader);

        load_with_watchdog_using(&strategy, tmp.path(), &stalled, ActivationTarget::default())
            .expect_err("the first load times out");

        let error = load_with_watchdog_using(
            &strategy,
            tmp.path(),
            &active,
            ActivationTarget {
                artifact_id: Some("rt-active"),
                execution_providers: None,
            },
        )
        .expect_err("a poisoned process cannot load another runtime");

        assert_eq!(
            failure_phase_of(&error),
            Some(RuntimeBootstrapFailurePhase::Probe)
        );
        let slots = read_slots(tmp.path());
        assert_eq!(
            slots.active.as_deref(),
            Some("rt-active"),
            "a poisoned process implicates no artifact, so nothing rolls back"
        );
        assert!(runtime_artifact_dir(tmp.path(), "rt-active").exists());
    }

    #[test]
    fn a_directml_runtime_timeout_records_the_cpu_fallback() {
        let _guard = DIRECTML_TIMEOUT_TEST_LOCK.lock().unwrap();
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
        let tmp = tempfile::tempdir().unwrap();
        let stalled = install(tmp.path(), "rt-directml");
        let loader = ScriptedLoader::new().script(&stalled, Behaviour::Stall);
        let providers = vec!["cpu".to_owned(), "directml".to_owned()];

        load_with_watchdog_using(
            &strategy(&loader),
            tmp.path(),
            &stalled,
            ActivationTarget {
                artifact_id: Some("rt-directml"),
                execution_providers: Some(&providers),
            },
        )
        .expect_err("a stalled load must time out");

        let config = crate::config::load_config(tmp.path())
            .expect("config should load")
            .expect("the fallback must be persisted");
        assert_eq!(
            config.directml_disabled_by_runtime_timeout.as_deref(),
            Some("directml-runtime-load-timeout")
        );
        assert!(crate::platform_capabilities::directml_disabled_by_timeout());
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
    }

    #[test]
    fn a_cpu_only_runtime_timeout_leaves_directml_enabled() {
        let _guard = DIRECTML_TIMEOUT_TEST_LOCK.lock().unwrap();
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
        let tmp = tempfile::tempdir().unwrap();
        let stalled = install(tmp.path(), "rt-cpu");
        let loader = ScriptedLoader::new().script(&stalled, Behaviour::Stall);
        let providers = vec!["cpu".to_owned()];

        load_with_watchdog_using(
            &strategy(&loader),
            tmp.path(),
            &stalled,
            ActivationTarget {
                artifact_id: Some("rt-cpu"),
                execution_providers: Some(&providers),
            },
        )
        .expect_err("a stalled load must time out");

        assert!(crate::config::load_config(tmp.path())
            .expect("config should load")
            .is_none());
        assert!(!crate::platform_capabilities::directml_disabled_by_timeout());
    }

    #[test]
    fn a_successful_load_acknowledges_a_pending_activation() {
        let tmp = tempfile::tempdir().unwrap();
        let pending = install(tmp.path(), "rt-pending");
        write_slots(
            tmp.path(),
            &RuntimeSlots {
                active: Some("rt-pending".to_owned()),
                activation_pending: true,
                activation_attempts: 1,
                ..RuntimeSlots::default()
            },
        )
        .expect("write slots");
        let loader = ScriptedLoader::new();

        let loaded = load_with_watchdog_using(
            &strategy(&loader),
            tmp.path(),
            &pending,
            ActivationTarget {
                artifact_id: Some("rt-pending"),
                execution_providers: None,
            },
        )
        .expect("the load should succeed");

        assert_eq!(loaded, pending);
        let slots = read_slots(tmp.path());
        assert!(!slots.activation_pending);
        assert_eq!(slots.activation_attempts, 0);
    }

    #[test]
    fn a_legacy_install_that_fails_to_load_never_touches_a_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("runtime").join(ORT_RUNTIME_FILENAME);
        fs::create_dir_all(legacy.parent().unwrap()).expect("legacy dir");
        fs::write(&legacy, b"legacy").expect("write legacy");
        write_slots(
            tmp.path(),
            &RuntimeSlots {
                active: Some("rt-active".to_owned()),
                ..RuntimeSlots::default()
            },
        )
        .expect("write slots");
        let loader = ScriptedLoader::new().script(&legacy, Behaviour::Fail("dlopen failed"));

        load_with_watchdog_using(
            &strategy(&loader),
            tmp.path(),
            &legacy,
            ActivationTarget::default(),
        )
        .expect_err("a legacy load failure must surface");

        assert_eq!(read_slots(tmp.path()).active.as_deref(), Some("rt-active"));
    }

    #[test]
    fn runtime_dll_search_dir_is_the_library_parent() {
        let path = Path::new("/tmp/runtimes/rt-a").join(ORT_RUNTIME_FILENAME);
        assert_eq!(
            runtime_dll_search_dir(&path),
            Some(Path::new("/tmp/runtimes/rt-a"))
        );
        assert_eq!(
            runtime_dll_search_dir(Path::new(ORT_RUNTIME_FILENAME)),
            None
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn describe_win32_load_error_includes_code_and_known_hint() {
        // The most common cause of an instant LoadLibraryExW failure for a
        // /MD-built onnxruntime.dll on a stripped Server image is a missing
        // VC++ runtime dependency.
        let missing = describe_win32_load_error(126);
        assert!(
            missing.contains("vcruntime140") && missing.contains("msvcp140"),
            "missing-dep hint should name the app-local CRT DLLs: {missing}"
        );
        assert!(
            missing.contains("126") && missing.contains("0x0000007E"),
            "numeric + hex code should appear: {missing}"
        );

        let bad_arch = describe_win32_load_error(193);
        assert!(
            bad_arch.contains("architecture") || bad_arch.contains("corrupt"),
            "bad-exe-format hint should explain the cause: {bad_arch}"
        );
        assert!(bad_arch.contains("193") && bad_arch.contains("0x000000C1"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn describe_win32_load_error_handles_unknown_codes() {
        // Unknown codes still surface enough detail for triage.
        let unknown = describe_win32_load_error(0x12345678);
        assert!(unknown.contains("305419896"));
        assert!(unknown.contains("0x12345678"));
    }

    #[test]
    fn the_timeout_message_carries_the_marker_and_the_recovery_hint() {
        let message = runtime_load_timeout_message();
        assert!(message.starts_with(RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER));
        assert!(message.contains(RUNTIME_POST_DOWNLOAD_TIMEOUT_HINT));
        assert!(message.contains("120 seconds"));
    }

    #[test]
    fn with_failure_phase_keeps_the_message_and_survives_added_context() {
        let error = with_failure_phase(
            anyhow::anyhow!("LoadLibraryExW failed for onnxruntime.dll"),
            RuntimeBootstrapFailurePhase::Probe,
        );
        assert_eq!(
            error.to_string(),
            "LoadLibraryExW failed for onnxruntime.dll"
        );
        assert_eq!(
            failure_phase_of(&error),
            Some(RuntimeBootstrapFailurePhase::Probe)
        );

        let wrapped = error.context("while preparing separation");
        assert_eq!(
            failure_phase_of(&wrapped),
            Some(RuntimeBootstrapFailurePhase::Probe),
            "the phase must survive contextual wrapping"
        );

        assert_eq!(failure_phase_of(&anyhow::anyhow!("plain error")), None);
    }
}
