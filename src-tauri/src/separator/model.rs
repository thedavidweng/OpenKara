use crate::config::ExecutionProviderPreference;
use anyhow::{Context, Result};
use ort::{session::builder::GraphOptimizationLevel, value::TensorElementType};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Instant,
};

#[cfg(target_os = "windows")]
pub const ORT_RUNTIME_FILENAME: &str = "onnxruntime.dll";
#[cfg(target_os = "linux")]
pub const ORT_RUNTIME_FILENAME: &str = "libonnxruntime.so";
#[cfg(target_vendor = "apple")]
pub const ORT_RUNTIME_FILENAME: &str = "libonnxruntime.dylib";

static ORT_RUNTIME_PATH: OnceLock<PathBuf> = OnceLock::new();
static ORT_RUNTIME_INIT_LOCK: Mutex<()> = Mutex::new(());

const MODEL_CACHE_KEY_METADATA: &str = "openkara.model_cache_key";
const OPTIMIZED_BY_METADATA: &str = "openkara.optimized_by";
const TENSOR_INTERFACE_METADATA: &str = "openkara.tensor_interface";
const SPECTRAL_CONTRACT_METADATA: &str = "openkara.spectral_contract";
const ONNXRUNTIME_OPTIMIZED_BY_VALUE: &str = "onnxruntime";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ModelRuntimeMetadata {
    pub model_cache_key: Option<String>,
    pub optimized_by: Option<String>,
    pub tensor_interface: Option<String>,
    pub spectral_contract: Option<String>,
}

/// Validate that a model's embedded metadata declares the spectral-core
/// interface at a supported contract version.
///
/// The spectral-core boundary (`openkara.spectral-contract/v1`) is the ONLY
/// production separation path (issue #172): the app runs the transforms
/// (`separator::spectral`) and the graph consumes / produces the contract
/// spectral tensors. The legacy waveform graph path has been removed, so a
/// model with no interface declaration or the `waveform` declaration is
/// refused HERE, before any ORT session is created.
pub(crate) fn ensure_spectral_core_metadata(metadata: &ModelRuntimeMetadata) -> Result<()> {
    match metadata.tensor_interface.as_deref() {
        Some("spectral-core") => {
            let contract = metadata.spectral_contract.as_deref();
            anyhow::ensure!(
                contract == Some(crate::separator::spectral::SPECTRAL_CONTRACT_VERSION),
                "spectral-core model declares unsupported contract {:?} \
                 (this build implements {}); refusing before session creation",
                contract,
                crate::separator::spectral::SPECTRAL_CONTRACT_VERSION
            );
            Ok(())
        }
        None | Some("waveform") => anyhow::bail!(
            "waveform-interface models are no longer supported; \
             install a spectral-core bundle from the catalog"
        ),
        Some(other) => anyhow::bail!(
            "model declares unknown tensor interface {other:?}; \
             refusing before session creation"
        ),
    }
}

pub struct LoadedModel {
    pub model_path: PathBuf,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub input_shape: Vec<i64>,
    pub input_tensor_type: TensorElementType,
    /// Verified spectral-core session interface (issue #172). Every loaded
    /// model runs the app-side transforms and consumes / produces the
    /// contract spectral tensors; verification happens at load time so a
    /// non-conforming graph fails before any separation starts.
    pub spectral: crate::separator::spectral_session::SpectralInterface,
    // Mutex: ort::Session::run needs exclusive access despite Session being Send.
    pub(crate) session: std::sync::Mutex<ort::session::Session>,
}

impl std::fmt::Debug for LoadedModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedModel")
            .field("model_path", &self.model_path)
            .field("inputs", &self.inputs)
            .field("outputs", &self.outputs)
            .field("input_shape", &self.input_shape)
            .field("input_tensor_type", &self.input_tensor_type)
            .finish_non_exhaustive()
    }
}

pub fn default_model_path_for_filename(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("models")
        .join(filename)
}

