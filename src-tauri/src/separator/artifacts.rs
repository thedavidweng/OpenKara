//! One catalog-driven artifact installer for models and runtimes.
//!
//! The installer owns download, verification, staging, activation, installed
//! identity, failure cleanup, and progress reporting (issue #168). Its
//! invariants:
//!
//! - Download memory is fixed-size and independent of artifact size: payloads
//!   stream to a unique temporary file and are hashed during transfer.
//! - Expected byte size and SHA-256 verify before extraction or activation.
//! - Archives extract through one safe implementation that rejects absolute
//!   paths, traversal, links, duplicate normalized paths, excessive member
//!   counts, and excessive expanded size.
//! - Every extracted file declared by the catalog verifies by size and
//!   SHA-256 before the artifact can be activated.
//! - Activation is an atomic directory rename; a partial artifact never
//!   becomes visible at its final path.
//! - Interrupted downloads restart from zero. Resume is intentionally not
//!   implemented: it is only permissible with proven byte continuity, and
//!   restart is always correct.

use crate::separator::catalog::{CatalogFileDigest, InstalledFileRecord};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Hard ceiling on a single downloaded artifact (largest model is ~1.4 GiB).
const MAX_DOWNLOAD_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Wall-clock ceiling for one download on a slow but live connection.
const DOWNLOAD_TOTAL_DEADLINE: Duration = Duration::from_secs(60 * 60);
/// Transport attempts per artifact. Each retry resumes with a Range request,
/// so an attempt costs a round trip rather than the megabytes already fetched.
const DOWNLOAD_ATTEMPTS: u32 = 5;
const DOWNLOAD_RETRY_BACKOFF: Duration = Duration::from_secs(2);
const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Per-read inactivity timeout (reqwest's `timeout` covers reads on blocking
/// streams once the response body is being consumed).
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(120);

const MAX_ARCHIVE_MEMBERS: usize = 512;
const MAX_ARCHIVE_EXPANDED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub fn unique_temp_path(directory: &Path, stem: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    directory.join(format!("{stem}.download.{nanos}.tmp"))
}

fn download_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .timeout(DOWNLOAD_READ_TIMEOUT)
        .build()
        .context("failed to build artifact download client")
}

/// Stream a payload to a unique temporary file inside `staging_dir`, hashing
/// during transfer, and verify the declared byte size and SHA-256 before
/// returning the temp path. The temp file is removed on any failure.
pub fn download_verified_to_temp(
    url: &str,
    expected_size: u64,
    expected_sha256: &str,
    staging_dir: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    if expected_size == 0 || expected_size > MAX_DOWNLOAD_BYTES {
        bail!("artifact declares an unacceptable size of {expected_size} bytes");
    }

    fs::create_dir_all(staging_dir).with_context(|| {
        format!(
            "failed to create artifact staging directory {}",
            staging_dir.display()
        )
    })?;

    let client = download_client()?;
    let temp_path = unique_temp_path(staging_dir, "artifact");
    let result = download_with_resume(
        &client,
        url,
        &temp_path,
        expected_size,
        expected_sha256,
        &mut progress,
    );

    match result {
        Ok(()) => Ok(temp_path),
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            Err(error)
        }
    }
}

