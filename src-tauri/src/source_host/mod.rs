mod install;
mod network;
mod process;
mod types;
use base64::Engine;
pub use network::MediaResponse;
use process::Process;
pub use process::RequestHandle;
use ring::signature;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
pub use types::*;

#[derive(Clone)]
pub struct SourceHost {
    inner: Arc<Inner>,
}
struct Inner {
    config: HostConfig,
    state: Mutex<State>,
    changes: tokio::sync::broadcast::Sender<SourceSnapshot>,
    starts: Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>,
    installs: Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>,
    runtime_install: tokio::sync::Mutex<()>,
    initialization_error: Option<HostError>,
}
struct State {
    stored: StoredState,
    index: Option<SourceIndex>,
    sources: BTreeMap<String, Live>,
    sequence: u64,
}
#[derive(Default)]
struct Live {
    process: Option<Arc<Process>>,
    progress: Option<InstallProgress>,
    error: Option<HostError>,
    install: Option<Cancellation>,
    work: BTreeMap<u64, (WorkKind, Cancellation)>,
    failures: u32,
    retry_at: Option<Instant>,
    stopping: bool,
    failed_install: Option<String>,
}
pub struct WorkLease {
    inner: Weak<Inner>,
    source_id: String,
    id: u64,
    cancellation: Cancellation,
}
impl WorkLease {
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }
}
impl Drop for WorkLease {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            if let Some(live) = inner.state.lock().unwrap().sources.get_mut(&self.source_id) {
                live.work.remove(&self.id);
            }
            inner.publish(&self.source_id);
        }
    }
}

#[cfg(test)]
pub(crate) fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    digest_hex(Sha256::digest(bytes))
}
pub(crate) fn digest_hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 15) as usize] as char);
    }
    output
}
pub(crate) fn test_http(config: &HostConfig) -> bool {
    #[cfg(test)]
    {
        config.allow_test_distribution_http
    }
    #[cfg(not(test))]
    {
        let _ = config;
        false
    }
}
fn unknown() -> HostError {
    HostError::new(
        "not_found",
        "Source is not present in the verified source index. Refresh available sources.",
    )
}
fn incompatible() -> HostError {
    HostError::new(
        "incompatible_plugin",
        "This source needs a compatible update. Update the source or OpenKara.",
    )
}
fn binding(publisher: &str, package: &str) -> String {
    format!("{publisher}/{package}")
}
fn save(config: &HostConfig, state: &StoredState) -> HostResult<()> {
    fs::create_dir_all(&config.data_root)?;
    let path = config.data_root.join("state.json");
    let temp = config.data_root.join("state.json.tmp");
    fs::write(
        &temp,
        serde_json::to_vec(state)
            .map_err(|_| HostError::new("storage", "Cannot save source settings."))?,
    )?;
    fs::rename(temp, path)?;
    Ok(())
}
const INVALID_INDEX: &str =
    "Source index signature or release data is invalid. Keep the installed version and retry discovery later.";
const CACHED_OWNER_MISMATCH: &str =
    "Cached source identity conflicts with the installed publisher. Restore source settings from backup.";
const CHANGED_OWNER: &str =
    "Source publisher ownership changed. The existing installation and credentials remain protected.";
const WORK_APPROVAL_REQUIRED: &str =
    "This source has active sign-in, preview, or import work. Choose Stop Work to continue.";
const WORK_STOP_TIMEOUT: &str =
    "Source work has not stopped. Close the active sign-in, preview, or import, then retry.";