/// The development-fallback path of the standard model, resolved through
/// the catalog descriptor so the filename tracks the pinned artifact.
pub fn default_model_path() -> PathBuf {
    let descriptor =
        crate::separator::bootstrap::descriptor_for(crate::config::ModelVariant::Htdemucs);
    default_model_path_for_filename(&descriptor.filename)
}

/// The runtime library committed into this process, when one is loaded.
/// ORT cannot be unloaded or swapped in place — a different runtime only
/// takes effect after a restart.
pub fn loaded_runtime_path() -> Option<&'static Path> {
    ORT_RUNTIME_PATH.get().map(|path| path.as_path())
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

/// Load the bundled DirectML companion only when a DirectML session is
/// requested. ORT resolves provider libraries by module name, so the exact
/// artifact path must be preloaded before provider registration.
#[cfg(target_os = "windows")]
fn preload_directml_companion() -> Result<()> {
    let runtime_path = ORT_RUNTIME_PATH
        .get()
        .context("ONNX Runtime path is not available for DirectML setup")?;
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

fn init_ort_from_path(runtime_path: &Path) -> Result<()> {
    anyhow::ensure!(
        runtime_path.is_file(),
        "ONNX Runtime library is missing at {}",
        runtime_path.display()
    );

    #[cfg(target_os = "windows")]
    {
        prepare_windows_runtime_dll_search(runtime_path)?;
        // Synchronously read the library into the page cache before the loader
        // maps it. On some Windows VMs (e.g. PVE/KVM), a real-time AV scan or a
        // cold/slow virtual disk stalls section mapping inside `LoadLibraryW`
        // long enough to trip the load watchdog; reading first lets the scan
        // finish and warms the disk so the subsequent load is fast. Read errors
        // are ignored — this is a best-effort hint that must never block the
        // load.
        let _ = fs::read(runtime_path);
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

pub(crate) fn read_model_runtime_metadata(path: &Path) -> Result<ModelRuntimeMetadata> {
    let bytes =
        fs::read(path).with_context(|| format!("failed to read model file {}", path.display()))?;
    Ok(parse_model_runtime_metadata(&bytes))
}

pub(crate) fn session_cache_key(
    model_path: &Path,
    provider: ExecutionProviderPreference,
    metadata: &ModelRuntimeMetadata,
) -> String {
    let mut key = match metadata.model_cache_key.as_deref() {
        Some(model_cache_key) => format!(
            "{}::{}::{}",
            model_path.display(),
            provider.as_str(),
            model_cache_key
        ),
        None => format!("{}::{}", model_path.display(), provider.as_str()),
    };
    // Spectral-core session keys include contract revision (#172); waveform keys unchanged.
    if metadata.tensor_interface.as_deref() == Some("spectral-core") {
        if let Some(contract) = metadata.spectral_contract.as_deref() {
            key.push_str("::");
            key.push_str(contract);
        }
    }
    key
}

pub fn load_from_path(
    path: &Path,
    ep_preference: ExecutionProviderPreference,
) -> Result<LoadedModel> {
    tracing::info!(
        "attempting ONNX session load for {} via {}",
        path.display(),
        provider_diagnostic_summary(ep_preference)
    );

    let provider_chain = execution_provider_chain(ep_preference);
    let mut last_error = None;

    for (index, provider) in provider_chain.iter().copied().enumerate() {
        match load_with_ep(path, provider) {
            Ok(model) => {
                if index > 0 {
                    tracing::warn!(
                        "recovered ONNX session load by falling back to {} for {}",
                        provider.as_str(),
                        path.display()
                    );
                }
                return Ok(model);
            }
            Err(error) => {
                if index + 1 < provider_chain.len() {
                    tracing::warn!(
                        "ONNX session load failed with {} for {}: {error:#}",
                        provider.as_str(),
                        path.display()
                    );
                }
                last_error = Some(error);
            }
        }
    }

    Err(last_error.expect("provider chain should contain at least one provider"))
}

pub fn provider_diagnostic_summary(preference: ExecutionProviderPreference) -> String {
    execution_provider_chain(preference)
        .into_iter()
        .map(|provider| provider.as_str())
        .collect::<Vec<_>>()
        .join(" -> ")
}

/// Benchmark/diagnostic override for the ORT intra-op thread count.
///
/// Setting `OPENKARA_INTRA_THREADS` forces a specific count; it is how the
/// #170 thread sweep was measured and how the CI bench harness can sweep
/// threads later. It is never set in production launches.
const INTRA_THREADS_ENV: &str = "OPENKARA_INTRA_THREADS";

/// Parse the `OPENKARA_INTRA_THREADS` override into a validated thread count.
///
/// Returns `None` when the variable is absent, unparsable, or `< 1`, so that
/// an empty or garbage value falls through to the measured default rather than
/// forcing a degenerate thread count.
fn parse_intra_threads_override(raw: Option<String>) -> Option<usize> {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|count| *count >= 1)
}

/// Physical performance-core count, when the platform exposes it.
///
/// Apple Silicon partitions its cores into "perf levels": `perflevel0` is the
/// performance-core cluster and `perflevel1` the efficiency cluster. Reading
/// `hw.perflevel0.physicalcpu` yields the P-core count directly. Returns `None`
/// on any sysctl error.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn performance_core_count() -> Option<usize> {
    let mut value: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    let name = c"hw.perflevel0.physicalcpu";
    // SAFETY: `name` is a valid NUL-terminated C string; `value` and `size`
    // point to a live `c_int` and its byte length. sysctlbyname writes at most
    // `size` bytes into `value` and updates `size` with the bytes written.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            &mut value as *mut libc::c_int as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc == 0 && value >= 1 {
        Some(value as usize)
    } else {
        None
    }
}

