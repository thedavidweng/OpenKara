from pathlib import Path


output = Path("src-tauri/src/audio/output.rs")
text = output.read_text()
old = """                let resampler = Async::<f32>::new_sinc(
                    src_rate as f64 / dst_rate as f64,
"""
new = """                let resampler = Async::<f32>::new_sinc(
                    resample_ratio(src_rate, dst_rate),
"""
if text.count(old) != 1:
    raise SystemExit("output.rs: resampler construction drifted")
text = text.replace(old, new, 1)
marker = """struct ResamplerEntry {
    resampler: Async<f32>,
"""
helper = """fn resample_ratio(src_rate: u32, dst_rate: u32) -> f64 {
    dst_rate as f64 / src_rate as f64
}

struct ResamplerEntry {
    resampler: Async<f32>,
"""
if text.count(marker) != 1:
    raise SystemExit("output.rs: ResamplerEntry marker drifted")
text = text.replace(marker, helper, 1)
test_marker = """mod tests {
    use super::{forward_rendered_audio_to_airplay, render_output_buffer, write_output_samples};
"""
test_insert = """mod tests {
    use super::{
        forward_rendered_audio_to_airplay, render_output_buffer, resample_ratio,
        write_output_samples,
    };

    #[test]
    fn resample_ratio_is_output_rate_over_input_rate() {
        let upsample = resample_ratio(44_100, 48_000);
        let downsample = resample_ratio(48_000, 44_100);
        assert!((upsample - 48_000.0 / 44_100.0).abs() < f64::EPSILON);
        assert!((downsample - 44_100.0 / 48_000.0).abs() < f64::EPSILON);
        assert!(upsample > 1.0);
        assert!(downsample < 1.0);
    }
"""
if text.count(test_marker) != 1:
    raise SystemExit("output.rs: test module marker drifted")
output.write_text(text.replace(test_marker, test_insert, 1))

runtime = Path("src-tauri/src/separator/runtime_bootstrap.rs")
text = runtime.read_text()
old = """/// Development fallback: the staged runtime used during development builds.
pub fn development_runtime_path() -> PathBuf {
    PathBuf::from(env!(\"CARGO_MANIFEST_DIR\"))
        .join(\"generated\")
        .join(\"onnxruntime\")
        .join(ORT_RUNTIME_FILENAME)
}

"""
if text.count(old) != 1:
    raise SystemExit("runtime_bootstrap.rs: development fallback function drifted")
text = text.replace(old, "", 1)
old = """    let dev = development_runtime_path();
    if dev.is_file() {
        return if verify_runtime_install(&dev)? {
            Ok(RuntimeResolution::Ready(dev))
        } else {
            Ok(RuntimeResolution::Corrupt(dev))
        };
    }

"""
if text.count(old) != 1:
    raise SystemExit("runtime_bootstrap.rs: development fallback resolution drifted")
runtime.write_text(text.replace(old, "", 1))

model = Path("src-tauri/src/separator/model.rs")
text = model.read_text()
text = text.replace('pub const ORT_RUNTIME_STAGING_DIR: &str = "generated/onnxruntime";\n', '')
start = text.index("pub fn default_runtime_library_path() -> PathBuf {")
end = text.index("pub fn ensure_runtime_loaded_from_path", start)
text = text[:start] + text[end:]
old = """fn load_with_ep(path: &Path, ep_preference: ExecutionProviderPreference) -> Result<LoadedModel> {
    ensure_runtime_loaded(None)?;
    let runtime_metadata = read_model_runtime_metadata(path)?;
"""
new = """fn load_with_ep(path: &Path, ep_preference: ExecutionProviderPreference) -> Result<LoadedModel> {
    anyhow::ensure!(
        ORT_RUNTIME_PATH.get().is_some(),
        \"ONNX Runtime is not initialized; the managed runtime bootstrap must complete before model loading\"
    );
    let runtime_metadata = read_model_runtime_metadata(path)?;
"""
if text.count(old) != 1:
    raise SystemExit("model.rs: load_with_ep precondition drifted")
model.write_text(text.replace(old, new, 1))

commands = Path("src-tauri/src/commands/runtime_bootstrap.rs")
text = commands.read_text()
start = text.index("pub fn ensure_runtime_ready_or_install_blocking(")
end = text.index("\n#[tauri::command]\npub fn download_runtime", start)
replacement = '''pub fn ensure_runtime_ready_or_install_blocking(
    app_data_dir: &Path,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> CommandResult<PathBuf> {
    let snapshot = get_runtime_bootstrap_status_from_state(status)?;
    if snapshot.state == RuntimeBootstrapState::Downloading {
        return Err(model_bootstrap_error(format!(
            "ONNX Runtime is still downloading to {}",
            snapshot.runtime_path
        )));
    }

    match runtime_bootstrap::resolve_runtime_installation(app_data_dir)
        .map_err(|error| model_bootstrap_error(error.to_string()))?
    {
        runtime_bootstrap::RuntimeResolution::Ready(path) => {
            crate::separator::model::ensure_runtime_loaded_from_path(&path)
                .map_err(|error| model_bootstrap_error(error.to_string()))?;
            let ready = ready_snapshot(&path);
            store_snapshot(status, ready.clone());
            emit(RUNTIME_BOOTSTRAP_READY_EVENT, ready);
            Ok(path)
        }
        runtime_bootstrap::RuntimeResolution::Corrupt(_) => {
            let _ = runtime_bootstrap::delete_runtime(app_data_dir);
            install_and_load_runtime_blocking(app_data_dir, status, emit).map_err(|error| {
                let command_error = model_bootstrap_error(error.to_string());
                let failed = failed_snapshot(
                    &runtime_bootstrap::managed_runtime_path(app_data_dir),
                    command_error.clone(),
                );
                store_snapshot(status, failed.clone());
                emit(RUNTIME_BOOTSTRAP_ERROR_EVENT, failed);
                command_error
            })
        }
        runtime_bootstrap::RuntimeResolution::Absent => {
            install_and_load_runtime_blocking(app_data_dir, status, emit).map_err(|error| {
                let command_error = model_bootstrap_error(error.to_string());
                let failed = failed_snapshot(
                    &runtime_bootstrap::managed_runtime_path(app_data_dir),
                    command_error.clone(),
                );
                store_snapshot(status, failed.clone());
                emit(RUNTIME_BOOTSTRAP_ERROR_EVENT, failed);
                command_error
            })
        }
    }
}
'''
commands.write_text(text[:start] + replacement + text[end:])

