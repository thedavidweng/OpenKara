use crate::separator::{artifacts, catalog::CatalogPlugin};
use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

const PLUGIN_DIR: &str = "webgpu";
const PLUGIN_RECORD: &str = "webgpu.json";

fn library_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "onnxruntime_providers_webgpu.dll"
    } else {
        "libonnxruntime_providers_webgpu.dylib"
    }
}

fn verify_plugin(dir: &Path, plugin: &CatalogPlugin) -> Result<PathBuf> {
    let mut library = None;
    let mut paths = Vec::new();
    for path in plugin.extracted_file_digests.keys() {
        let path = Path::new(path);
        if !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        {
            bail!("invalid plugin file path");
        }
        if path.file_name().is_some_and(|name| name == library_name()) {
            if library.replace(dir.join(path)).is_some() {
                bail!("multiple WebGPU libraries in plugin");
            }
        }
        paths.push(path.to_path_buf());
    }
    let library = library.context("WebGPU library missing from plugin")?;
    artifacts::verify_extracted_files(dir, &plugin.extracted_file_digests, &paths)?;
    Ok(library)
}

pub fn install(
    runtime_dir: &Path,
    plugin: &CatalogPlugin,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let dest = runtime_dir.join(PLUGIN_DIR);
    if dest.is_dir() && verify_plugin(&dest, plugin).is_ok() {
        fs::write(runtime_dir.join(PLUGIN_RECORD), serde_json::to_vec(plugin)?)?;
        return Ok(());
    }
    let archive = artifacts::download_verified_to_temp(
        &plugin.download_url,
        plugin.byte_size,
        &plugin.archive_digest,
        runtime_dir,
        progress,
    )?;
    let staging = artifacts::unique_temp_path(runtime_dir, "webgpu-staging");
    let result = (|| -> Result<()> {
        let extracted = artifacts::extract_archive_preserving_root(
            &archive,
            artifacts::archive_kind_for_filename(&plugin.filename)?,
            &staging,
        )?;
        artifacts::verify_extracted_files(&staging, &plugin.extracted_file_digests, &extracted)?;
        verify_plugin(&staging, plugin)?;
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }
        fs::rename(&staging, &dest)?;
        fs::write(runtime_dir.join(PLUGIN_RECORD), serde_json::to_vec(plugin)?)?;
        Ok(())
    })();
    let _ = fs::remove_file(archive);
    if result.is_err() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

pub fn register(environment: &ort::environment::Environment, runtime_path: &Path) -> Result<()> {
    let Some(runtime_dir) = runtime_path.ancestors().find(|dir| {
        dir.join(crate::separator::runtime_bootstrap::RUNTIME_RECORD_FILENAME)
            .is_file()
    }) else {
        return Ok(());
    };
    if !runtime_dir.join(PLUGIN_RECORD).is_file() {
        return Ok(());
    }
    let plugin: CatalogPlugin =
        serde_json::from_slice(&fs::read(runtime_dir.join(PLUGIN_RECORD))?)?;
    if plugin.provider != "webgpu"
        || plugin.target_triple != crate::separator::catalog::current_target_triple()
    {
        bail!("WebGPU plugin does not match this platform");
    }
    let library = verify_plugin(&runtime_dir.join(PLUGIN_DIR), &plugin)?;
    environment.register_ep_library("WebGPU", library)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::separator::catalog::CatalogFileDigest;
    use std::collections::BTreeMap;

    #[test]
    fn verifies_plugin_bytes_and_rejects_escaping_paths() {
        let dir = tempfile::tempdir().unwrap();
        let relative = format!("onnxruntime_ep_webgpu/{}", library_name());
        let file = dir.path().join(&relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"plugin").unwrap();
        let mut plugin = CatalogPlugin {
            artifact_id: "webgpu".into(),
            target_triple: crate::separator::catalog::current_target_triple().into(),
            provider: "webgpu".into(),
            version: "0.5.0".into(),
            filename: "plugin.whl".into(),
            byte_size: 1,
            archive_digest: "0".repeat(64),
            download_url: "https://example.org/plugin.whl".into(),
            extracted_file_digests: BTreeMap::from([(
                relative,
                CatalogFileDigest {
                    size: 6,
                    sha256: artifacts::sha256_file(&file).unwrap(),
                },
            )]),
        };
        assert!(verify_plugin(dir.path(), &plugin).is_ok());
        fs::write(&file, b"tamper").unwrap();
        assert!(verify_plugin(dir.path(), &plugin).is_err());
        plugin.extracted_file_digests.insert(
            "../outside".into(),
            CatalogFileDigest {
                size: 0,
                sha256: "0".repeat(64),
            },
        );
        assert!(verify_plugin(dir.path(), &plugin).is_err());
    }
}