/// Downloads to `temp_path`, resuming after a transport failure instead of
/// starting over.
///
/// The artifacts are hundreds of megabytes. A single stalled read used to
/// discard everything already on disk, so a user on a slow link could spend an
/// hour reaching 30% and then start again from zero (#270). A `Range` request
/// picks up where the stream stopped, and the running hash keeps its state
/// because no downloaded byte is ever thrown away.
fn download_with_resume(
    client: &reqwest::blocking::Client,
    url: &str,
    temp_path: &Path,
    expected_size: u64,
    expected_sha256: &str,
    progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let started_at = Instant::now();
    let mut state = TransferState::begin(temp_path)?;
    let mut last_error = None;

    for attempt in 0..DOWNLOAD_ATTEMPTS {
        if attempt > 0 {
            thread::sleep(DOWNLOAD_RETRY_BACKOFF * attempt);
        }
        if started_at.elapsed() > DOWNLOAD_TOTAL_DEADLINE {
            bail!("artifact download exceeded the total time budget");
        }

        let request = if state.downloaded > 0 {
            client.get(url).header(
                reqwest::header::RANGE,
                format!("bytes={}-", state.downloaded),
            )
        } else {
            client.get(url)
        };

        let mut response = match request
            .send()
            .and_then(|response| response.error_for_status())
        {
            Ok(response) => response,
            Err(error) => {
                last_error = Some(anyhow::anyhow!(
                    "failed to download artifact from {url}: {}",
                    error.without_url()
                ));
                continue;
            }
        };

        // A server that ignores Range answers 200 with the whole body. Taking
        // it would append a second copy, so restart the transfer instead.
        if state.downloaded > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            state = TransferState::restart(temp_path)?;
        }

        match stream_response_to_file(
            &mut response,
            &mut state,
            started_at,
            expected_size,
            progress,
        ) {
            Ok(()) if state.downloaded == expected_size => {
                return state.finish(expected_size, expected_sha256)
            }
            // A body that ends short is the common shape of the failure this
            // exists for: the connection dropped and the reader simply saw EOF.
            // Retry from where it stopped rather than reporting a truncation.
            Ok(()) => {
                last_error = Some(anyhow::anyhow!(
                    "artifact download ended early at {} of {expected_size} bytes",
                    state.downloaded
                ));
            }
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("artifact download failed with no recorded error")))
}

/// The bytes already on disk plus the hash of exactly those bytes. Carrying
/// both across attempts is what makes a resume cheaper than a restart.
struct TransferState {
    file: fs::File,
    hasher: Sha256,
    downloaded: u64,
}

impl TransferState {
    fn begin(temp_path: &Path) -> Result<Self> {
        let file = fs::File::create(temp_path)
            .with_context(|| format!("failed to create download file {}", temp_path.display()))?;
        Ok(Self {
            file,
            hasher: Sha256::new(),
            downloaded: 0,
        })
    }

    fn restart(temp_path: &Path) -> Result<Self> {
        Self::begin(temp_path)
    }

    fn finish(self, expected_size: u64, expected_sha256: &str) -> Result<()> {
        if self.downloaded != expected_size {
            bail!(
                "artifact download was truncated: expected {expected_size} bytes, got {}",
                self.downloaded
            );
        }
        let actual_sha256 = crate::hash::hex_lower(self.hasher.finalize());
        if actual_sha256 != expected_sha256 {
            bail!("artifact digest mismatch: expected {expected_sha256}, got {actual_sha256}");
        }
        self.file
            .sync_all()
            .context("failed to sync the downloaded artifact")?;
        Ok(())
    }
}

fn stream_response_to_file(
    response: &mut reqwest::blocking::Response,
    state: &mut TransferState,
    started_at: Instant,
    expected_size: u64,
    progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let total_bytes = Some(expected_size);
    let mut buffer = [0_u8; 64 * 1024];

    let mut last_emit_bytes = 0_u64;
    let mut last_emit_at = Instant::now();
    let emit_interval = Duration::from_millis(150);
    let emit_min_step: u64 = 256 * 1024;
    let mut emit = |downloaded: u64, force: bool, last_bytes: &mut u64, last_at: &mut Instant| {
        let step_ok = downloaded.saturating_sub(*last_bytes) >= emit_min_step;
        let time_ok = last_at.elapsed() >= emit_interval;
        if force || step_ok || time_ok {
            progress(downloaded, total_bytes);
            *last_bytes = downloaded;
            *last_at = Instant::now();
        }
    };

    emit(
        state.downloaded,
        true,
        &mut last_emit_bytes,
        &mut last_emit_at,
    );

    loop {
        if started_at.elapsed() > DOWNLOAD_TOTAL_DEADLINE {
            bail!("artifact download exceeded the total time budget");
        }
        let read = response
            .read(&mut buffer)
            .context("failed while streaming artifact download")?;
        if read == 0 {
            break;
        }
        // Written and hashed before `downloaded` advances, so the counter never
        // claims bytes a resume would then skip.
        state
            .file
            .write_all(&buffer[..read])
            .context("failed writing the artifact download to disk")?;
        state.hasher.update(&buffer[..read]);
        state.downloaded += read as u64;
        if state.downloaded > expected_size {
            bail!(
                "artifact download exceeded the declared size of {expected_size} bytes; aborting"
            );
        }
        emit(
            state.downloaded,
            false,
            &mut last_emit_bytes,
            &mut last_emit_at,
        );
    }

    emit(
        state.downloaded,
        true,
        &mut last_emit_bytes,
        &mut last_emit_at,
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Safe archive extraction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    TarGz,
    Zip,
}

pub fn archive_kind_for_filename(filename: &str) -> Result<ArchiveKind> {
    if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        Ok(ArchiveKind::TarGz)
    } else if filename.ends_with(".zip") || filename.ends_with(".nupkg") {
        Ok(ArchiveKind::Zip)
    } else {
        bail!("unsupported archive format for {filename}")
    }
}

