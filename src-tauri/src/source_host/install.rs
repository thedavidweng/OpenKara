use super::{network, types::*};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const DOWNLOAD_MISMATCH: &str =
    "Downloaded source bytes do not match the signed release. Retry or report the release.";
const BUNDLED_RUNTIME_INCOMPATIBLE: &str =
    "The bundled runtime cannot run this release. Update OpenKara or select a compatible source version.";
const RUNTIME_VALIDATION_TIMEOUT: &str =
    "Runtime validation timed out. Update OpenKara or reinstall the runtime.";
const RUNTIME_EXECUTION_FAILED: &str =
    "The pinned runtime cannot execute on this platform. Update OpenKara or reinstall the runtime.";
const RUNTIME_IDENTITY_MISMATCH: &str =
    "The runtime version or platform does not match the signed release. Update OpenKara or reinstall the runtime.";

pub(crate) fn relative(path: &str) -> HostResult<PathBuf> {
    if path.is_empty() || path.contains(['\\', ':', '\0']) {
        return Err(invalid());
    }
    let path = Path::new(path);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
        || path.is_absolute()
    {
        return Err(invalid());
    }
    let clean: PathBuf = path
        .components()
        .filter(|part| !matches!(part, Component::CurDir))
        .collect();
    if clean.as_os_str().is_empty() {
        return Err(invalid());
    }
    Ok(clean)
}
fn invalid() -> HostError {
    HostError::new("invalid_package","Source archive contains an unsafe path, file, or package structure. Obtain a corrected release.")
}
pub(crate) fn package_path(config: &HostConfig, release: &Release) -> PathBuf {
    config.data_root.join("packages").join(
        release
            .installed_package
            .as_deref()
            .unwrap_or(&release.archive.sha256),
    )
}
pub(crate) fn runtime_path(config: &HostConfig, release: &Release) -> HostResult<PathBuf> {
    let artifact = release
        .runtime
        .targets
        .get(&config.target)
        .ok_or_else(invalid)?;
    Ok(config.data_root.join("runtimes").join(
        release
            .installed_runtime
            .as_deref()
            .unwrap_or(&artifact.sha256),
    ))
}
pub(crate) fn verify_installed(config: &HostConfig, release: &Release) -> HostResult<()> {
    let package = package_path(config, release);
    verify_seal(&package)?;
    validate_package(&package, release)?;
    match &config.runtime_policy {
        RuntimePolicy::Downloaded => {
            let root = runtime_path(config, release)?;
            verify_seal(&root)?;
            let artifact = release
                .runtime
                .targets
                .get(&config.target)
                .ok_or_else(invalid)?;
            if !root.join(relative(&artifact.executable)?).is_file() {
                return Err(invalid());
            }
        }
        RuntimePolicy::ChannelBundled { executable, .. } => {
            if !executable.is_file() {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
pub(crate) fn choose_repair_slots(config: &HostConfig, release: &mut Release) -> HostResult<()> {
    if package_path(config, release).exists()
        && verify_seal(&package_path(config, release)).is_err()
    {
        release.installed_package = Some(format!(
            "{}-{}",
            release.archive.sha256,
            uuid::Uuid::new_v4()
        ));
    }
    if matches!(config.runtime_policy, RuntimePolicy::Downloaded) {
        let root = runtime_path(config, release)?;
        if root.exists() && verify_seal(&root).is_err() {
            let artifact = release
                .runtime
                .targets
                .get(&config.target)
                .ok_or_else(invalid)?;
            release.installed_runtime =
                Some(format!("{}-{}", artifact.sha256, uuid::Uuid::new_v4()));
        }
    }
    Ok(())
}
pub(crate) fn validate_artifact(artifact: &Artifact) -> HostResult<()> {
    if artifact.size == 0
        || artifact.size > 2 * 1024 * 1024 * 1024
        || artifact.sha256.len() != 64
        || !artifact
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || !["tar", "tar.gz", "zip"].contains(&artifact.format.as_str())
    {
        return Err(invalid());
    }
    Ok(())
}
pub(crate) async fn artifact(
    config: &HostConfig,
    artifact: &Artifact,
    cancel: &Cancellation,
    progress: impl FnMut(u64),
) -> HostResult<PathBuf> {
    validate_artifact(artifact)?;
    let staging = config.data_root.join("staging");
    fs::create_dir_all(&staging)?;
    let directory = staging.join(uuid::Uuid::new_v4().to_string());
    fs::create_dir(&directory)?;
    let archive = directory.join("archive");
    let result = async {
        let response = network::response(
            &artifact.url,
            &Default::default(),
            None,
            super::test_http(config),
            cancel,
        )
        .await?;
        let (size, sha256) =
            network::download(response, &archive, artifact.size, cancel, progress).await?;
        if size != artifact.size || sha256 != artifact.sha256 {
            return Err(HostError::new("invalid_package", DOWNLOAD_MISMATCH));
        }
        let output = directory.join("files");
        fs::create_dir(&output)?;
        extract(
            &archive,
            &output,
            &artifact.format,
            cancel,
            if artifact.executable.is_empty() {
                None
            } else {
                Some(artifact.executable.as_str())
            },
        )?;
        fs::remove_file(&archive)?;
        Ok(output)
    }
    .await;
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result
}
pub(crate) fn extract(
    archive: &Path,
    output: &Path,
    format: &str,
    cancel: &Cancellation,
    runtime_entry: Option<&str>,
) -> HostResult<()> {
    let mut seen = BTreeSet::new();
    let mut expanded = 0u64;
    // Archive entry names always use `/`, while `Path` renders with `\` on
    // Windows. Compare components through `Path` so the pinned runtime entry
    // matches on every platform instead of relying on the host separator.
    let selected = |path: &Path| {
        runtime_entry.is_none_or(|entry| {
            path == Path::new(entry)
                || path.file_name().is_some_and(|name| {
                    matches!(name.to_str(), Some("LICENSE" | "NOTICE" | "LICENSE.txt"))
                })
        })
    };
    let mut member =
        |name: &str, size: u64, is_dir: bool, reader: &mut dyn Read, mode: u32| -> HostResult<()> {
            cancel.check()?;
            let path = relative(name)?;
            if !seen.insert(path.clone()) || seen.len() > 100_000 {
                return Err(invalid());
            }
            expanded = expanded.checked_add(size).ok_or_else(invalid)?;
            if expanded > 4 * 1024 * 1024 * 1024 {
                return Err(invalid());
            }
            let destination = output.join(&path);
            if is_dir {
                fs::create_dir_all(destination)?;
                return Ok(());
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)?;
            let mut remaining = size;
            let mut buffer = [0u8; 64 * 1024];
            while remaining > 0 {
                cancel.check()?;
                let n = reader.read(&mut buffer[..(remaining as usize).min(64 * 1024)])?;
                if n == 0 {
                    return Err(invalid());
                }
                file.write_all(&buffer[..n])?;
                remaining -= n as u64;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    &destination,
                    fs::Permissions::from_mode(if mode & 0o111 != 0 { 0o755 } else { 0o644 }),
                )?;
            }
            Ok(())
        };
    if format == "zip" {
        let mut zip = zip::ZipArchive::new(fs::File::open(archive)?).map_err(|_| invalid())?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(|_| invalid())?;
            let mode = entry.unix_mode().unwrap_or(0o100644);
            let kind = mode & 0o170000;
            let name = entry.name().map_err(|_| invalid())?.into_owned();
            let path = relative(&name)?;
            if !selected(&path) {
                continue;
            }
            if kind != 0 && kind != 0o100000 && kind != 0o040000 {
                return Err(invalid());
            }
            let size = entry.size();
            let dir = entry.is_dir();
            member(&name, size, dir, &mut entry, mode)?;
        }
    } else {
        let file = fs::File::open(archive)?;
        let reader: Box<dyn Read> = if format == "tar.gz" {
            Box::new(flate2::read::GzDecoder::new(file))
        } else {
            Box::new(file)
        };
        let mut tar = tar::Archive::new(reader);
        for entry in tar.entries().map_err(|_| invalid())? {
            let mut entry = entry.map_err(|_| invalid())?;
            let kind = entry.header().entry_type();
            let name = entry
                .path()
                .map_err(|_| invalid())?
                .to_str()
                .ok_or_else(invalid)?
                .to_string();
            let path = relative(&name)?;
            if !selected(&path) {
                continue;
            }
            if !kind.is_file() && !kind.is_dir() {
                return Err(invalid());
            }
            let size = entry.size();
            let mode = entry.header().mode().map_err(|_| invalid())?;
            member(&name, size, kind.is_dir(), &mut entry, mode)?;
        }
    }
    Ok(())
}
pub(crate) fn validate_package(directory: &Path, release: &Release) -> HostResult<()> {
    let manifest: SourceManifest =
        serde_json::from_slice(&fs::read(directory.join("manifest.json"))?)
            .map_err(|_| invalid())?;
    if manifest != release.manifest
        || !manifest.entry.ends_with(".mjs")
        || !directory.join(relative(&manifest.entry)?).is_file()
        || !directory.join("LICENSE").is_file()
        || !directory.join("NOTICE").is_file()
        || !directory.join("package-lock.json").is_file()
    {
        return Err(invalid());
    }
    let mut directories = vec![directory.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let metadata = entry.file_type()?;
            if metadata.is_symlink() {
                return Err(invalid());
            }
            if metadata.is_dir() {
                directories.push(entry.path());
            }
            if entry.file_name() == "package.json" {
                let package: serde_json::Value =
                    serde_json::from_slice(&fs::read(entry.path())?).map_err(|_| invalid())?;
                if ["preinstall", "install", "postinstall"]
                    .iter()
                    .any(|key| package["scripts"].get(key).is_some())
                {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(())
}
fn file_digest(path: &Path) -> HostResult<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(super::digest_hex(hash.finalize()))
}
fn inventory(directory: &Path) -> HostResult<std::collections::BTreeMap<String, String>> {
    let mut records = std::collections::BTreeMap::new();
    let mut dirs = vec![directory.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(invalid());
            }
            if kind.is_dir() {
                dirs.push(entry.path());
            } else if kind.is_file() {
                let path = entry.path();
                let relative = path
                    .strip_prefix(directory)
                    .map_err(|_| invalid())?
                    .to_str()
                    .ok_or_else(invalid)?;
                if relative != ".verified-files.json" {
                    records.insert(relative.into(), file_digest(&path)?);
                }
            } else {
                return Err(invalid());
            }
        }
    }
    Ok(records)
}
pub(crate) fn seal(directory: &Path) -> HostResult<()> {
    if directory.join(".verified-files.json").exists() {
        return Err(invalid());
    }
    let records = inventory(directory)?;
    fs::write(
        directory.join(".verified-files.json"),
        serde_json::to_vec(&records).map_err(|_| invalid())?,
    )?;
    Ok(())
}
pub(crate) fn verify_seal(directory: &Path) -> HostResult<()> {
    if fs::symlink_metadata(directory)?.file_type().is_symlink()
        || fs::symlink_metadata(directory.join(".verified-files.json"))?
            .file_type()
            .is_symlink()
    {
        return Err(invalid());
    }
    let records: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(directory.join(".verified-files.json"))?)
            .map_err(|_| invalid())?;
    if records != inventory(directory)? {
        return Err(invalid());
    }
    Ok(())
}
pub(crate) async fn runtime(
    config: &HostConfig,
    release: &Release,
    cancel: &Cancellation,
    progress: impl FnMut(u64),
) -> HostResult<PathBuf> {
    let executable = match &config.runtime_policy {
        RuntimePolicy::ChannelBundled {
            executable,
            version,
            target,
        } => {
            if version != &release.runtime.version || target != &config.target {
                return Err(HostError::new(
                    "incompatible_plugin",
                    BUNDLED_RUNTIME_INCOMPATIBLE,
                ));
            }
            executable.clone()
        }
        RuntimePolicy::Downloaded => {
            let artifact = release.runtime.targets.get(&config.target).ok_or_else(|| {
                HostError::new(
                    "incompatible_plugin",
                    "This source has no runtime for this platform.",
                )
            })?;
            let runtime = runtime_path(config, release)?;
            if !runtime.exists() {
                let staged = self::artifact(config, artifact, cancel, progress).await?;
                let result: HostResult<()> = (|| {
                    if !staged.join(relative(&artifact.executable)?).is_file() {
                        return Err(invalid());
                    }
                    seal(&staged)?;
                    fs::create_dir_all(runtime.parent().ok_or_else(invalid)?)?;
                    fs::rename(&staged, &runtime)?;
                    Ok(())
                })();
                let _ = fs::remove_dir_all(staged.parent().unwrap());
                result?;
            }
            verify_seal(&runtime)?;
            runtime.join(relative(&artifact.executable)?)
        }
    };
    cancel.check()?;
    let mut command = tokio::process::Command::new(&executable);
    command
        .arg("-p")
        .arg("JSON.stringify({version:process.versions.node,platform:process.platform,arch:process.arch})")
        .env_clear()
        .kill_on_drop(true);
    for key in ["SYSTEMROOT", "WINDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let output = tokio::select! {
        _ = cancel.cancelled() => {
            return Err(HostError::new("cancelled", "Runtime validation cancelled. Retry when ready."));
        }
        output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output()) => {
            output
                .map_err(|_| HostError::new("incompatible_plugin", RUNTIME_VALIDATION_TIMEOUT))?
                .map_err(|_| HostError::new("incompatible_plugin", RUNTIME_EXECUTION_FAILED))?
        }
    };
    let info: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|_| invalid())?;
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    if !output.status.success()
        || info["version"] != release.runtime.version
        || info["platform"] != platform
        || info["arch"] != arch
    {
        return Err(HostError::new(
            "incompatible_plugin",
            RUNTIME_IDENTITY_MISMATCH,
        ));
    }
    Ok(executable)
}
