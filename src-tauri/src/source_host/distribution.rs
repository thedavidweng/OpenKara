use super::{
    binding, install, network,
    runtime::{BoxFuture, Progress},
    test_http,
    types::*,
    unknown,
};
use base64::Engine;
use ring::signature;
use std::{collections::BTreeMap, fs, path::PathBuf};

const INVALID_INDEX: &str =
    "Source index signature or release data is invalid. Keep the installed version and retry discovery later.";

pub(crate) struct FetchedIndex {
    pub index: SourceIndex,
    pub bytes: Vec<u8>,
}

pub(crate) trait SourceDistribution: Send + Sync {
    /// Returns the last verified index, or the error that made the cached copy unusable.
    fn cached_index(&self, config: &HostConfig) -> (Option<SourceIndex>, Option<HostError>);
    fn fetch_index<'a>(&'a self, config: &'a HostConfig)
        -> BoxFuture<'a, HostResult<FetchedIndex>>;
    fn cache_index(&self, config: &HostConfig, bytes: &[u8]) -> std::io::Result<()>;
    /// Downloads and seals the release package when no installed copy exists, then
    /// returns the verified package directory.
    fn install_package<'a>(
        &'a self,
        config: &'a HostConfig,
        release: &'a Release,
        cancel: &'a Cancellation,
        progress: Progress<'a>,
    ) -> BoxFuture<'a, HostResult<PathBuf>>;
    fn verified_package(&self, config: &HostConfig, release: &Release) -> HostResult<PathBuf>;
    fn choose_repair_slot(&self, config: &HostConfig, release: &mut Release);
}

pub(crate) struct SignedIndexDistribution;

impl SourceDistribution for SignedIndexDistribution {
    fn cached_index(&self, config: &HostConfig) -> (Option<SourceIndex>, Option<HostError>) {
        match fs::read(config.data_root.join("verified-index.json")) {
            Ok(bytes) => match verify_index(config, &bytes) {
                Ok(index) => (Some(index), None),
                Err(error) => (None, Some(error)),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, None),
            Err(error) => (None, Some(HostError::from(error))),
        }
    }
    fn fetch_index<'a>(
        &'a self,
        config: &'a HostConfig,
    ) -> BoxFuture<'a, HostResult<FetchedIndex>> {
        Box::pin(async move {
            let cancel = Cancellation::new();
            let response = network::response(
                &config.index_url,
                &BTreeMap::new(),
                None,
                test_http(config),
                &cancel,
            )
            .await?;
            let bytes = network::read_bounded(response, 8 * 1024 * 1024, &cancel).await?;
            let index = verify_index(config, &bytes)?;
            Ok(FetchedIndex { index, bytes })
        })
    }
    fn cache_index(&self, config: &HostConfig, bytes: &[u8]) -> std::io::Result<()> {
        let temp = config.data_root.join("verified-index.json.tmp");
        fs::write(&temp, bytes)?;
        fs::rename(temp, config.data_root.join("verified-index.json"))
    }
    fn install_package<'a>(
        &'a self,
        config: &'a HostConfig,
        release: &'a Release,
        cancel: &'a Cancellation,
        progress: Progress<'a>,
    ) -> BoxFuture<'a, HostResult<PathBuf>> {
        Box::pin(async move {
            let package = install::package_path(config, release);
            if !package.exists() {
                let total = Some(release.archive.size);
                progress(0, total);
                let staged = install::artifact(config, &release.archive, cancel, |done| {
                    progress(done, total)
                })
                .await?;
                let result: HostResult<()> = (|| {
                    install::validate_package(&staged, release)?;
                    install::seal(&staged)?;
                    fs::create_dir_all(package.parent().ok_or_else(unknown)?)?;
                    fs::rename(&staged, &package)?;
                    Ok(())
                })();
                let _ = fs::remove_dir_all(staged.parent().unwrap());
                result?;
            }
            install::verify_package(config, release)
        })
    }
    fn verified_package(&self, config: &HostConfig, release: &Release) -> HostResult<PathBuf> {
        install::verify_package(config, release)
    }
    fn choose_repair_slot(&self, config: &HostConfig, release: &mut Release) {
        install::choose_package_slot(config, release);
    }
}

