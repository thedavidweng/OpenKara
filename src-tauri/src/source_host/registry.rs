use super::{
    digest_hex, engine, incompatible, install, network, runtime::Progress, test_http, types::*,
};
use base64::Engine;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const FORMAT_VERSION: u32 = 1;
const MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PACKAGES: usize = 1000;
const MAX_VERSIONS: usize = 200;
const MAX_URLS: usize = 8;
/// Packages are one bundled ES module plus a manifest, so a small cap applies
/// while bytes arrive, before integrity verification.
pub(crate) const MAX_PACKAGE_BYTES: u64 = 32 * 1024 * 1024;
const CACHE_FILE: &str = "registry-cache.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SourceRegistry {
    pub format_version: u32,
    pub registry_id: String,
    pub display_name: String,
    pub homepage: String,
    pub packages: Vec<RegistryPackage>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RegistryPackage {
    pub package_id: String,
    pub source_id: String,
    pub name: String,
    pub description: String,
    pub license: String,
    pub versions: Vec<RegistryVersion>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RegistryVersion {
    pub version: String,
    pub protocol_majors: Vec<u32>,
    pub minimum_host_version: String,
    pub size: u64,
    pub integrity: String,
    pub urls: Vec<String>,
    pub resource_origins: Vec<String>,
    pub media_origins: Vec<String>,
    pub capabilities: Vec<String>,
}

/// One added Source Registry with its last good listing.
pub(crate) struct RegistryEntry {
    pub url: String,
    pub registry_id: String,
    pub document: Option<SourceRegistry>,
    pub refreshed_at: Option<u64>,
    pub attempted_at: Option<Instant>,
    pub error: Option<HostError>,
}
impl RegistryEntry {
    pub fn status(&self) -> RegistryStatus {
        RegistryStatus {
            url: self.url.clone(),
            registry_id: self.registry_id.clone(),
            display_name: self
                .document
                .as_ref()
                .map(|document| document.display_name.clone())
                .unwrap_or_else(|| self.registry_id.clone()),
            homepage: self
                .document
                .as_ref()
                .map(|document| document.homepage.clone())
                .unwrap_or_default(),
            refreshed_at: self.refreshed_at,
            last_error: self.error.clone(),
        }
    }
    pub fn due(&self, interval: std::time::Duration) -> bool {
        let interval_ms = u64::try_from(interval.as_millis()).unwrap_or(u64::MAX);
        self.attempted_at
            .is_none_or(|attempted| attempted.elapsed() >= interval)
            && self
                .refreshed_at
                .is_none_or(|refreshed| now_ms().saturating_sub(refreshed) >= interval_ms)
    }
}

#[derive(Serialize, Deserialize)]
struct CachedRegistry {
    refreshed_at: u64,
    document: SourceRegistry,
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

fn invalid_registry(message: &str) -> HostError {
    HostError::new("invalid_registry", message)
}
fn invalid_document() -> HostError {
    invalid_registry(
        "The Source Registry document is invalid. OpenKara keeps the last good listing. Ask the registry maintainer to fix it.",
    )
}
fn mismatch() -> HostError {
    HostError::new(
        "invalid_package",
        "The source package does not match its Source Registry entry. Ask the registry maintainer for a corrected release.",
    )
}
fn invalid_package() -> HostError {
    HostError::new(
        "invalid_package",
        "The source package has an invalid structure. Ask the registry maintainer for a corrected release.",
    )
}

/// Returns the canonical form of a registry or download URL, which must use
/// HTTPS. Test configurations also admit plain HTTP on loopback.
pub(crate) fn https_url(config: &HostConfig, value: &str) -> HostResult<String> {
    let url = Url::parse(value).map_err(|_| network::invalid())?;
    let test_loopback = test_http(config)
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if !test_loopback {
        network::origin(url.as_str())?;
    }
    Ok(url.to_string())
}

fn valid_source_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.as_bytes()[0].is_ascii_lowercase()
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
}
fn valid_license(license: &str) -> bool {
    !license.trim().is_empty()
        && license.len() <= 256
        && license
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " .-+:()".contains(c))
}
fn valid_capability(capability: &str) -> bool {
    !capability.is_empty()
        && capability.len() <= 64
        && capability
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}
fn unique<T: Ord>(values: impl IntoIterator<Item = T>) -> bool {
    let mut seen = BTreeSet::new();
    values.into_iter().all(|value| seen.insert(value))
}
fn exact_origins(origins: &[String]) -> bool {
    unique(origins)
        && origins
            .iter()
            .all(|origin| network::origin(origin).is_ok_and(|exact| exact == *origin))
}