const MISSING_RUNTIME: &str =
    "Source runtime is missing. Reinstall this source to restore verified runtime files.";

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
fn compatible(config: &HostConfig, release: &Release) -> bool {
    semver::Version::parse(&config.host_version)
        .ok()
        .zip(semver::Version::parse(&release.manifest.minimum_host_version).ok())
        .is_some_and(|(host, min)| host >= min)
        && release
            .manifest
            .protocol
            .majors
            .iter()
            .any(|major| process::supported(config.current_protocol).contains(major))
        && release.runtime.targets.contains_key(&config.target)
        && match &config.runtime_policy {
            RuntimePolicy::Downloaded => true,
            RuntimePolicy::ChannelBundled {
                executable,
                version,
                target,
            } => {
                version == &release.runtime.version
                    && target == &config.target
                    && executable.is_file()
            }
        }
}
impl Inner {
    fn publish(&self, id: &str) {
        if let Ok(snapshot) = self.snapshot(id) {
            let _ = self.changes.send(snapshot);
        }
    }
    fn snapshot(&self, id: &str) -> HostResult<SourceSnapshot> {
        self.check()?;
        let state = self.state.lock().unwrap();
        snapshot(&self.config, &state, id)
    }
    fn gate(&self, id: &str, install: bool) -> Arc<tokio::sync::Mutex<()>> {
        let map = if install {
            &self.installs
        } else {
            &self.starts
        };
        map.lock().unwrap().entry(id.into()).or_default().clone()
    }
    fn check(&self) -> HostResult<()> {
        self.initialization_error.clone().map_or(Ok(()), Err)
    }
}
fn snapshot(config: &HostConfig, state: &State, id: &str) -> HostResult<SourceSnapshot> {
    let stored = state.stored.sources.get(id).ok_or_else(unknown)?;
    let live = state.sources.get(id);
    let available = state
        .index
        .as_ref()
        .and_then(|index| index.sources.iter().find(|source| source.source_id == id))
        .and_then(|source| {
            source
                .releases
                .iter()
                .filter(|release| !stored.revoked.contains(&release.manifest.version))
                .max_by_key(|release| semver::Version::parse(&release.manifest.version).ok())
        });
    let manifest = stored
        .active
        .as_ref()
        .or(available)
        .map(|release| release.manifest.clone());
    let invalid = stored.active.as_ref().is_some_and(|release| {
        stored.revoked.contains(&release.manifest.version) || !compatible(config, release)
    });
    let installing = live.is_some_and(|live| live.install.is_some());
    let active_work = live
        .map(|live| {
            live.work
                .iter()
                .map(|(id, (kind, cancel))| ActiveWork {
                    id: *id,
                    kind: kind.clone(),
                    cancelling: cancel.is_cancelled(),
                })
                .collect()
        })
        .unwrap_or_default();
    let status = if invalid {
        SourceState::UpdateRequired
    } else if live.is_some_and(|live| live.stopping) {
        SourceState::Cancelling
    } else if installing {
        SourceState::Installing
    } else if stored.staged.is_some() {
        SourceState::WaitingForIdle
    } else if stored.active.is_none() {
        SourceState::NotInstalled
    } else if !stored.enabled {
        SourceState::Disabled
    } else if live.is_some_and(|live| live.failures >= 4) {
        SourceState::Unavailable
    } else {
        SourceState::Ready
    };
    Ok(SourceSnapshot {
        source_id: id.into(),
        owner_binding: stored.owner_binding.clone(),
        name: manifest
            .as_ref()
            .map(|manifest| manifest.name.clone())
            .unwrap_or_else(|| id.into()),
        description: manifest
            .as_ref()
            .map(|manifest| manifest.description.clone())
            .unwrap_or_default(),
        license: manifest
            .as_ref()
            .map(|manifest| manifest.license.clone())
            .unwrap_or_default(),
        installed_version: stored.active.as_ref().map(|r| r.manifest.version.clone()),
        available_version: available.map(|r| r.manifest.version.clone()),
        staged_version: stored.staged.as_ref().map(|r| r.manifest.version.clone()),
        pinned_version: stored.pinned_version.clone(),
        enabled: stored.enabled,
        runtime_ready: stored.active.is_some() && !invalid,
        running: live
            .and_then(|live| live.process.as_ref())
            .is_some_and(|process| process.alive()),
        state: status,
        progress: live.and_then(|live| live.progress.clone()),
        last_error: live.and_then(|live| live.error.clone()),
        active_work,
        manifest,
    })
}
impl SourceHost {
    pub fn unavailable(config: HostConfig, error: HostError) -> Self {
        let (changes, _) = tokio::sync::broadcast::channel(1);
        Self {
            inner: Arc::new(Inner {
                config,
                state: Mutex::new(State {
                    stored: StoredState::default(),
                    index: None,
                    sources: BTreeMap::new(),
                    sequence: 0,
                }),
                changes,
                starts: Mutex::new(BTreeMap::new()),
                installs: Mutex::new(BTreeMap::new()),
                runtime_install: tokio::sync::Mutex::new(()),
                initialization_error: Some(error),
            }),
        }
    }
    pub async fn new(config: HostConfig) -> HostResult<Self> {
        fs::create_dir_all(&config.data_root)?;
        let mut stored: StoredState = match fs::read(config.data_root.join("state.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                HostError::new(
                    "storage",
                    "Source settings are invalid. Restore the application-data backup.",
                )
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoredState::default(),
            Err(error) => return Err(error.into()),
        };
        let (index, cache_error) = match fs::read(config.data_root.join("verified-index.json")) {
            Ok(bytes) => match verify_index(&config, &bytes) {
                Ok(index) => (Some(index), None),
                Err(error) => (None, Some(error)),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, None),
            Err(error) => (None, Some(HostError::from(error))),
        };
        if let Some(index) = &index {
            for source in &index.sources {
                let owner = binding(&source.publisher, &source.package_id);
                let entry = stored.sources.entry(source.source_id.clone()).or_default();
                if !entry.owner_binding.is_empty() && entry.owner_binding != owner {
                    return Err(HostError::new("owner_mismatch", CACHED_OWNER_MISMATCH));
                }
                entry.owner_binding = owner;
                for version in &source.revoked_versions {
                    if !entry.revoked.contains(version) {
                        entry.revoked.push(version.clone());
                    }
                }
            }
            stored.generation = stored.generation.max(index.generation);
            save(&config, &stored)?;
        }
        let (changes, _) = tokio::sync::broadcast::channel(128);
        let sources = stored
            .sources
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    Live {
                        error: cache_error.clone(),
                        ..Live::default()
                    },
                )
            })
            .collect();
        let host = Self {
            inner: Arc::new(Inner {
                config,
                state: Mutex::new(State {
                    stored,
                    index,
                    sources,
                    sequence: 0,
                }),
                changes,
                starts: Mutex::new(BTreeMap::new()),
                installs: Mutex::new(BTreeMap::new()),
                runtime_install: tokio::sync::Mutex::new(()),
                initialization_error: None,
            }),
        };
        let weak = Arc::downgrade(&host.inner);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(40)).await;
                let Some(inner) = weak.upgrade() else { return };
                let host = SourceHost { inner };
                host.maintain().await;
            }
        });
        Ok(host)
    }
    pub fn snapshots(&self) -> Vec<SourceSnapshot> {
        let state = self.inner.state.lock().unwrap();
        state
            .stored
            .sources
            .keys()
            .filter_map(|id| snapshot(&self.inner.config, &state, id).ok())
            .collect()
    }
    pub fn snapshot(&self, id: &str) -> HostResult<SourceSnapshot> {
        self.inner.snapshot(id)
    }
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SourceSnapshot> {
        self.inner.changes.subscribe()
    }
    pub fn description(&self, id: &str) -> HostResult<VerifiedSource> {
        let snapshot = self.snapshot(id)?;
        Ok(VerifiedSource {
            source_id: id.into(),
            owner_binding: snapshot.owner_binding,
            manifest: snapshot.manifest.ok_or_else(unknown)?,
        })
    }
    pub async fn refresh(&self) -> HostResult<Vec<SourceSnapshot>> {
        self.inner.check()?;
        let result: HostResult<Vec<SourceSnapshot>> = async {
            let config = &self.inner.config;
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
            {
                let mut state = self.inner.state.lock().unwrap();
                if index.generation < state.stored.generation {
                    return Err(HostError::new(
                        "untrusted_index",
                        "Source index is older than the last verified index. Retry later.",
                    ));
                }
                for source in &index.sources {
                    if state
                        .stored
                        .sources
                        .get(&source.source_id)
                        .is_some_and(|stored| {
                            stored.owner_binding != binding(&source.publisher, &source.package_id)
                        })
                    {
                        return Err(HostError::new("owner_mismatch", CHANGED_OWNER));
                    }
                }
                let mut stored = state.stored.clone();
                stored.generation = index.generation;
                for source in &index.sources {
                    let entry = stored.sources.entry(source.source_id.clone()).or_default();
                    entry.owner_binding = binding(&source.publisher, &source.package_id);
                    for version in &source.revoked_versions {
                        if !entry.revoked.contains(version) {
                            entry.revoked.push(version.clone());
                        }
                    }
                }
                state.stored = stored;
                state.index = Some(index);
                let revoked_ids: Vec<_> = state
                    .stored
                    .sources
                    .iter()
                    .filter(|(_, source)| {
                        source.active.as_ref().is_some_and(|release| {
                            source.revoked.contains(&release.manifest.version)
                        })
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in revoked_ids {
                    if let Some(live) = state.sources.get(&id) {
                        for (_, cancel) in live.work.values() {
                            cancel.cancel();
                        }
                    }
                }
                let temp = config.data_root.join("verified-index.json.tmp");
                let cached = (|| {
                    fs::write(&temp, &bytes)?;
                    fs::rename(temp, config.data_root.join("verified-index.json"))?;
                    Ok::<(), std::io::Error>(())
                })();
                let saved = save(config, &state.stored);
                cached?;
                saved?;
            }
            Ok(self.snapshots())
        }
        .await;
        self.maintain().await;
        if let Err(error) = &result {
            let mut state = self.inner.state.lock().unwrap();
            let ids: Vec<_> = state.stored.sources.keys().cloned().collect();
            for id in ids {
                state.sources.entry(id).or_default().error = Some(error.clone());
            }
        }
        for snapshot in self.snapshots() {
            let _ = self.inner.changes.send(snapshot);
        }
        result
    }
    fn progress(&self, id: &str, phase: &str, completed: u64, total: Option<u64>) {
        self.inner
            .state
            .lock()
            .unwrap()
            .sources
            .entry(id.into())
            .or_default()
            .progress = Some(InstallProgress {
            phase: phase.into(),
            completed,
            total,
        });
        self.inner.publish(id);
    }
    pub fn cancel_install(&self, id: &str) -> HostResult<()> {
        self.inner.check()?;
        let state = self.inner.state.lock().unwrap();
        let live = state.sources.get(id).ok_or_else(unknown)?;
        if let Some(cancel) = &live.install {
            cancel.cancel();
        }
        Ok(())
    }
    pub async fn install(&self, id: &str, version: Option<&str>) -> HostResult<SourceSnapshot> {
        self.inner.check()?;
        let gate = self.inner.gate(id, true);
        let _guard = gate
            .try_lock()
            .map_err(|_| HostError::new("busy", "A source installation is already active."))?;
        let selection: HostResult<(Release, Cancellation)> = (|| {
            let mut state = self.inner.state.lock().unwrap();
            let stored = state.stored.sources.get(id).ok_or_else(unknown)?;
            let wanted = version.or(stored.pinned_version.as_deref());
            let source = state
                .index
                .as_ref()
                .and_then(|index| index.sources.iter().find(|source| source.source_id == id))
                .ok_or_else(unknown)?;
            let release = source
                .releases
                .iter()
                .filter(|release| {
                    !stored.revoked.contains(&release.manifest.version)
                        && wanted.is_none_or(|wanted| wanted == release.manifest.version)
                        && compatible(&self.inner.config, release)
                })
                .max_by_key(|release| semver::Version::parse(&release.manifest.version).ok())
                .cloned()
                .ok_or_else(incompatible)?;
            let cancel = Cancellation::new();
            let live = state.sources.entry(id.into()).or_default();
            live.install = Some(cancel.clone());
            live.error = None;
            Ok((release, cancel))
        })();
        let (mut release, cancel) = match selection {
            Ok(selection) => selection,
            Err(error) => {
                self.inner
                    .state
                    .lock()
                    .unwrap()
                    .sources
                    .entry(id.into())
                    .or_default()
                    .error = Some(error.clone());
                self.inner.publish(id);
                return Err(error);
            }
        };
        let result = async {
            install::choose_repair_slots(&self.inner.config, &mut release)?;
            self.install_release(id, &release, &cancel).await
        }
        .await;
        {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.sources.entry(id.into()).or_default();
            live.install = None;
            live.progress = None;
            if let Err(error) = &result {
                live.error = Some(error.clone());
                live.failed_install = Some(release.manifest.version.clone());
            } else {
                live.failed_install = None;
            }
        }
        self.inner.publish(id);
        result?;
        self.snapshot(id)
    }
    async fn install_release(
        &self,
        id: &str,
        release: &Release,
        cancel: &Cancellation,
    ) -> HostResult<()> {
        let config = &self.inner.config;
        self.progress(id, "runtime", 0, None);
        let executable = {
            let _guard = self.inner.runtime_install.lock().await;
            install::runtime(config, release, cancel, |done| {
                self.progress(id, "runtime", done, None)
            })
            .await?
        };
        let package = install::package_path(config, release);
        if !package.exists() {
            self.progress(id, "package", 0, Some(release.archive.size));
            let staged = install::artifact(config, &release.archive, cancel, |done| {
                self.progress(id, "package", done, Some(release.archive.size))
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
        install::verify_seal(&package)?;
        install::validate_package(&package, release)?;
        cancel.check()?;
        self.progress(id, "handshake", 0, None);
        {
            let state = self.inner.state.lock().unwrap();
            let source = state.stored.sources.get(id).ok_or_else(unknown)?;
            if source.revoked.contains(&release.manifest.version) || !compatible(config, release) {
                return Err(incompatible());
            }
        }
        cancel.check()?;
        let candidate = tokio::select! {
            _ = cancel.cancelled() => return cancel.check(),
            candidate = Process::start(&executable, &package, release, config, |command| {
                let state = self.inner.state.lock().unwrap();
                let source = state.stored.sources.get(id).ok_or_else(unknown)?;
                cancel.check()?;
                if source.revoked.contains(&release.manifest.version) || !compatible(config, release) {
                    return Err(incompatible());
                }
                command.spawn().map_err(|_| {
                    HostError::new(
                        "source_unavailable",
                        "Source process cannot start. Retry or update this source.",
                    )
                })
            }) => candidate?,
        };
        candidate.stop().await;
        cancel.check()?;
        install::verify_installed(config, release)?;
        {
            let mut state = self.inner.state.lock().unwrap();
            let mut stored = state.stored.clone();
            let source = stored.sources.get_mut(id).ok_or_else(unknown)?;
            if source.revoked.contains(&release.manifest.version) {
                return Err(incompatible());
            }
            source.staged = Some(release.clone());
            save(config, &stored)?;
            state.stored = stored;
        }
        self.activate(id).await
    }
    async fn activate(&self, id: &str) -> HostResult<()> {
        let gate = self.inner.gate(id, false);
        let _guard = gate.lock().await;
        {
            let mut state = self.inner.state.lock().unwrap();
            let source = state.stored.sources.get(id).ok_or_else(unknown)?;
            if source.staged.as_ref().is_some_and(|release| {
                source.revoked.contains(&release.manifest.version)
                    || !compatible(&self.inner.config, release)
            }) {
                let mut stored = state.stored.clone();
                stored.sources.get_mut(id).ok_or_else(unknown)?.staged = None;
                save(&self.inner.config, &stored)?;
                state.stored = stored;
                return Err(incompatible());
            }
        }
        let staged = self
            .inner
            .state
            .lock()
            .unwrap()
            .stored
            .sources
            .get(id)
            .and_then(|source| source.staged.clone());
        if let Some(staged) = staged {
            if let Err(error) = install::verify_installed(&self.inner.config, &staged) {
                let mut state = self.inner.state.lock().unwrap();
                let mut stored = state.stored.clone();
                stored.sources.get_mut(id).ok_or_else(unknown)?.staged = None;
                let saved = save(&self.inner.config, &stored);
                state.stored = stored;
                saved?;
                return Err(error);
            }
        }
        let old = {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.sources.entry(id.into()).or_default();
            if !live.work.is_empty()
                || live.stopping
                || live
                    .process
                    .as_ref()
                    .is_some_and(|process| process.pending_count() > 0)
            {
                return Ok(());
            }
            live.stopping = true;
            live.process.take()
        };
        if let Some(old) = old {
            old.stop().await;
        }
        let result = {
            let mut state = self.inner.state.lock().unwrap();
            let mut stored = state.stored.clone();
            let source = stored.sources.get_mut(id).ok_or_else(unknown)?;
            let result = if let Some(release) = source.staged.take() {
                if source.revoked.contains(&release.manifest.version) {
                    Err(incompatible())
                } else {
                    source.active = Some(release);
                    save(&self.inner.config, &stored).map(|_| {
                        state.stored = stored;
                        let live = state.sources.entry(id.into()).or_default();
                        live.failures = 0;
                        live.error = None;
                    })
                }
            } else {
                Ok(())
            };
            state.sources.entry(id.into()).or_default().stopping = false;
            result
        };
        self.inner.publish(id);
        result
    }
    pub fn set_pin(&self, id: &str, version: Option<&str>) -> HostResult<SourceSnapshot> {
        self.inner.check()?;
        let mut state = self.inner.state.lock().unwrap();
        let mut stored = state.stored.clone();
        let source = stored.sources.get_mut(id).ok_or_else(unknown)?;
        if version.is_some_and(|version| {
            source
                .active
                .as_ref()
                .is_none_or(|active| active.manifest.version != version)
        }) {
            return Err(HostError::new(
                "invalid_request",
                "Only the installed version can be pinned.",
            ));
        }
        source.pinned_version = version.map(str::to_string);
        save(&self.inner.config, &stored)?;
        state.stored = stored;
        drop(state);
        self.inner.publish(id);
        self.snapshot(id)
    }
    pub fn acquire_work(&self, id: &str, kind: WorkKind) -> HostResult<WorkLease> {
        self.inner.check()?;
        let mut state = self.inner.state.lock().unwrap();
        let source = state.stored.sources.get(id).ok_or_else(unknown)?;
        if !source.enabled {
            return Err(HostError::new(
                "source_disabled",
                "Enable the source before starting work.",
            ));
        }
        if source.active.as_ref().is_none_or(|release| {
            source.revoked.contains(&release.manifest.version)
                || !compatible(&self.inner.config, release)
        }) {
            return Err(incompatible());
        }
        state.sequence += 1;
        let sequence = state.sequence;
        let live = state.sources.entry(id.into()).or_default();
        if live.stopping {
            return Err(HostError::new("busy", "Source work is stopping."));
        }
        let cancel = Cancellation::new();
        live.work.insert(sequence, (kind, cancel.clone()));
        drop(state);
        self.inner.publish(id);
        Ok(WorkLease {
            inner: Arc::downgrade(&self.inner),
            source_id: id.into(),
            id: sequence,
            cancellation: cancel,
        })
    }
    async fn stop_work(&self, id: &str, approved: bool) -> HostResult<()> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.sources.entry(id.into()).or_default();
            if !live.work.is_empty() && !approved {
                return Err(HostError::new("active_work", WORK_APPROVAL_REQUIRED));
            }
            live.stopping = true;
            for (_, cancel) in live.work.values() {
                cancel.cancel();
            }
        }
        self.inner.publish(id);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self
                .inner
                .state
                .lock()
                .unwrap()
                .sources
                .get(id)
                .is_none_or(|live| live.work.is_empty())
            {
                break;
            }
            if Instant::now() >= deadline {
                self.inner
                    .state
                    .lock()
                    .unwrap()
                    .sources
                    .entry(id.into())
                    .or_default()
                    .stopping = false;
                self.inner.publish(id);
                return Err(HostError::new("active_work", WORK_STOP_TIMEOUT));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let gate = self.inner.gate(id, false);
        let _guard = gate.lock().await;
        let process = self
            .inner
            .state
            .lock()
            .unwrap()
            .sources
            .entry(id.into())
            .or_default()
            .process
            .take();
        if let Some(process) = process {
            process.stop().await;
        }
        Ok(())
    }
    pub async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
        stop_work: bool,
    ) -> HostResult<SourceSnapshot> {
        self.snapshot(id)?;
        let gate = self.inner.gate(id, true);
        let _guard = gate.lock().await;
        if enabled {
            let snapshot = self.snapshot(id)?;
            if snapshot.installed_version.is_none() || snapshot.state == SourceState::UpdateRequired
            {
                return Err(incompatible());
            }
        } else {
            self.stop_work(id, stop_work).await?;
        }
        {
            let mut state = self.inner.state.lock().unwrap();
            let mut stored = state.stored.clone();
            stored.sources.get_mut(id).ok_or_else(unknown)?.enabled = enabled;
            let result = save(&self.inner.config, &stored);
            state.sources.entry(id.into()).or_default().stopping = false;
            result?;
            state.stored = stored;
        }
        self.inner.publish(id);
        self.snapshot(id)
    }
    pub async fn uninstall(&self, id: &str, stop_work: bool) -> HostResult<SourceSnapshot> {
        self.snapshot(id)?;
        let gate = self.inner.gate(id, true);
        let _guard = gate.lock().await;
        self.stop_work(id, stop_work).await?;
        {
            let mut state = self.inner.state.lock().unwrap();
            let mut stored = state.stored.clone();
            let source = stored.sources.get_mut(id).ok_or_else(unknown)?;
            source.enabled = false;
            source.active = None;
            source.staged = None;
            source.pinned_version = None;
            let result = save(&self.inner.config, &stored);
            state.sources.entry(id.into()).or_default().stopping = false;
            result?;
            state.stored = stored;
        }
        self.inner.publish(id);
        self.snapshot(id)
    }
    async fn running(&self, id: &str) -> HostResult<Arc<Process>> {
        let gate = self.inner.gate(id, false);
        let _guard = gate.lock().await;
        self.running_locked(id).await
    }
    async fn running_locked(&self, id: &str) -> HostResult<Arc<Process>> {
        let release = {
            let mut state = self.inner.state.lock().unwrap();
            let stored = state.stored.sources.get(id).ok_or_else(unknown)?;
            if !stored.enabled {
                return Err(HostError::new(
                    "source_disabled",
                    "Enable this source before using it.",
                ));
            }
            let release = stored.active.clone().ok_or_else(incompatible)?;
            if stored.revoked.contains(&release.manifest.version)
                || !compatible(&self.inner.config, &release)
            {
                return Err(incompatible());
            }
            let live = state.sources.entry(id.into()).or_default();
            if live.stopping {
                return Err(HostError::new("busy", "Source work is stopping."));
            }
            if let Some(process) = &live.process {
                if process.alive() {
                    return Ok(process.clone());
                }
                live.failures += 1;
                live.retry_at = Some(
                    Instant::now()
                        + self.inner.config.backoff_unit
                            * 2u32.pow(live.failures.saturating_sub(1).min(2)),
                );
                live.process = None;
            }
            if live.failures >= 4 {
                return Err(HostError::new(
                    "source_unavailable",
                    "Source repeatedly failed to start. Choose Retry or install an update.",
                ));
            }
            release
        };
        let retry_at = self
            .inner
            .state
            .lock()
            .unwrap()
            .sources
            .get(id)
            .and_then(|live| live.retry_at);
        if let Some(retry_at) = retry_at {
            tokio::time::sleep(retry_at.saturating_duration_since(Instant::now())).await;
        }
        let result = async {
            let package = install::package_path(&self.inner.config, &release);
            install::verify_seal(&package)?;
            install::validate_package(&package, &release)?;
            if let RuntimePolicy::Downloaded = &self.inner.config.runtime_policy {
                if !install::runtime_path(&self.inner.config, &release)?.exists() {
                    return Err(HostError::new("source_unavailable", MISSING_RUNTIME));
                }
            }
            let executable = {
                let _guard = self.inner.runtime_install.lock().await;
                install::runtime(&self.inner.config, &release, &Cancellation::new(), |_| {}).await?
            };
            Process::start(
                &executable,
                &package,
                &release,
                &self.inner.config,
                |command| {
                    let state = self.inner.state.lock().unwrap();
                    let source = state.stored.sources.get(id).ok_or_else(unknown)?;
                    if !source.enabled || source.revoked.contains(&release.manifest.version) {
                        return Err(incompatible());
                    }
                    command.spawn().map_err(|_| {
                        HostError::new(
                            "source_unavailable",
                            "Source process cannot start. Retry or update this source.",
                        )
                    })
                },
            )
            .await
        }
        .await;
        let still_allowed = {
            let state = self.inner.state.lock().unwrap();
            state.stored.sources.get(id).is_some_and(|source| {
                source.enabled
                    && source.active.as_ref().is_some_and(|active| {
                        active.archive.sha256 == release.archive.sha256
                            && !source.revoked.contains(&active.manifest.version)
                    })
            }) && state.sources.get(id).is_none_or(|live| !live.stopping)
        };
        if !still_allowed {
            if let Ok(process) = &result {
                process.stop().await;
            }
            return Err(HostError::new(
                "source_disabled",
                "Source was disabled or replaced while starting. Retry only after enabling it.",
            ));
        }
        {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.sources.entry(id.into()).or_default();
            match &result {
                Ok(process) => {
                    live.process = Some(process.clone());
                    live.error = None
                }
                Err(error) => {
                    live.failures += 1;
                    live.retry_at = Some(
                        Instant::now()
                            + self.inner.config.backoff_unit
                                * 2u32.pow(live.failures.saturating_sub(1).min(2)),
                    );
                    live.error = Some(error.clone());
                }
            }
        }
        self.inner.publish(id);
        result
    }
    pub async fn start_request(
        &self,
        id: &str,
        method: &str,
        params: Value,
    ) -> HostResult<RequestHandle> {
        if method == "handshake" || method == "request.cancel" {
            return Err(HostError::new(
                "invalid_request",
                "The host owns protocol control requests.",
            ));
        }
        let description = self.description(id)?;
        let capability = match method {
            "search" => Some("search"),
            "liked_tracks" => Some("liked_tracks"),
            "playlists" => Some("playlists"),
            "playlist.tracks" => Some("playlist_tracks"),
            "tracks.get" => Some("track_details"),
            "media.resolve" => Some(if params["purpose"] == "preview" {
                "preview"
            } else {
                "import"
            }),
            "describe" | "session.restore" | "auth.start" | "auth.poll" | "auth.complete"
            | "auth.cancel" | "auth.sign_out" => None,
            _ => {
                return Err(HostError::new(
                    "unsupported_operation",
                    "This source operation is not supported.",
                ))
            }
        };
        if capability.is_some_and(|capability| {
            !description
                .manifest
                .capabilities
                .iter()
                .any(|value| value == capability)
        }) {
            return Err(HostError::new(
                "unsupported_operation",
                "This source does not provide this capability.",
            ));
        }
        let gate = self.inner.gate(id, false);
        let _guard = gate.lock().await;
        self.running_locked(id)
            .await?
            .send(
                method,
                params,
                matches!(method, "auth.cancel" | "auth.sign_out"),
            )
            .await
    }
    pub async fn request(&self, id: &str, method: &str, params: Value) -> HostResult<Value> {
        self.start_request(id, method, params).await?.result().await
    }
    pub async fn retry(&self, id: &str) -> HostResult<SourceSnapshot> {
        let snapshot = self.snapshot(id)?;
        let failed = {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.sources.entry(id.into()).or_default();
            live.failures = 0;
            live.retry_at = None;
            live.error = None;
            live.failed_install.clone()
        };
        if let Some(version) = failed {
            return self.install(id, Some(&version)).await;
        }
        if snapshot.installed_version.is_none() {
            return self.install(id, None).await;
        }
        if snapshot.enabled {
            self.running(id).await?;
        }
        self.snapshot(id)
    }
    async fn maintain(&self) {
        let ids = {
            let state = self.inner.state.lock().unwrap();
            let ids: Vec<_> = state.stored.sources.keys().cloned().collect();
            for id in &ids {
                let source = &state.stored.sources[id];
                let revoked = source.active.as_ref().is_some_and(|release| {
                    source.revoked.contains(&release.manifest.version)
                        || !compatible(&self.inner.config, release)
                });
                if revoked {
                    if let Some(live) = state.sources.get(id) {
                        for (_, cancel) in live.work.values() {
                            cancel.cancel();
                        }
                    }
                }
            }
            ids
        };
        for id in ids {
            let gate = self.inner.gate(&id, false);
            let Ok(guard) = gate.try_lock() else { continue };
            let (process, activate, changed) = {
                let mut state = self.inner.state.lock().unwrap();
                let source = &state.stored.sources[&id];
                let revoked = source.active.as_ref().is_some_and(|release| {
                    source.revoked.contains(&release.manifest.version)
                        || !compatible(&self.inner.config, release)
                });
                let staged = source.staged.is_some();
                let live = state.sources.entry(id.clone()).or_default();
                let mut changed = false;
                if live
                    .process
                    .as_ref()
                    .is_some_and(|process| !process.alive())
                {
                    live.process = None;
                    live.failures += 1;
                    live.retry_at = Some(
                        Instant::now()
                            + self.inner.config.backoff_unit
                                * 2u32.pow(live.failures.saturating_sub(1).min(2)),
                    );
                    live.error = Some(HostError::new(
                        "source_unavailable",
                        "Source process stopped. Retry or install a source update.",
                    ));
                    changed = true;
                }
                let idle = live.work.is_empty()
                    && !live.stopping
                    && live
                        .process
                        .as_ref()
                        .is_none_or(|process| process.pending_count() == 0);
                let stop = idle
                    && (revoked
                        || live.process.as_ref().is_some_and(|process| {
                            process.idle_for() >= self.inner.config.idle_timeout
                        }));
                let process = if stop { live.process.take() } else { None };
                (process, staged && idle && live.install.is_none(), changed)
            };
            let stopped = process.is_some();
            if let Some(process) = process {
                process.stop().await;
            }
            drop(guard);
            if activate {
                if let Err(error) = self.activate(&id).await {
                    self.inner
                        .state
                        .lock()
                        .unwrap()
                        .sources
                        .entry(id.clone())
                        .or_default()
                        .error = Some(error);
                }
            }
            if stopped || activate || changed {
                self.inner.publish(&id);
            }
        }
    }
    pub async fn download_media(
        &self,
        id: &str,
        descriptor: ResourceDescriptor,
        destination: PathBuf,
        cancellation: Cancellation,
    ) -> HostResult<()> {
        use std::io::Write;
        let mut response = self
            .open_media(id, &descriptor, None, cancellation.clone())
            .await?;
        if !(200..300).contains(&response.status) {
            return Err(HostError::new(
                if matches!(response.status, 403 | 410) {
                    "media_expired"
                } else {
                    "network"
                },
                "Source media download failed. Resolve a fresh descriptor or retry.",
            ));
        }
        let temp = destination.with_extension(format!("{}.download", uuid::Uuid::new_v4()));
        let result: HostResult<()> = async {
            let mut file = fs::File::create(&temp)?;
            let mut size = 0u64;
            while let Some(chunk) = response.chunk().await? {
                size += chunk.len() as u64;
                if size > 4 * 1024 * 1024 * 1024 {
                    return Err(HostError::new(
                        "invalid_resource",
                        "Source media exceeds the download limit.",
                    ));
                }
                file.write_all(&chunk)?;
            }
            cancellation.check()?;
            file.sync_all()?;
            fs::rename(&temp, &destination)?;
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
    pub async fn open_media(
        &self,
        id: &str,
        descriptor: &ResourceDescriptor,
        range: Option<(u64, Option<u64>)>,
        cancellation: Cancellation,
    ) -> HostResult<MediaResponse> {
        let description = self.description(id)?;
        self.ensure_download(id)?;
        network::check_expiry(descriptor)?;
        #[cfg(test)]
        if let Some(response) = network::fixture_media(
            &self.inner.config,
            descriptor,
            &description.manifest.media_origins,
            range,
            cancellation.clone(),
        ) {
            return response;
        }
        let response = network::response_range(
            &descriptor.url,
            &descriptor.headers,
            Some(&description.manifest.media_origins),
            false,
            &cancellation,
            range,
        )
        .await?;
        Ok(MediaResponse::new(response, cancellation))
    }
    fn ensure_download(&self, id: &str) -> HostResult<()> {
        let snapshot = self.snapshot(id)?;
        if !snapshot.enabled {
            return Err(HostError::new("source_disabled", "Source is disabled."));
        }
        if snapshot.state == SourceState::UpdateRequired {
            return Err(incompatible());
        }
        Ok(())
    }
    pub async fn load_resource(
        &self,
        id: &str,
        descriptor: ResourceDescriptor,
        cancellation: Cancellation,
    ) -> HostResult<Vec<u8>> {
        let description = self.description(id)?;
        self.ensure_download(id)?;
        let cancel = cancellation;
        cancel.check()?;
        network::check_expiry(&descriptor)?;
        #[cfg(test)]
        if let Some(response) = network::fixture_media(
            &self.inner.config,
            &descriptor,
            &description.manifest.resource_origins,
            None,
            cancel.clone(),
        ) {
            let mut response = response?;
            if !(200..300).contains(&response.status) {
                return Err(HostError::new(
                    "network",
                    "Source artwork is unavailable. Retry later.",
                ));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
                    return Err(HostError::new(
                        "invalid_resource",
                        "Source artwork exceeds the download limit.",
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            return Ok(bytes);
        }
        let response = network::response(
            &descriptor.url,
            &descriptor.headers,
            Some(&description.manifest.resource_origins),
            false,
            &cancel,
        )
        .await?;
        network::read_bounded(response, 16 * 1024 * 1024, &cancel).await
    }
}
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
