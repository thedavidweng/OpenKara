use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_ABSOLUTE_ERROR: f32 = 1e-3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DeviceKey {
    pub adapter_name: String,
    pub driver_version: String,
    pub runtime_version: String,
    pub runtime_digest: String,
    pub plugin_digest: String,
    pub model_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum ValidationResult {
    Passed { max_absolute_error: f32 },
    Failed { reason: String },
}

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    key: DeviceKey,
    result: ValidationResult,
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn adapter_identity() -> Result<(String, String)> {
    let output = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-r", "-c", "IOAccelerator", "-d", "1", "-l"])
        .output()
        .context("failed to identify the GPU driver")?;
    anyhow::ensure!(output.status.success(), "failed to identify the GPU driver");
    let output = String::from_utf8(output.stdout)?;
    let values = |key: &str| -> Vec<String> {
        let prefix = format!("\"{key}\" = \"");
        output
            .lines()
            .filter_map(|line| line.trim().strip_prefix(&prefix))
            .filter_map(|value| value.strip_suffix('"'))
            .map(str::to_owned)
            .collect()
    };
    let names = values("model");
    let versions = values("IOSourceVersion");
    anyhow::ensure!(
        names.len() == 1 && versions.len() == 1,
        "cannot identify a unique GPU and driver"
    );
    Ok((names[0].clone(), versions[0].clone()))
}

#[cfg(target_os = "windows")]
fn adapter_identity() -> Result<(String, String)> {
    use windows::{
        core::Interface,
        Win32::Graphics::{
            Direct3D::D3D_FEATURE_LEVEL_11_0,
            Direct3D12::{D3D12CreateDevice, ID3D12Device},
            Dxgi::{
                CreateDXGIFactory2, IDXGIAdapter1, IDXGIDevice, IDXGIFactory6,
                DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_CREATE_FACTORY_FLAGS,
                DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
            },
        },
    };
    // Dawn's D3D12 backend uses this same DXGI high-performance ordering.
    // SAFETY: DXGI constructs the COM interfaces and the output arguments are valid.
    let factory =
        unsafe { CreateDXGIFactory2::<IDXGIFactory6>(DXGI_CREATE_FACTORY_FLAGS::default()) }?;
    let mut index = 0;
    let (adapter, description) = loop {
        let adapter: IDXGIAdapter1 = unsafe {
            factory.EnumAdapterByGpuPreference(index, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE)
        }
        .context("no usable high-performance D3D12 adapter")?;
        index += 1;
        let description = unsafe { adapter.GetDesc1() }?;
        if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let mut device = None::<ID3D12Device>;
        if unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.is_ok()
            && device.is_some()
        {
            break (adapter, description);
        }
    };
    let version = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID) }? as u64;
    let name = String::from_utf16_lossy(&description.Description)
        .trim_end_matches('\0')
        .to_owned();
    let version = format!(
        "{}.{}.{}.{}",
        version >> 48,
        (version >> 32) & 0xffff,
        (version >> 16) & 0xffff,
        version & 0xffff
    );
    Ok((name, version))
}

#[cfg(not(any(
    target_os = "windows",
    all(target_os = "macos", target_arch = "aarch64")
)))]
fn adapter_identity() -> Result<(String, String)> {
    bail!("WebGPU is unavailable on this platform")
}

fn runtime_root() -> Result<&'static Path> {
    let runtime =
        crate::separator::activation::loaded_runtime_path().context("runtime is not loaded")?;
    runtime
        .ancestors()
        .find(|directory| {
            directory
                .join(crate::separator::runtime_bootstrap::RUNTIME_RECORD_FILENAME)
                .is_file()
        })
        .context("installed runtime identity is unavailable")
}

pub(crate) fn device_key(model_path: &Path) -> Result<DeviceKey> {
    let (adapter_name, driver_version) = adapter_identity()?;
    let root = runtime_root()?;
    let record = crate::separator::catalog::read_artifact_record(
        &root.join(crate::separator::runtime_bootstrap::RUNTIME_RECORD_FILENAME),
    )
    .context("installed runtime identity is invalid")?;
    let plugin: crate::separator::catalog::CatalogPlugin =
        serde_json::from_slice(&fs::read(root.join("webgpu.json"))?)?;
    let metadata = crate::separator::model::read_model_runtime_metadata(model_path)?;
    let model_key = match metadata.model_cache_key {
        Some(key) => key,
        None => crate::separator::artifacts::sha256_file(model_path)?,
    };
    Ok(DeviceKey {
        adapter_name,
        driver_version,
        runtime_version: record.upstream_version,
        runtime_digest: record.archive_sha256,
        plugin_digest: plugin.archive_digest,
        model_key,
    })
}

fn record_path(directory: &Path, key: &DeviceKey) -> Result<PathBuf> {
    let key_bytes = serde_json::to_vec(key)?;
    let name = crate::separator::verified_manifest::sha256_hex(&key_bytes);
    Ok(directory.join(format!("{name}.json")))
}

pub(crate) fn read(directory: &Path, key: &DeviceKey) -> Result<Option<ValidationResult>> {
    let path = record_path(directory, key)?;
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let record: Record =
        serde_json::from_slice(&bytes).context("invalid WebGPU validation record")?;
    anyhow::ensure!(
        record.key == *key,
        "WebGPU validation record identity mismatch"
    );
    Ok(Some(record.result))
}

pub(crate) fn write(directory: &Path, key: &DeviceKey, result: &ValidationResult) -> Result<()> {
    fs::create_dir_all(directory)?;
    let data = serde_json::to_vec(&Record {
        key: key.clone(),
        result: result.clone(),
    })?;
    crate::config::write_atomically(&record_path(directory, key)?, &data)
}