/// Decodes an npm-style Subresource Integrity value; only one SHA-512 digest is accepted.
fn sha512(integrity: &str) -> Option<Vec<u8>> {
    let digest = base64::engine::general_purpose::STANDARD
        .decode(integrity.strip_prefix("sha512-")?)
        .ok()?;
    (digest.len() == 64).then_some(digest)
}

pub(crate) fn validate(config: &HostConfig, registry: &SourceRegistry) -> HostResult<()> {
    let homepage = Url::parse(&registry.homepage).map_err(|_| invalid_document())?;
    if registry.format_version != FORMAT_VERSION
        || !valid_id(&registry.registry_id)
        || registry.display_name.trim().is_empty()
        || homepage.scheme() != "https"
        || homepage.host_str().is_none()
        || registry.packages.len() > MAX_PACKAGES
        || !unique(registry.packages.iter().map(|package| &package.package_id))
    {
        return Err(invalid_document());
    }
    for package in &registry.packages {
        if !valid_id(&package.package_id)
            || !valid_source_id(&package.source_id)
            || package.name.trim().is_empty()
            || !valid_license(&package.license)
            || package.versions.is_empty()
            || package.versions.len() > MAX_VERSIONS
            || !unique(package.versions.iter().map(|version| &version.version))
        {
            return Err(invalid_document());
        }
        for version in &package.versions {
            if semver::Version::parse(&version.version).is_err()
                || semver::Version::parse(&version.minimum_host_version).is_err()
                || version.protocol_majors.is_empty()
                || !unique(&version.protocol_majors)
                || version.size == 0
                || version.size > MAX_PACKAGE_BYTES
                || sha512(&version.integrity).is_none()
                || version.urls.is_empty()
                || version.urls.len() > MAX_URLS
                || version
                    .urls
                    .iter()
                    .any(|url| https_url(config, url).is_err())
                || !exact_origins(&version.resource_origins)
                || !exact_origins(&version.media_origins)
                || !unique(&version.capabilities)
                || !version
                    .capabilities
                    .iter()
                    .all(|capability| valid_capability(capability))
            {
                return Err(invalid_document());
            }
        }
    }
    Ok(())
}

pub(crate) async fn fetch(config: &HostConfig, url: &str) -> HostResult<SourceRegistry> {
    let cancel = Cancellation::new();
    let response = network::response_range(
        url,
        &BTreeMap::new(),
        None,
        test_http(config),
        &cancel,
        None,
    )
    .await?;
    if !response.status().is_success() {
        return Err(HostError::new(
            "network",
            format!(
                "The Source Registry is unavailable (HTTP {}). OpenKara keeps the last good listing. Retry later.",
                response.status().as_u16()
            ),
        ));
    }
    let bytes = network::read_bounded(response, MAX_DOCUMENT_BYTES, &cancel)
        .await
        .map_err(|error| {
            if error.code == "invalid_resource" {
                invalid_document()
            } else {
                error
            }
        })?;
    let registry: SourceRegistry =
        serde_json::from_slice(&bytes).map_err(|_| invalid_document())?;
    validate(config, &registry)?;
    Ok(registry)
}