fn verify_index(config: &HostConfig, bytes: &[u8]) -> HostResult<SourceIndex> {
    let error = || HostError::new("untrusted_index", INVALID_INDEX);
    let envelope: SignedIndex = serde_json::from_slice(bytes).map_err(|_| error())?;
    let key = config
        .trusted_keys
        .get(&envelope.key_id)
        .ok_or_else(error)?;
    let payload = base64::engine::general_purpose::STANDARD
        .decode(&envelope.payload)
        .map_err(|_| error())?;
    let signed = base64::engine::general_purpose::STANDARD
        .decode(&envelope.signature)
        .map_err(|_| error())?;
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(&payload, &signed)
        .map_err(|_| error())?;
    let index: SourceIndex = serde_json::from_slice(&payload).map_err(|_| error())?;
    if index.schema_version != 1 || index.sources.len() > 1000 {
        return Err(error());
    }
    let mut identities = BTreeMap::new();
    for source in &index.sources {
        let id = &source.source_id;
        if id.is_empty()
            || id.len() > 64
            || !id.as_bytes()[0].is_ascii_lowercase()
            || !id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
            })
            || identities
                .insert(id, binding(&source.publisher, &source.package_id))
                .is_some()
            || source.publisher.is_empty()
            || source.package_id.is_empty()
            || source.publisher.contains('/')
            || source.package_id.contains('/')
        {
            return Err(error());
        }
        let mut versions = std::collections::BTreeSet::new();
        for release in &source.releases {
            let manifest = &release.manifest;
            if manifest.manifest_version != 1
                || manifest.source_id != *id
                || manifest.publisher != source.publisher
                || manifest.package_id != source.package_id
                || !versions.insert(&manifest.version)
                || semver::Version::parse(&manifest.version).is_err()
                || semver::Version::parse(&manifest.minimum_host_version).is_err()
                || manifest.protocol.majors.is_empty()
                || manifest.runtime.kind != "node"
                || !manifest.entry.ends_with(".mjs")
                || install::relative(&manifest.entry).is_err()
                || manifest.name.is_empty()
                || manifest.license.is_empty()
            {
                return Err(error());
            }
            let runtime = semver::Version::parse(&release.runtime.version).map_err(|_| error())?;
            if runtime.major != manifest.runtime.major
                || release.runtime.targets.is_empty()
                || !release.archive.executable.is_empty()
                || release.installed_package.is_some()
                || release.installed_runtime.is_some()
            {
                return Err(error());
            }
            install::validate_artifact(&release.archive)?;
            for artifact in release.runtime.targets.values() {
                install::validate_artifact(artifact)?;
                install::relative(&artifact.executable)?;
            }
            for origin in manifest
                .resource_origins
                .iter()
                .chain(&manifest.media_origins)
            {
                if network::origin(origin)? != *origin {
                    return Err(error());
                }
            }
            match manifest.authentication["flow"].as_str() {
                Some("none") => {
                    if !manifest.requires_authentication.is_empty() {
                        return Err(error());
                    }
                }
                Some("plugin") => {
                    let presentations = manifest.authentication["presentations"]
                        .as_array()
                        .ok_or_else(error)?;
                    if presentations.is_empty()
                        || presentations
                            .iter()
                            .any(|p| !matches!(p.as_str(), Some("qr" | "text")))
                    {
                        return Err(error());
                    }
                }
                Some("web") => {
                    let auth = &manifest.authentication;
                    let url = auth["sign_in_url"].as_str().ok_or_else(error)?;
                    let origin = network::origin(url)?;
                    let origins = auth["navigation_origins"].as_array().ok_or_else(error)?;
                    if !origins
                        .iter()
                        .any(|value| value.as_str() == Some(origin.as_str()))
                        || auth["cookie_names"].as_array().is_none_or(|cookies| {
                            cookies.is_empty()
                                || cookies.iter().any(|cookie| {
                                    cookie.as_str().is_none_or(|name| {
                                        name.is_empty()
                                            || name.contains(['\r', '\n', ';', '=', ' '])
                                    })
                                })
                        })
                    {
                        return Err(error());
                    }
                    for origin in origins {
                        let origin = origin.as_str().ok_or_else(error)?;
                        if network::origin(origin)? != origin {
                            return Err(error());
                        }
                    }
                }
                _ => return Err(error()),
            }
        }
    }
    Ok(index)
}