/// Non-Apple-Silicon stub: no performance-core signal is available, so the
/// caller keeps the previous `available.min(8)` policy unchanged on every other
/// target.
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn performance_core_count() -> Option<usize> {
    None
}

/// Choose the ORT intra-op thread count.
///
/// Policy, in priority order (measured, #170):
///   1. an explicit `override_value` (`>= 1`) always wins — the bench/diagnostic knob;
///   2. else the physical performance-core count when the platform exposes it
///      (Apple Silicon: `hw.perflevel0.physicalcpu`);
///   3. else the historical `available.min(8)` fallback.
///
/// The result is floored at 1.
///
/// Why performance cores on Apple Silicon: a decisive alternating A/B benchmark
/// on an M3 (4 P-cores + 4 E-cores, order 8/4/8/4/8/4 with cooldowns so thermal
/// drift cancels) had t=4 (the P-core count) beat t=8 (all logical cores) on
/// ALL SIX pairwise runs, for both the CPU EP and XNNPACK. `num_threads` also
/// sizes the XNNPACK worker pool (see `build_execution_provider_list`), so one
/// policy covers both EPs. Intra-op threads that spill onto the efficiency
/// cores hurt this latency-sensitive workload; the P-core count is the right
/// size. Every non-Apple-Silicon target has a `None` performance-core signal,
/// so its behavior is exactly the previous `available.min(8)` policy.
fn intra_thread_count(
    override_value: Option<usize>,
    performance_cores: Option<usize>,
    available: usize,
) -> usize {
    override_value
        .filter(|count| *count >= 1)
        .or_else(|| performance_cores.filter(|count| *count >= 1))
        .unwrap_or_else(|| available.min(8))
        .max(1)
}