pub(crate) fn restore(
    config: &HostConfig,
    stored: &[StoredRegistry],
) -> HostResult<Vec<RegistryEntry>> {
    let mut cache: BTreeMap<String, CachedRegistry> =
        match fs::read(config.data_root.join(CACHE_FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
    Ok(stored
        .iter()
        .map(|registry| {
            let cached = cache.remove(&registry.url).filter(|cached| {
                cached.document.registry_id == registry.registry_id
                    && validate(config, &cached.document).is_ok()
            });
            RegistryEntry {
                url: registry.url.clone(),
                registry_id: registry.registry_id.clone(),
                refreshed_at: cached.as_ref().map(|cached| cached.refreshed_at),
                document: cached.map(|cached| cached.document),
                attempted_at: None,
                error: None,
            }
        })
        .collect())
}

pub(crate) fn save_cache(config: &HostConfig, entries: &[RegistryEntry]) -> HostResult<()> {
    let cache: BTreeMap<&str, CachedRegistry> = entries
        .iter()
        .filter_map(|entry| {
            Some((
                entry.url.as_str(),
                CachedRegistry {
                    refreshed_at: entry.refreshed_at?,
                    document: entry.document.clone()?,
                },
            ))
        })
        .collect();
    fs::create_dir_all(&config.data_root)?;
    let temp = config.data_root.join(format!("{CACHE_FILE}.tmp"));
    fs::write(
        &temp,
        serde_json::to_vec(&cache)
            .map_err(|_| HostError::new("storage", "Cannot save the Source Registry listing."))?,
    )?;
    fs::rename(temp, config.data_root.join(CACHE_FILE))?;
    Ok(())
}

pub(crate) fn package<'a>(
    entries: &'a [RegistryEntry],
    identity: &PackageIdentity,
) -> Option<&'a RegistryPackage> {
    entries
        .iter()
        .filter(|entry| entry.registry_id == identity.registry_id)
        .filter_map(|entry| entry.document.as_ref())
        .flat_map(|document| &document.packages)
        .find(|package| package.package_id == identity.package_id)
}

pub(crate) fn newest(package: &RegistryPackage) -> Option<&RegistryVersion> {
    package
        .versions
        .iter()
        .max_by_key(|version| semver::Version::parse(&version.version).ok())
}

/// Selects the newest listed version that this host can run, honoring an exact
/// version request.
pub(crate) fn select<'a>(
    config: &HostConfig,
    package: &'a RegistryPackage,
    wanted: Option<&str>,
) -> HostResult<&'a RegistryVersion> {
    let host = semver::Version::parse(&config.host_version).ok();
    package
        .versions
        .iter()
        .filter(|version| {
            wanted.is_none_or(|wanted| wanted == version.version)
                && version.protocol_majors.contains(&engine::PROTOCOL_MAJOR)
                && host
                    .as_ref()
                    .zip(semver::Version::parse(&version.minimum_host_version).ok())
                    .is_some_and(|(host, minimum)| *host >= minimum)
        })
        .max_by_key(|version| semver::Version::parse(&version.version).ok())
        .ok_or_else(incompatible)
}

enum Failure {
    Cancelled(HostError),
    Unavailable(String),
    WrongBytes(String),
}

