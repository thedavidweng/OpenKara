use super::types::*;
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

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
pub(crate) fn choose_package_slot(config: &HostConfig, release: &mut Release) {
    if package_path(config, release).exists()
        && verify_seal(&package_path(config, release)).is_err()
    {
        release.installed_package = Some(format!(
            "{}-{}",
            release.archive.sha256,
            uuid::Uuid::new_v4()
        ));
    }
}
/// Extracts a gzip-compressed tar archive of regular files and directories.
/// The registry format is tar-only; the guards below (dup/path caps, 4 GiB
/// expansion bound, create_new, Unix mode) are the 629-side protections,
/// kept as they were on the source head.
pub(crate) fn extract_tar_gz(
    archive: &Path,
    output: &Path,
    cancel: &Cancellation,
) -> HostResult<()> {
    let mut seen = BTreeSet::new();
    let mut expanded = 0u64;
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
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(archive)?));
    for entry in tar.entries().map_err(|_| invalid())? {
        let mut entry = entry.map_err(|_| invalid())?;
        let kind = entry.header().entry_type();
        let name = entry
            .path()
            .map_err(|_| invalid())?
            .to_str()
            .ok_or_else(invalid)?
            .to_string();
        if !kind.is_file() && !kind.is_dir() {
            return Err(invalid());
        }
        let size = entry.size();
        let mode = entry.header().mode().map_err(|_| invalid())?;
        member(&name, size, kind.is_dir(), &mut entry, mode)?;
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