fn load_with_ep(path: &Path, ep_preference: ExecutionProviderPreference) -> Result<LoadedModel> {
    anyhow::ensure!(
        ORT_RUNTIME_PATH.get().is_some(),
        "ONNX Runtime is not initialized; the managed runtime bootstrap must complete before model loading"
    );
    let runtime_metadata = read_model_runtime_metadata(path)?;
    // Fail unsupported spectral contracts before creating an ORT session (#172).
    ensure_spectral_core_metadata(&runtime_metadata)
        .with_context(|| format!("cannot load model {}", path.display()))?;

    let model_path = path.to_path_buf();
    // P-cores on Apple Silicon; else `available.min(8)`. Override: `OPENKARA_INTRA_THREADS`.
    let available = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let num_threads = intra_thread_count(
        parse_intra_threads_override(std::env::var(INTRA_THREADS_ENV).ok()),
        performance_core_count(),
        available,
    );

    let mut builder =
        ort::session::Session::builder().context("failed to create ONNX session builder")?;

    builder = builder
        .with_intra_threads(num_threads)
        .map_err(|e| anyhow::anyhow!("failed to set intra-op thread count: {e}"))?;

    // XNNPACK has its own pool; disable ORT intra-op spinning to avoid oversubscription.
    if matches!(ep_preference, ExecutionProviderPreference::Xnnpack) {
        builder = builder
            .with_intra_op_spinning(false)
            .map_err(|e| anyhow::anyhow!("failed to disable intra-op spinning: {e}"))?;
    }

    builder = builder
        .with_optimization_level(graph_optimization_level_for(&runtime_metadata))
        .map_err(|e| anyhow::anyhow!("failed to set graph optimization level: {e}"))?;

    #[cfg(target_os = "windows")]
    if matches!(ep_preference, ExecutionProviderPreference::DirectMl) {
        preload_directml_companion()?;
    }

    let ep_list = build_execution_provider_list(ep_preference, num_threads);
    if !ep_list.is_empty() {
        builder = builder
            .with_execution_providers(ep_list)
            .map_err(|e| anyhow::anyhow!("failed to configure execution providers: {e}"))?;
    }

    tracing::info!(
        "committing ONNX session for {} (provider preference: {})",
        path.display(),
        ep_preference.as_str()
    );
    let commit_start = Instant::now();
    let session = builder
        .commit_from_file(path)
        .with_context(|| format!("failed to load ONNX model from {}", path.display()))?;
    tracing::info!(
        "committed ONNX session for {} in {:?}",
        path.display(),
        commit_start.elapsed()
    );

    let inputs: Vec<String> = session
        .inputs()
        .iter()
        .map(|input| input.name().to_owned())
        .collect();
    let outputs: Vec<String> = session
        .outputs()
        .iter()
        .map(|output| output.name().to_owned())
        .collect();
    let input_spec = session
        .inputs()
        .first()
        .context("model did not expose any inputs")?;
    let input_shape = input_spec
        .dtype()
        .tensor_shape()
        .context("model input is not a tensor")?
        .iter()
        .copied()
        .collect();
    let input_tensor_type = input_spec
        .dtype()
        .tensor_type()
        .context("model input tensor type is missing")?;

    let spectral = {
        let input_infos: Vec<(String, Vec<i64>)> = session
            .inputs()
            .iter()
            .map(|io| {
                let dims = io
                    .dtype()
                    .tensor_shape()
                    .map(|s| s.iter().copied().collect())
                    .unwrap_or_default();
                (io.name().to_owned(), dims)
            })
            .collect();
        let output_infos: Vec<(String, Vec<i64>)> = session
            .outputs()
            .iter()
            .map(|io| {
                let dims = io
                    .dtype()
                    .tensor_shape()
                    .map(|s| s.iter().copied().collect())
                    .unwrap_or_default();
                (io.name().to_owned(), dims)
            })
            .collect();
        crate::separator::spectral_session::verify_spectral_interface(&input_infos, &output_infos)
            .with_context(|| {
            format!(
                "model {} declares spectral-core but its graph does not \
                     match the contract tensor interface",
                path.display()
            )
        })?
    };

    Ok(LoadedModel {
        model_path,
        inputs,
        outputs,
        input_shape,
        input_tensor_type,
        spectral,
        session: std::sync::Mutex::new(session),
    })
}