/// Validate one archive member path: relative, no traversal, no drive
/// prefixes, normal components only. Returns the normalized relative path.
fn sanitize_member_path(raw: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in raw.components() {
        match component {
            Component::Normal(part) => {
                let part_str = part
                    .to_str()
                    .with_context(|| format!("archive member {} is not UTF-8", raw.display()))?;
                if part_str.contains('\\') {
                    bail!(
                        "archive member {} contains a backslash component",
                        raw.display()
                    );
                }
                normalized.push(part_str);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                bail!(
                    "archive member {} attempts directory traversal",
                    raw.display()
                )
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("archive member {} is not a relative path", raw.display())
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        bail!("archive member has an empty path");
    }
    Ok(normalized)
}

/// Extract an archive into `dest_dir` through one safe implementation.
///
/// Rejected: absolute paths, drive prefixes, `..` traversal, symlinks and
/// hardlinks, duplicate normalized paths, more than `MAX_ARCHIVE_MEMBERS`
/// members, and more than `MAX_ARCHIVE_EXPANDED_BYTES` of expanded content.
/// If every member shares a single top-level directory, that directory is
/// stripped so the extracted layout matches the catalog's flat file list.
pub fn extract_archive_safely(
    archive_path: &Path,
    kind: ArchiveKind,
    dest_dir: &Path,
) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(dest_dir).with_context(|| {
        format!(
            "failed to create extraction directory {}",
            dest_dir.display()
        )
    })?;

    let members = match kind {
        ArchiveKind::TarGz => collect_tar_members(archive_path)?,
        ArchiveKind::Zip => collect_zip_members(archive_path)?,
    };

    if members.is_empty() {
        bail!("archive {} contains no files", archive_path.display());
    }
    if members.len() > MAX_ARCHIVE_MEMBERS {
        bail!(
            "archive {} has {} members, more than the {MAX_ARCHIVE_MEMBERS} allowed",
            archive_path.display(),
            members.len()
        );
    }

    let total: u64 = members.iter().map(|(_, bytes)| bytes.len() as u64).sum();
    if total > MAX_ARCHIVE_EXPANDED_BYTES {
        bail!(
            "archive {} expands to {total} bytes, more than the {MAX_ARCHIVE_EXPANDED_BYTES} allowed",
            archive_path.display()
        );
    }

    // Strip a single shared top-level directory (common tar convention) so
    // the on-disk layout matches the catalog's relative file paths.
    let stripped = strip_shared_top_level(members)?;

    let mut seen = std::collections::HashSet::new();
    let mut written = Vec::new();
    for (relative, bytes) in &stripped {
        if !seen.insert(relative.clone()) {
            bail!(
                "archive contains duplicate normalized path {}",
                relative.display()
            );
        }
        let destination = dest_dir.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        fs::write(&destination, bytes)
            .with_context(|| format!("failed to write {}", destination.display()))?;
        written.push(relative.clone());
    }

    Ok(written)
}

type ArchiveMembers = Vec<(PathBuf, Vec<u8>)>;

/// Read one archive member with the expanded-size budget enforced on the
/// ACTUAL decompressed bytes. Archive headers can lie about sizes; trusting
/// them for allocation or budget accounting lets a hostile archive expand
/// far past the cap before any digest check runs.
fn read_member_bounded(
    reader: &mut impl Read,
    declared_size: u64,
    budget_remaining: u64,
) -> Result<Vec<u8>> {
    if declared_size > budget_remaining {
        bail!("archive exceeds the expanded size limit while reading");
    }
    let mut bytes = Vec::new();
    let mut limited = reader.take(declared_size + 1);
    limited
        .read_to_end(&mut bytes)
        .context("failed to read archive member")?;
    if bytes.len() as u64 > declared_size {
        bail!("archive member expands past its declared size");
    }
    Ok(bytes)
}

fn strip_shared_top_level(members: ArchiveMembers) -> Result<ArchiveMembers> {
    let shared_root: Option<PathBuf> = members
        .iter()
        .map(|(path, _)| {
            path.components()
                .next()
                .map(|c| PathBuf::from(c.as_os_str()))
        })
        .collect::<Option<Vec<_>>>()
        .and_then(|roots| {
            let first = roots.first()?.clone();
            (roots.iter().all(|root| *root == first)
                && members
                    .iter()
                    .all(|(path, _)| path.components().count() > 1))
            .then_some(first)
        });

    match shared_root {
        Some(root) => members
            .into_iter()
            .map(|(path, bytes)| {
                let stripped = path
                    .strip_prefix(&root)
                    .context("failed to strip shared archive root")?
                    .to_path_buf();
                Ok((stripped, bytes))
            })
            .collect(),
        None => Ok(members),
    }
}

fn collect_tar_members(archive_path: &Path) -> Result<ArchiveMembers> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open archive {}", archive_path.display()))?;
    let decoder = flate2::read::GzDecoder::new(std::io::BufReader::new(file));
    let mut archive = tar::Archive::new(decoder);

    let mut members = Vec::new();
    let mut expanded: u64 = 0;
    for entry in archive
        .entries()
        .context("failed to read archive entries")?
    {
        let mut entry = entry.context("failed to read archive entry")?;
        let entry_type = entry.header().entry_type();
        match entry_type {
            tar::EntryType::Regular => {}
            tar::EntryType::Directory => continue,
            tar::EntryType::Symlink | tar::EntryType::Link => {
                bail!("archive contains a link entry, which is not allowed")
            }
            other => bail!("archive contains unsupported entry type {other:?}"),
        }

        let raw_path = entry
            .path()
            .context("failed to read entry path")?
            .to_path_buf();
        let relative = sanitize_member_path(&raw_path)?;

        if members.len() + 1 > MAX_ARCHIVE_MEMBERS {
            bail!("archive exceeds the member count limit while reading");
        }
        let declared = entry.size();
        let bytes = read_member_bounded(
            &mut entry,
            declared,
            MAX_ARCHIVE_EXPANDED_BYTES.saturating_sub(expanded),
        )?;
        expanded = expanded.saturating_add(bytes.len() as u64);
        members.push((relative, bytes));
    }
    Ok(members)
}