app_runtime = Path("src-tauri/src/app_runtime.rs")
text = app_runtime.read_text()
start = text.index("    // The runtime may come from:")
end = text.index("\n    let app_config = config::load_config", start)
replacement = '''    // ONNX Runtime has one installation authority: the verified managed
    // app-data location. Development and packaged builds use the same path.
    let runtime_status_snapshot =
        separator::runtime_bootstrap::runtime_status_snapshot(&app_data_dir);
    let runtime_bootstrap_status = Arc::new(Mutex::new(
        commands::runtime_bootstrap::RuntimeBootstrapStatusSnapshot::from(
            runtime_status_snapshot.clone(),
        ),
    ));

    if runtime_status_snapshot.status == separator::runtime_bootstrap::RuntimeStatus::Ready {
        match separator::runtime_bootstrap::ensure_runtime_verified(&app_data_dir) {
            Ok(path) => {
                if let Err(err) = separator::model::ensure_runtime_loaded_from_path(&path) {
                    eprintln!(
                        "warning: failed to load managed ONNX Runtime from {}: {err:#}",
                        path.display()
                    );
                }
            }
            Err(err) => {
                eprintln!("warning: managed ONNX Runtime verification failed: {err:#}");
            }
        }
    }
'''
app_runtime.write_text(text[:start] + replacement + text[end:])

phase3_model = Path("src-tauri/tests/phase3_model.rs")
text = phase3_model.read_text()
text = text.replace(
    "use openkara_lib::{config::ExecutionProviderPreference, separator::model};",
    "use openkara_lib::{\n    config::ExecutionProviderPreference,\n    separator::{model, runtime_bootstrap},\n};",
)
start = text.index("#[test]\nfn resolves_staged_runtime_library_path()")
end = text.index("#[test]\nfn loads_embedded_demucs_model_session()", start)
replacement = '''#[test]
fn managed_runtime_path_is_the_only_installation_location() {
    let app_data_dir = support::unique_temp_path("phase3-managed-runtime");
    let runtime_path = runtime_bootstrap::managed_runtime_path(&app_data_dir);

    assert_eq!(
        runtime_path,
        app_data_dir
            .join("runtime")
            .join(model::ORT_RUNTIME_FILENAME)
    );
}

fn initialize_test_runtime() {
    let runtime_path = repo_root()
        .join("generated")
        .join("onnxruntime")
        .join(model::ORT_RUNTIME_FILENAME);
    model::ensure_runtime_loaded_from_path(&runtime_path)
        .expect("CI-prepared runtime should initialize explicitly for model tests");
}

'''
text = text[:start] + replacement + text[end:]
text = text.replace(
    "fn loads_embedded_demucs_model_session() {\n    let loaded = model::load_from_path(",
    "fn loads_embedded_demucs_model_session() {\n    initialize_test_runtime();\n    let loaded = model::load_from_path(",
    1,
)
text = text.replace(
    "fn fails_with_clear_error_for_missing_model_file() {\n    let missing_path",
    "fn fails_with_clear_error_for_missing_model_file() {\n    initialize_test_runtime();\n    let missing_path",
    1,
)
missing_start = text.index("#[test]\nfn fails_with_clear_error_for_missing_runtime_library()")
missing_end = text.index("#[test]\nfn describes_cpu_only_provider_path()", missing_start)
text = text[:missing_start] + text[missing_end:]
text = text.replace(
    "fn loads_embedded_demucs_model_with_xnnpack_preference() {\n    let loaded = model::load_from_path(",
    "fn loads_embedded_demucs_model_with_xnnpack_preference() {\n    initialize_test_runtime();\n    let loaded = model::load_from_path(",
    1,
)
phase3_model.write_text(text)

for p in Path("src-tauri").rglob("*.rs"):
    if p == model:
        continue
    t = p.read_text()
    for retired in (
        "resolve_runtime_library_path_for_tests",
        "resolve_runtime_library_path(",
        "default_runtime_library_path",
        "ORT_RUNTIME_STAGING_DIR",
    ):
        if retired in t:
            raise SystemExit(f"retired runtime resolver {retired!r} still referenced by {p}")

Path(".github/workflows/agent-runtime-playback-fix.yml").unlink()
Path("scripts/agent-runtime-playback-fix.py").unlink()