fn build_execution_provider_list(
    preference: ExecutionProviderPreference,
    num_threads: usize,
) -> Vec<ort::ep::ExecutionProviderDispatch> {
    use ort::ep;
    use std::num::NonZeroUsize;

    match preference {
        // Empty list means ORT uses the built-in CPU EP.
        ExecutionProviderPreference::Cpu => vec![],
        // Align XNNPACK workers with ORT intra-op threads.
        ExecutionProviderPreference::Xnnpack => vec![ep::XNNPACK::default()
            .with_intra_op_num_threads(
                NonZeroUsize::new(num_threads).expect("num_threads is non-zero"),
            )
            .build()],
        ExecutionProviderPreference::CoreMl => vec![ep::CoreML::default().build()],
        ExecutionProviderPreference::DirectMl => vec![ep::DirectML::default().build()],
    }
}

fn execution_provider_chain(
    preference: ExecutionProviderPreference,
) -> Vec<ExecutionProviderPreference> {
    match preference {
        ExecutionProviderPreference::Xnnpack => vec![
            ExecutionProviderPreference::Xnnpack,
            ExecutionProviderPreference::Cpu,
        ],
        ExecutionProviderPreference::DirectMl => vec![
            ExecutionProviderPreference::DirectMl,
            ExecutionProviderPreference::Cpu,
        ],
        ExecutionProviderPreference::CoreMl => vec![
            ExecutionProviderPreference::CoreMl,
            ExecutionProviderPreference::Cpu,
        ],
        resolved => vec![resolved],
    }
}

fn parse_model_runtime_metadata(bytes: &[u8]) -> ModelRuntimeMetadata {
    // Read only metadata_props (avoids a full protobuf dependency).
    let mut metadata = ModelRuntimeMetadata::default();
    let mut cursor = 0;

    while let Some(tag) = decode_varint(bytes, &mut cursor) {
        let field_number = tag >> 3;
        let wire_type = (tag & 0x07) as u8;

        if field_number == 14 && wire_type == 2 {
            let Some(entry_bytes) = read_length_delimited(bytes, &mut cursor) else {
                break;
            };
            let Some((key, value)) = parse_string_string_entry(entry_bytes) else {
                continue;
            };

            match key.as_str() {
                MODEL_CACHE_KEY_METADATA => metadata.model_cache_key = Some(value),
                OPTIMIZED_BY_METADATA => metadata.optimized_by = Some(value),
                TENSOR_INTERFACE_METADATA => metadata.tensor_interface = Some(value),
                SPECTRAL_CONTRACT_METADATA => metadata.spectral_contract = Some(value),
                _ => {}
            }
            continue;
        }

        if !skip_field(bytes, &mut cursor, wire_type) {
            break;
        }
    }

    metadata
}

fn graph_optimization_level_for(metadata: &ModelRuntimeMetadata) -> GraphOptimizationLevel {
    if metadata.optimized_by.as_deref() == Some(ONNXRUNTIME_OPTIMIZED_BY_VALUE) {
        GraphOptimizationLevel::Disable
    } else {
        GraphOptimizationLevel::Level3
    }
}

fn parse_string_string_entry(bytes: &[u8]) -> Option<(String, String)> {
    let mut cursor = 0;
    let mut key = None;
    let mut value = None;

    while let Some(tag) = decode_varint(bytes, &mut cursor) {
        let field_number = tag >> 3;
        let wire_type = (tag & 0x07) as u8;

        match (field_number, wire_type) {
            (1, 2) => {
                let entry = read_length_delimited(bytes, &mut cursor)?;
                key = Some(std::str::from_utf8(entry).ok()?.to_owned());
            }
            (2, 2) => {
                let entry = read_length_delimited(bytes, &mut cursor)?;
                value = Some(std::str::from_utf8(entry).ok()?.to_owned());
            }
            _ => {
                if !skip_field(bytes, &mut cursor, wire_type) {
                    return None;
                }
            }
        }
    }

    Some((key?, value?))
}

