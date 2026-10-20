use crate::separator::artifacts;
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::{
    fs,
    io::{self, Read},
    path::Path,
    process::Command,
};

const VERSION: &str = "24.21.0";
const MAX_RUNTIME_BYTES: u64 = 256 * 1024 * 1024;

pub fn maybe_run_from_cli() -> Result<bool> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--automation-streaming-runtime")) {
        return Ok(false);
    }
    let phase = args.next().context("expected install or restart")?;
    let root = args.next().context("expected an isolated app-data directory")?;
    if args.next().is_some() {
        bail!("unexpected streaming runtime smoke argument");
    }
    let phase = phase.to_str().context("phase is not UTF-8")?;
    if phase != "install" && phase != "restart" {
        bail!("expected install or restart");
    }
    run(phase, Path::new(&root))?;
    Ok(true)
}

fn run(phase: &str, root: &Path) -> Result<()> {
    let target = crate::separator::catalog::current_target_triple();
    let pin: serde_json::Value = serde_json::from_str(include_str!("../../sidecars/netease/runtime.json"))?;
    let target_pin = pin["targets"][target]
        .as_array()
        .context("unsupported runtime smoke target")?;
    let suffix = target_pin[0].as_str().context("missing archive suffix")?;
    let digest = target_pin[1].as_str().context("missing archive digest")?;
    let size = match suffix {
        "darwin-arm64.tar.gz" => 52_909_993,
        "darwin-x64.tar.gz" => 54_203_979,
        "linux-x64.tar.gz" => 58_088_022,
        "linux-arm64.tar.gz" => 57_824_078,
        "win-x64.zip" => 37_618_919,
        _ => bail!("unsupported archive"),
    };
    let archive_name = format!("node-v{VERSION}-{suffix}");
    let url = format!("https://nodejs.org/dist/v{VERSION}/{archive_name}");
    let installed = root.join(format!("node-{VERSION}-{target}"));
    let executable = installed.join(if cfg!(windows) { "node.exe" } else { "node" });
    let identity_path = installed.join("sha256");
    fs::create_dir_all(root)?;
    if phase == "install" {
        if installed.exists() {
            bail!("install requires a fresh directory; use restart for an existing runtime");
        }
        let staging = root.join("streaming-runtime-staging");
        fs::create_dir(&staging).context("staging directory already exists or cannot be created")?;
        let result = (|| -> Result<()> {
            let archive = artifacts::download_verified_to_temp(&url, size, digest, &staging, |_, _| {})?;
            let candidate = staging.join(if cfg!(windows) { "node.exe" } else { "node" });
            extract_runtime(&archive, &archive_name, &candidate)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&candidate, fs::Permissions::from_mode(0o755))?;
            }
            fs::write(staging.join("sha256"), artifacts::sha256_file(&candidate)?)?;
            fs::remove_file(archive)?;
            fs::rename(&staging, &installed)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result?;
    }
    let runtime_sha256 = artifacts::sha256_file(&executable)?;
    if fs::read_to_string(&identity_path)? != runtime_sha256 {
        bail!("installed runtime identity changed");
    }
    let app = std::env::current_exe()?;
    let mut command = Command::new(fs::canonicalize(&executable)?);
    command
        .args(["-p", "JSON.stringify({version:process.version,platform:process.platform,arch:process.arch,parent_pid:process.ppid})"])
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .env_remove("NODE_EXTRA_CA_CERTS")
        .env_remove("NODE_REPL_EXTERNAL_MODULE")
        .current_dir(&installed);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().context("downloaded runtime could not start")?;
    if !output.status.success() {
        bail!("downloaded runtime exited with {}", output.status);
    }
    let runtime: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    if runtime["version"] != format!("v{VERSION}")
        || runtime["parent_pid"] != std::process::id()
    {
        bail!("runtime did not report the expected version and app parent");
    }
    let report = json!({
        "phase": phase,
        "app_executable": app,
        "app_sha256": artifacts::sha256_file(&app)?,
        "app_pid": std::process::id(),
        "target": target,
        "archive_url": url,
        "archive_size": size,
        "archive_sha256": digest,
        "runtime_executable": fs::canonicalize(executable)?,
        "runtime_sha256": runtime_sha256,
        "runtime": runtime,
        "flatpak_id": std::env::var("FLATPAK_ID").ok(),
    });
    fs::write(root.join(format!("streaming-runtime-{phase}.json")), serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

fn copy_runtime(mut reader: impl Read, destination: &Path, declared: u64) -> Result<()> {
    if declared == 0 || declared > MAX_RUNTIME_BYTES {
        bail!("runtime exceeds size limit");
    }
    let mut output = fs::OpenOptions::new().write(true).create_new(true).open(destination)?;
    if io::copy(&mut reader.by_ref().take(MAX_RUNTIME_BYTES + 1), &mut output)? != declared {
        bail!("runtime expanded size differs from its declaration");
    }
    Ok(())
}

fn extract_runtime(archive: &Path, archive_name: &str, destination: &Path) -> Result<()> {
    let stem = archive_name.trim_end_matches(".tar.gz").trim_end_matches(".zip");
    let member = format!("{stem}/{}", if cfg!(windows) { "node.exe" } else { "bin/node" });
    if archive_name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
        let mut found = false;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            if entry.name() == member {
                if found || !entry.is_file() || entry.is_symlink() {
                    bail!("runtime archive has an invalid executable member");
                }
                found = true;
                let size = entry.size();
                copy_runtime(&mut entry, destination, size)?;
            }
        }
        if !found {
            bail!("runtime archive has no executable");
        }
    } else {
        let decoder = flate2::read::GzDecoder::new(fs::File::open(archive)?);
        let mut tar = tar::Archive::new(decoder);
        let mut found = false;
        for entry in tar.entries()? {
            let mut entry = entry?;
            if entry.path()?.as_ref() == Path::new(&member) {
                if found || !entry.header().entry_type().is_file() {
                    bail!("runtime archive has an invalid executable member");
                }
                found = true;
                let size = entry.size();
                copy_runtime(&mut entry, destination, size)?;
            }
        }
        if !found {
            bail!("runtime archive has no executable");
        }
    }
    Ok(())
}