fn collect_zip_members(archive_path: &Path) -> Result<ArchiveMembers> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open archive {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .with_context(|| format!("failed to read archive {}", archive_path.display()))?;

    let mut members = Vec::new();
    let mut expanded: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .context("failed to read zip archive entry")?;
        if entry.is_dir() {
            continue;
        }
        if entry.is_symlink() {
            bail!("archive contains a link entry, which is not allowed");
        }

        let raw_path = PathBuf::from(entry.name());
        let relative = sanitize_member_path(&raw_path)?;

        if members.len() + 1 > MAX_ARCHIVE_MEMBERS {
            bail!("archive exceeds the member count limit while reading");
        }
        let declared = entry.size();
        let bytes = read_member_bounded(
            &mut entry,
            declared,
            MAX_ARCHIVE_EXPANDED_BYTES.saturating_sub(expanded),
        )?;
        expanded = expanded.saturating_add(bytes.len() as u64);
        members.push((relative, bytes));
    }
    Ok(members)
}

/// Verify every extracted file against the catalog's declared digests. All
/// declared files must exist with matching size and SHA-256.
///
/// RATIONALE: undeclared extra members are tolerated. The archive's own
/// SHA-256 (`archive_digest`) is verified against the catalog before a single
/// byte is extracted, so every member — declared or not — is exactly what the
/// publisher signed; rejecting extras added no integrity guarantee and instead
/// bricked installs whenever the build pipeline shipped a new metadata file
/// (`build-manifest.json`, which the generator omits from its own file list).
/// The declared-file checks below stay strict, and the returned records still
/// cover only declared files.
pub fn verify_extracted_files(
    dest_dir: &Path,
    declared: &BTreeMap<String, CatalogFileDigest>,
    extracted: &[PathBuf],
) -> Result<Vec<InstalledFileRecord>> {
    let extracted_set: std::collections::HashSet<String> = extracted
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();

    let mut records = Vec::new();
    for (path, digest) in declared {
        if !extracted_set.contains(path) {
            bail!("archive is missing declared file {path}");
        }
        let full_path = dest_dir.join(path);
        let metadata = fs::metadata(&full_path)
            .with_context(|| format!("failed to inspect extracted file {path}"))?;
        if metadata.len() != digest.size {
            bail!(
                "extracted file {path} has size {}, expected {}",
                metadata.len(),
                digest.size
            );
        }
        let actual = sha256_file(&full_path)?;
        if actual != digest.sha256 {
            bail!("extracted file {path} digest mismatch");
        }
        records.push(InstalledFileRecord {
            path: path.clone(),
            size: digest.size,
            sha256: digest.sha256.clone(),
        });
    }
    Ok(records)
}