async fn fetch_package(
    config: &HostConfig,
    url: &str,
    size: u64,
    cancel: &Cancellation,
    progress: Progress<'_>,
) -> Result<Vec<u8>, Failure> {
    let larger = || Failure::WrongBytes("larger than the declared size".into());
    let mut response =
        network::response_range(url, &BTreeMap::new(), None, test_http(config), cancel, None)
            .await
            .map_err(|error| match error.code.as_str() {
                "cancelled" => Failure::Cancelled(error),
                "invalid_resource" => {
                    Failure::Unavailable("not a permitted public HTTPS address".into())
                }
                _ => Failure::Unavailable("unreachable".into()),
            })?;
    if !response.status().is_success() {
        return Err(Failure::Unavailable(format!(
            "HTTP {}",
            response.status().as_u16()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > size)
    {
        return Err(larger());
    }
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => {
                return Err(Failure::Cancelled(HostError::new(
                    "cancelled",
                    "Source download cancelled. Retry when ready.",
                )));
            }
            chunk = response.chunk() => chunk.map_err(|_| Failure::Unavailable("interrupted".into()))?,
        };
        let Some(chunk) = chunk else { break };
        if bytes.len() as u64 + chunk.len() as u64 > size {
            return Err(larger());
        }
        bytes.extend_from_slice(&chunk);
        progress(bytes.len() as u64, Some(size));
    }
    if (bytes.len() as u64) < size {
        return Err(Failure::WrongBytes("smaller than the declared size".into()));
    }
    Ok(bytes)
}

/// A registry version chosen for installation, with the package that lists it.
pub(crate) struct Candidate {
    pub identity: PackageIdentity,
    pub package: RegistryPackage,
    pub version: RegistryVersion,
}

/// Downloads a listed version from the first URL that serves its exact bytes,
/// verifies them before extraction, and seals the package tree.
pub(crate) async fn download(
    config: &HostConfig,
    candidate: &Candidate,
    cancel: &Cancellation,
    progress: Progress<'_>,
) -> HostResult<Release> {
    let version = &candidate.version;
    let expected = sha512(&version.integrity).ok_or_else(invalid_document)?;
    let mut tried = Vec::new();
    let mut wrong_bytes = false;
    let mut verified = None;
    for url in &version.urls {
        progress(0, Some(version.size));
        match fetch_package(config, url, version.size, cancel, progress).await {
            Ok(bytes) if Sha512::digest(&bytes).as_slice() == expected.as_slice() => {
                verified = Some((url, bytes));
                break;
            }
            Ok(_) => {
                wrong_bytes = true;
                tried.push(format!("{url} (integrity mismatch)"));
            }
            Err(Failure::Cancelled(error)) => return Err(error),
            Err(Failure::Unavailable(reason)) => tried.push(format!("{url} ({reason})")),
            Err(Failure::WrongBytes(reason)) => {
                wrong_bytes = true;
                tried.push(format!("{url} ({reason})"));
            }
        }
    }
    let Some((url, bytes)) = verified else {
        return Err(HostError::new(
            if wrong_bytes {
                "invalid_package"
            } else {
                "network"
            },
            format!(
                "Cannot download the source package from any listed URL: {}. Retry later or ask the registry maintainer.",
                tried.join("; ")
            ),
        ));
    };
    cancel.check()?;
    let staging = config
        .data_root
        .join("staging")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&staging)?;
    let result = install_verified(config, candidate, (url, &bytes), &staging, cancel);
    let _ = fs::remove_dir_all(&staging);
    result
}

fn install_verified(
    config: &HostConfig,
    candidate: &Candidate,
    (url, bytes): (&str, &[u8]),
    staging: &std::path::Path,
    cancel: &Cancellation,
) -> HostResult<Release> {
    let Candidate {
        identity,
        package,
        version,
    } = candidate;
    let archive = staging.join("archive");
    fs::write(&archive, bytes)?;
    let files = staging.join("files");
    fs::create_dir(&files)?;
    install::extract_filtered(&archive, &files, "tar.gz", cancel, None)?;
    let mut members = fs::read_dir(&files)?.collect::<Result<Vec<_>, _>>()?;
    let root = match members.pop() {
        Some(member) if members.is_empty() && member.file_name() == "package" => member.path(),
        _ => return Err(invalid_package()),
    };
    if !root.is_dir() {
        return Err(invalid_package());
    }
    let manifest: SourceManifest = serde_json::from_slice(
        &fs::read(root.join("manifest.json")).map_err(|_| invalid_package())?,
    )
    .map_err(|_| invalid_package())?;
    check_manifest(&manifest, package, version)?;
    if !root.join(install::relative(&manifest.entry)?).is_file() {
        return Err(invalid_package());
    }
    let mut release = Release {
        manifest,
        archive: Artifact {
            url: url.into(),
            size: version.size,
            sha256: digest_hex(Sha256::digest(bytes)),
            format: "tar.gz".into(),
        },
        installed_package: None,
        registry: Some(identity.clone()),
    };
    install::choose_package_slot(config, &mut release);
    let target = install::package_path(config, &release);
    if !target.exists() {
        install::seal(&root)?;
        fs::create_dir_all(target.parent().ok_or_else(invalid_package)?)?;
        fs::rename(&root, &target)?;
    }
    verified_package(config, &release)?;
    Ok(release)
}