pub(crate) fn compare(reference: &[f32], candidate: &[f32]) -> Result<f32> {
    anyhow::ensure!(
        !reference.is_empty() && reference.len() == candidate.len(),
        "WebGPU output shape differs from CPU"
    );
    let mut maximum = 0.0_f32;
    for (&cpu, &gpu) in reference.iter().zip(candidate) {
        if !cpu.is_finite() || !gpu.is_finite() {
            bail!("WebGPU or CPU output contains non-finite values");
        }
        maximum = maximum.max((cpu - gpu).abs());
    }
    if maximum > MAX_ABSOLUTE_ERROR {
        bail!("WebGPU maximum absolute error {maximum:.6} exceeds {MAX_ABSOLUTE_ERROR}");
    }
    Ok(maximum)
}

pub(crate) fn cached_failure(model_path: &Path) -> Result<Option<String>> {
    if !runtime_root()?.join("webgpu.json").is_file() {
        return Ok(None);
    }
    let directory = model_path
        .parent()
        .context("model path has no parent")?
        .join(".webgpu-validation");
    let key = device_key(model_path)?;
    Ok(match read(&directory, &key)? {
        Some(ValidationResult::Failed { reason }) => Some(reason),
        _ => None,
    })
}

pub(crate) fn verify_or_cpu(
    gpu: crate::separator::model::LoadedModel,
    samples: &[f32],
    channels: usize,
) -> Result<crate::separator::model::LoadedModel> {
    use crate::config::ExecutionProviderPreference;
    use crate::separator::model;

    let directory = gpu
        .model_path
        .parent()
        .context("model path has no parent")?
        .join(".webgpu-validation");
    let key = device_key(&gpu.model_path);
    let prior = match &key {
        Ok(key) => read(&directory, key),
        Err(_) => Ok(None),
    };
    if let (Ok(_), Ok(Some(ValidationResult::Passed { .. }))) = (&key, &prior) {
        return Ok(gpu);
    }
    let mut cpu = model::load_with_ep(&gpu.model_path, ExecutionProviderPreference::Cpu)?;
    if !samples.iter().any(|sample| sample.abs() > 1e-5) {
        cpu.provider_fallback_reason = Some("WebGPU validation requires audible audio".to_owned());
        return Ok(cpu);
    }
    let error = match (&key, prior) {
        (Err(error), _) => Some(format!("Unable to identify the WebGPU adapter: {error:#}")),
        (_, Err(error)) => Some(format!(
            "Unable to read the WebGPU validation record: {error:#}"
        )),
        (_, Ok(Some(ValidationResult::Failed { reason }))) => Some(reason),
        (Ok(key), Ok(None)) => {
            let result = match crate::separator::spectral_session::compare_webgpu_window(
                &cpu, &gpu, samples, channels,
            ) {
                Ok(max_absolute_error) => {
                    tracing::info!(
                        "WebGPU validation passed for {} (driver {}): maximum absolute error {max_absolute_error}",
                        key.adapter_name,
                        key.driver_version
                    );
                    ValidationResult::Passed { max_absolute_error }
                }
                Err(error) => ValidationResult::Failed {
                    reason: format!(
                        "WebGPU validation failed for {} (driver {}): {error:#}",
                        key.adapter_name, key.driver_version
                    ),
                },
            };
            match write(&directory, key, &result) {
                Err(error) => Some(format!(
                    "Unable to save the WebGPU validation result: {error:#}"
                )),
                Ok(()) => match result {
                    ValidationResult::Passed { .. } => return Ok(gpu),
                    ValidationResult::Failed { reason } => Some(reason),
                },
            }
        }
        (_, Ok(Some(ValidationResult::Passed { .. }))) => unreachable!(),
    };
    cpu.provider_fallback_reason = error;
    Ok(cpu)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> DeviceKey {
        DeviceKey {
            adapter_name: "GPU".into(),
            driver_version: "1".into(),
            runtime_version: "1.31.1".into(),
            runtime_digest: "runtime".into(),
            plugin_digest: "plugin".into(),
            model_key: "model".into(),
        }
    }

    #[test]
    fn passing_comparison_is_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let result = ValidationResult::Passed {
            max_absolute_error: compare(&[0.1, 0.2], &[0.1001, 0.2]).unwrap(),
        };
        write(dir.path(), &key(), &result).unwrap();
        assert_eq!(read(dir.path(), &key()).unwrap(), Some(result));
    }

    #[test]
    fn failed_comparison_and_nonfinite_outputs_are_rejected() {
        assert!(compare(&[0.0], &[0.01]).is_err());
        assert!(compare(&[0.0], &[f32::NAN]).is_err());
        assert!(compare(&[0.0], &[f32::INFINITY]).is_err());
        let dir = tempfile::tempdir().unwrap();
        let result = ValidationResult::Failed {
            reason: "mismatch".into(),
        };
        write(dir.path(), &key(), &result).unwrap();
        assert_eq!(read(dir.path(), &key()).unwrap(), Some(result));
    }

    #[test]
    fn driver_or_runtime_change_requires_revalidation() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            &key(),
            &ValidationResult::Passed {
                max_absolute_error: 0.0,
            },
        )
        .unwrap();
        let mut changed = key();
        changed.driver_version = "2".into();
        assert_eq!(read(dir.path(), &changed).unwrap(), None);
        changed = key();
        changed.runtime_version = "1.32.0".into();
        assert_eq!(read(dir.path(), &changed).unwrap(), None);
    }
}