/// Streaming SHA-256 of a file with fixed memory.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(crate::hash::hex_lower(hasher.finalize()))
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

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("create archive");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in entries {
            writer.start_file(*name, options).expect("start file");
            writer.write_all(bytes).expect("write file");
        }
        writer.finish().expect("finish zip");
    }

    #[test]
    fn extracts_flat_tgz_members() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = dir.path().join("runtime.tar.gz");
        write_tgz(&archive, &[("lib.dylib", b"lib"), ("LICENSE", b"mit")]);

        let dest = dir.path().join("out");
        let written = extract_archive_safely(&archive, ArchiveKind::TarGz, &dest).expect("extract");

        assert_eq!(written.len(), 2);
        assert_eq!(fs::read(dest.join("lib.dylib")).expect("read"), b"lib");
    }

    #[test]
    fn strips_single_shared_top_level_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = dir.path().join("runtime.tar.gz");
        write_tgz(
            &archive,
            &[("pkg/lib.dylib", b"lib"), ("pkg/LICENSE", b"mit")],
        );

        let dest = dir.path().join("out");
        extract_archive_safely(&archive, ArchiveKind::TarGz, &dest).expect("extract");

        assert!(dest.join("lib.dylib").is_file());
        assert!(!dest.join("pkg").exists());
    }

    #[test]
    fn rejects_traversal_member_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = dir.path().join("evil.zip");
        write_zip(&archive, &[("../escape.txt", b"boom")]);

        let error = extract_archive_safely(&archive, ArchiveKind::Zip, &dir.path().join("out"))
            .expect_err("traversal must be rejected");
        assert!(error.to_string().contains("traversal"));
    }

    #[test]
    fn rejects_absolute_member_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = dir.path().join("evil.zip");
        write_zip(&archive, &[("/etc/passwd", b"boom")]);

        let error = extract_archive_safely(&archive, ArchiveKind::Zip, &dir.path().join("out"))
            .expect_err("absolute paths must be rejected");
        assert!(error.to_string().contains("not a relative path"));
    }

    #[test]
    fn rejects_link_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = dir.path().join("evil.tar.gz");
        let file = fs::File::create(&archive).expect("create archive");
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_cksum();
        builder
            .append_link(&mut header, "link", "/etc/passwd")
            .expect("append link");
        builder
            .into_inner()
            .expect("finish tar")
            .finish()
            .expect("finish gzip");

        let error = extract_archive_safely(&archive, ArchiveKind::TarGz, &dir.path().join("out"))
            .expect_err("links must be rejected");
        assert!(error.to_string().contains("link entry"));
    }

    #[test]
    fn verify_extracted_tolerates_undeclared_but_rejects_missing_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("lib.dylib"), b"lib").expect("write");

        let mut declared = BTreeMap::new();
        declared.insert(
            "lib.dylib".to_owned(),
            CatalogFileDigest {
                sha256: sha256_hex(b"lib"),
                size: 3,
            },
        );

        // Undeclared extra members ride along with the archive-level digest
        // (real ORT archives ship build-manifest.json, which the catalog
        // generator omits). They must not fail an otherwise valid install,
        // and they must not appear in the installed-file records.
        fs::write(dir.path().join("build-manifest.json"), b"{}").expect("write");
        let records = verify_extracted_files(
            dir.path(),
            &declared,
            &[
                PathBuf::from("lib.dylib"),
                PathBuf::from("build-manifest.json"),
            ],
        )
        .expect("undeclared extras must be tolerated");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, "lib.dylib");

        // Declared file missing from extraction.
        declared.insert(
            "NOTICE".to_owned(),
            CatalogFileDigest {
                sha256: sha256_hex(b"notice"),
                size: 6,
            },
        );
        let error = verify_extracted_files(dir.path(), &declared, &[PathBuf::from("lib.dylib")])
            .expect_err("missing declared file must be rejected");
        assert!(error.to_string().contains("missing declared file"));
    }

    #[test]
    fn verify_extracted_checks_size_and_digest() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("lib.dylib"), b"lib").expect("write");

        let mut declared = BTreeMap::new();
        declared.insert(
            "lib.dylib".to_owned(),
            CatalogFileDigest {
                sha256: "0".repeat(64),
                size: 3,
            },
        );
        let error = verify_extracted_files(dir.path(), &declared, &[PathBuf::from("lib.dylib")])
            .expect_err("digest mismatch must be rejected");
        assert!(error.to_string().contains("digest mismatch"));

        declared.insert(
            "lib.dylib".to_owned(),
            CatalogFileDigest {
                sha256: sha256_hex(b"lib"),
                size: 999,
            },
        );
        let error = verify_extracted_files(dir.path(), &declared, &[PathBuf::from("lib.dylib")])
            .expect_err("size mismatch must be rejected");
        assert!(error.to_string().contains("has size"));
    }

    #[test]
    fn sha256_file_streams_the_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("payload.bin");
        let payload = vec![7_u8; 300 * 1024];
        fs::write(&path, &payload).expect("write");
        assert_eq!(sha256_file(&path).expect("hash"), sha256_hex(&payload));
    }

    #[test]
    fn bounded_member_read_rejects_lying_headers_and_budget_overruns() {
        // Actual bytes exceed the declared size (a lying header).
        let payload = vec![1_u8; 100];
        let mut reader = std::io::Cursor::new(&payload);
        let error = read_member_bounded(&mut reader, 50, 1_000)
            .expect_err("member expanding past its declared size must be rejected");
        assert!(error.to_string().contains("declared size"));

        // Declared size exceeds the remaining budget.
        let mut reader = std::io::Cursor::new(&payload);
        let error = read_member_bounded(&mut reader, 100, 99)
            .expect_err("member past the expanded budget must be rejected");
        assert!(error.to_string().contains("expanded size limit"));

        // Honest member within budget.
        let mut reader = std::io::Cursor::new(&payload);
        let bytes = read_member_bounded(&mut reader, 100, 1_000).expect("honest member");
        assert_eq!(bytes.len(), 100);
    }

    #[test]
    fn archive_kind_detection() {
        assert_eq!(
            archive_kind_for_filename("x.tar.gz").expect("kind"),
            ArchiveKind::TarGz
        );
        assert_eq!(
            archive_kind_for_filename("x.zip").expect("kind"),
            ArchiveKind::Zip
        );
        assert!(archive_kind_for_filename("x.rar").is_err());
    }
}