fn check_manifest(
    manifest: &SourceManifest,
    package: &RegistryPackage,
    version: &RegistryVersion,
) -> HostResult<()> {
    let same = |left: &[String], right: &[String]| {
        left.iter().collect::<BTreeSet<_>>() == right.iter().collect::<BTreeSet<_>>()
    };
    if !basic_manifest(manifest)
        || validate_declarations(manifest).is_err()
        || manifest.source_id != package.source_id
        || manifest.package_id != package.package_id
        || manifest.version != version.version
        || manifest.minimum_host_version != version.minimum_host_version
        || manifest.license != package.license
        || manifest.protocol.majors.iter().collect::<BTreeSet<_>>()
            != version.protocol_majors.iter().collect::<BTreeSet<_>>()
        || !same(&manifest.capabilities, &version.capabilities)
        || !same(&manifest.resource_origins, &version.resource_origins)
        || !same(&manifest.media_origins, &version.media_origins)
    {
        return Err(mismatch());
    }
    if manifest.runtime.kind != engine::RUNTIME_KIND
        || manifest.runtime.major != engine::RUNTIME_MAJOR
    {
        return Err(HostError::new(
            "incompatible_plugin",
            "Source Registry packages run only in the embedded engine. Ask the registry maintainer for a compatible release.",
        ));
    }
    Ok(())
}

/// Checks the manifest fields that do not depend on the registry entry.
fn basic_manifest(manifest: &SourceManifest) -> bool {
    manifest.manifest_version == 1
        && semver::Version::parse(&manifest.version).is_ok()
        && semver::Version::parse(&manifest.minimum_host_version).is_ok()
        && !manifest.protocol.majors.is_empty()
        && manifest.entry.ends_with(".mjs")
        && install::relative(&manifest.entry).is_ok()
        && !manifest.name.is_empty()
        && !manifest.license.is_empty()
}
/// Checks the declared origins and authentication flow of a manifest.
fn validate_declarations(manifest: &SourceManifest) -> HostResult<()> {
    for origin in manifest
        .resource_origins
        .iter()
        .chain(&manifest.media_origins)
    {
        if network::origin(origin)? != *origin {
            return Err(mismatch());
        }
    }
    match manifest.authentication["flow"].as_str() {
        Some("none") => {
            if !manifest.requires_authentication.is_empty() {
                return Err(mismatch());
            }
        }
        Some("plugin") => {
            let presentations = manifest.authentication["presentations"]
                .as_array()
                .ok_or_else(mismatch)?;
            if presentations.is_empty()
                || presentations
                    .iter()
                    .any(|p| !matches!(p.as_str(), Some("qr" | "text")))
            {
                return Err(mismatch());
            }
        }
        Some("web") => {
            let auth = &manifest.authentication;
            let url = auth["sign_in_url"].as_str().ok_or_else(mismatch)?;
            let origin = network::origin(url)?;
            let origins = auth["navigation_origins"].as_array().ok_or_else(mismatch)?;
            if !origins
                .iter()
                .any(|value| value.as_str() == Some(origin.as_str()))
                || auth["cookie_names"].as_array().is_none_or(|cookies| {
                    cookies.is_empty()
                        || cookies.iter().any(|cookie| {
                            cookie.as_str().is_none_or(|name| {
                                name.is_empty() || name.contains(['\r', '\n', ';', '=', ' '])
                            })
                        })
                })
            {
                return Err(mismatch());
            }
            for origin in origins {
                let origin = origin.as_str().ok_or_else(mismatch)?;
                if network::origin(origin)? != origin {
                    return Err(mismatch());
                }
            }
        }
        _ => return Err(mismatch()),
    }
    Ok(())
}

/// Confirms that an installed registry package still has its sealed files and manifest.
pub(crate) fn verified_package(config: &HostConfig, release: &Release) -> HostResult<PathBuf> {
    let package = install::package_path(config, release);
    install::verify_seal(&package)?;
    let manifest: SourceManifest =
        serde_json::from_slice(&fs::read(package.join("manifest.json"))?)
            .map_err(|_| invalid_package())?;
    if manifest != release.manifest || !package.join(install::relative(&manifest.entry)?).is_file()
    {
        return Err(invalid_package());
    }
    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::sha512;

    #[test]
    fn integrity_accepts_one_sha512_subresource_integrity_value() {
        let digest = format!("sha512-{}", "A".repeat(86) + "==");
        assert_eq!(sha512(&digest).map(|bytes| bytes.len()), Some(64));
        assert!(sha512("sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=").is_none());
        assert!(sha512(&format!("{digest} {digest}")).is_none());
        assert!(sha512("sha512-AAAA").is_none());
        assert!(sha512("").is_none());
    }
}