fn decode_varint(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
    let mut value = 0_u64;
    let mut shift = 0_u32;

    while *cursor < bytes.len() && shift < 64 {
        let byte = bytes[*cursor];
        *cursor += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }

    None
}

fn read_length_delimited<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    let length = decode_varint(bytes, cursor)? as usize;
    let end = cursor.checked_add(length)?;
    if end > bytes.len() {
        return None;
    }

    let slice = &bytes[*cursor..end];
    *cursor = end;
    Some(slice)
}

fn skip_field(bytes: &[u8], cursor: &mut usize, wire_type: u8) -> bool {
    match wire_type {
        0 => decode_varint(bytes, cursor).is_some(),
        1 => advance_cursor(bytes, cursor, 8),
        2 => read_length_delimited(bytes, cursor).is_some(),
        5 => advance_cursor(bytes, cursor, 4),
        _ => false,
    }
}

fn advance_cursor(bytes: &[u8], cursor: &mut usize, length: usize) -> bool {
    let Some(end) = cursor.checked_add(length) else {
        return false;
    };
    if end > bytes.len() {
        return false;
    }

    *cursor = end;
    true
}

#[cfg(test)]
fn encode_varint(mut value: u64, bytes: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            break;
        }
    }
}

#[cfg(test)]
fn encode_length_delimited(payload: &[u8], bytes: &mut Vec<u8>) {
    encode_varint(payload.len() as u64, bytes);
    bytes.extend_from_slice(payload);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ort::session::builder::GraphOptimizationLevel;

    fn metadata_entry_bytes(key: &str, value: &str) -> Vec<u8> {
        let mut entry = Vec::new();
        encode_varint((1_u64 << 3) | 2, &mut entry);
        encode_length_delimited(key.as_bytes(), &mut entry);
        encode_varint((2_u64 << 3) | 2, &mut entry);
        encode_length_delimited(value.as_bytes(), &mut entry);
        entry
    }

    fn model_with_metadata(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (key, value) in entries {
            let entry = metadata_entry_bytes(key, value);
            encode_varint((14_u64 << 3) | 2, &mut bytes);
            encode_length_delimited(&entry, &mut bytes);
        }
        bytes
    }

    #[test]
    fn provider_chain_keeps_xnnpack_cpu_fallback() {
        assert_eq!(
            execution_provider_chain(ExecutionProviderPreference::Xnnpack),
            vec![
                ExecutionProviderPreference::Xnnpack,
                ExecutionProviderPreference::Cpu,
            ]
        );
    }

    #[test]
    fn provider_chain_keeps_directml_cpu_fallback() {
        assert_eq!(
            execution_provider_chain(ExecutionProviderPreference::DirectMl),
            vec![
                ExecutionProviderPreference::DirectMl,
                ExecutionProviderPreference::Cpu,
            ]
        );
    }

    #[test]
    fn provider_chain_keeps_coreml_cpu_fallback() {
        assert_eq!(
            execution_provider_chain(ExecutionProviderPreference::CoreMl),
            vec![
                ExecutionProviderPreference::CoreMl,
                ExecutionProviderPreference::Cpu,
            ]
        );
    }

    #[test]
    fn provider_chain_keeps_cpu_only_when_requested() {
        assert_eq!(
            execution_provider_chain(ExecutionProviderPreference::Cpu),
            vec![ExecutionProviderPreference::Cpu]
        );
    }

    #[test]
    fn parses_openkara_runtime_metadata_from_model_bytes() {
        let metadata = parse_model_runtime_metadata(&model_with_metadata(&[
            ("openkara.model_cache_key", "cache-key-123"),
            ("openkara.optimized_by", "onnxruntime"),
        ]));

        assert_eq!(metadata.model_cache_key.as_deref(), Some("cache-key-123"));
        assert_eq!(metadata.optimized_by.as_deref(), Some("onnxruntime"));
    }

    #[test]
    fn reads_openkara_runtime_metadata_from_downloaded_model_file() {
        let metadata =
            read_model_runtime_metadata(&default_model_path()).expect("model metadata should load");

        assert!(metadata.model_cache_key.is_some());
        assert_eq!(
            metadata.optimized_by.as_deref(),
            Some(ONNXRUNTIME_OPTIMIZED_BY_VALUE)
        );
    }

    #[test]
    fn optimized_model_metadata_disables_graph_optimization() {
        let metadata = ModelRuntimeMetadata {
            model_cache_key: Some("cache-key-123".to_owned()),
            optimized_by: Some("onnxruntime".to_owned()),
            ..Default::default()
        };

        assert_eq!(
            graph_optimization_level_for(&metadata),
            GraphOptimizationLevel::Disable
        );
    }

    #[test]
    fn session_cache_key_includes_model_cache_key_when_present() {
        let model_path = Path::new("/tmp/models/htdemucs.onnx");
        let metadata = ModelRuntimeMetadata {
            model_cache_key: Some("cache-key-123".to_owned()),
            optimized_by: None,
            ..Default::default()
        };

        assert_eq!(
            session_cache_key(model_path, ExecutionProviderPreference::Xnnpack, &metadata),
            "/tmp/models/htdemucs.onnx::xnnpack::cache-key-123"
        );
    }

    #[test]
    fn parses_spectral_interface_metadata_from_model_bytes() {
        let metadata = parse_model_runtime_metadata(&model_with_metadata(&[
            ("openkara.tensor_interface", "spectral-core"),
            (
                "openkara.spectral_contract",
                "openkara.spectral-contract/v1",
            ),
        ]));

        assert_eq!(metadata.tensor_interface.as_deref(), Some("spectral-core"));
        assert_eq!(
            metadata.spectral_contract.as_deref(),
            Some("openkara.spectral-contract/v1")
        );
    }

    #[test]
    fn absent_interface_metadata_is_refused() {
        let metadata = ModelRuntimeMetadata::default();
        let error = ensure_spectral_core_metadata(&metadata)
            .expect_err("absent interface must be rejected; waveform path is gone");
        assert!(error
            .to_string()
            .contains("waveform-interface models are no longer supported"));
    }

    #[test]
    fn waveform_interface_metadata_is_refused() {
        let metadata = ModelRuntimeMetadata {
            tensor_interface: Some("waveform".to_owned()),
            ..Default::default()
        };
        let error = ensure_spectral_core_metadata(&metadata)
            .expect_err("waveform interface must be rejected before session creation");
        assert!(error
            .to_string()
            .contains("waveform-interface models are no longer supported"));
    }

    #[test]
    fn spectral_interface_requires_the_supported_contract_version() {
        let mut metadata = ModelRuntimeMetadata {
            tensor_interface: Some("spectral-core".to_owned()),
            ..Default::default()
        };
        ensure_spectral_core_metadata(&metadata)
            .expect_err("missing contract version must fail before session creation");

        metadata.spectral_contract = Some("openkara.spectral-contract/v2".to_owned());
        ensure_spectral_core_metadata(&metadata)
            .expect_err("unsupported contract version must fail before session creation");

        metadata.spectral_contract =
            Some(crate::separator::spectral::SPECTRAL_CONTRACT_VERSION.to_owned());
        ensure_spectral_core_metadata(&metadata).expect("supported contract");
    }

    #[test]
    fn unknown_tensor_interface_fails_before_session_creation() {
        let metadata = ModelRuntimeMetadata {
            tensor_interface: Some("holographic".to_owned()),
            ..Default::default()
        };
        let error = ensure_spectral_core_metadata(&metadata)
            .expect_err("unknown interface must be rejected");
        assert!(error.to_string().contains("unknown tensor interface"));
    }

    #[test]
    fn spectral_session_cache_key_carries_the_contract_version() {
        let model_path = Path::new("/tmp/models/htdemucs.spectral.onnx");
        let metadata = ModelRuntimeMetadata {
            model_cache_key: Some("cache-key-123".to_owned()),
            optimized_by: Some("onnxruntime".to_owned()),
            tensor_interface: Some("spectral-core".to_owned()),
            spectral_contract: Some("openkara.spectral-contract/v1".to_owned()),
        };

        assert_eq!(
            session_cache_key(model_path, ExecutionProviderPreference::Cpu, &metadata),
            "/tmp/models/htdemucs.spectral.onnx::cpu::cache-key-123::openkara.spectral-contract/v1"
        );
    }

    #[test]
    fn intra_thread_override_wins_over_everything() {
        // An explicit override beats both the performance-core count and the
        // available-parallelism fallback.
        assert_eq!(intra_thread_count(Some(3), Some(4), 16), 3);
        assert_eq!(intra_thread_count(Some(1), Some(8), 8), 1);
    }

    #[test]
    fn intra_thread_zero_override_is_ignored() {
        // A zero override is out of range; policy falls through to the next
        // signal (performance cores here, else the fallback).
        assert_eq!(intra_thread_count(Some(0), Some(4), 16), 4);
        assert_eq!(intra_thread_count(Some(0), None, 16), 8);
    }

    #[test]
    fn intra_thread_uses_performance_cores_when_present() {
        // With no override, the performance-core count is preferred over the
        // available-parallelism fallback (this is the Apple-Silicon path).
        assert_eq!(intra_thread_count(None, Some(4), 8), 4);
        assert_eq!(intra_thread_count(None, Some(6), 16), 6);
    }

    #[test]
    fn intra_thread_falls_back_to_available_min_eight() {
        // No override and no performance-core signal reproduces the historical
        // `available.min(8)` policy exactly (the non-Apple-Silicon path).
        assert_eq!(intra_thread_count(None, None, 16), 8);
        assert_eq!(intra_thread_count(None, None, 8), 8);
        assert_eq!(intra_thread_count(None, None, 4), 4);
    }

    #[test]
    fn intra_thread_is_floored_at_one() {
        assert_eq!(intra_thread_count(None, None, 0), 1);
        assert_eq!(intra_thread_count(Some(0), Some(0), 0), 1);
    }

    #[test]
    fn parses_intra_threads_override_values() {
        assert_eq!(parse_intra_threads_override(Some("4".to_owned())), Some(4));
        assert_eq!(
            parse_intra_threads_override(Some("  6 ".to_owned())),
            Some(6)
        );
        // Out of range, unparsable, and absent all disable the override.
        assert_eq!(parse_intra_threads_override(Some("0".to_owned())), None);
        assert_eq!(
            parse_intra_threads_override(Some("garbage".to_owned())),
            None
        );
        assert_eq!(parse_intra_threads_override(Some("-1".to_owned())), None);
        assert_eq!(parse_intra_threads_override(Some(String::new())), None);
        assert_eq!(parse_intra_threads_override(None), None);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn performance_core_count_reads_apple_silicon_perflevel0() {
        // On Apple Silicon the sysctl must resolve to a real P-core count.
        let cores = performance_core_count().expect("hw.perflevel0.physicalcpu should resolve");
        assert!(cores >= 1, "performance-core count must be at least 1");
    }

    #[test]
    fn runtime_dll_search_dir_is_the_library_parent() {
        let path = PathBuf::from("/data/runtimes/rt-1/onnxruntime.dll");
        assert_eq!(
            runtime_dll_search_dir(&path),
            Some(Path::new("/data/runtimes/rt-1"))
        );
        assert_eq!(runtime_dll_search_dir(Path::new("onnxruntime.dll")), None);
    }
}
