mod discovery;
mod distribution;
mod engine;
mod install;
mod lifecycle;
mod media;
mod network;
mod process;
mod runtime;
mod snapshot;
mod supervisor;
mod types;
mod work;

pub use network::MediaResponse;
pub use runtime::RequestHandle;
pub use types::*;
pub use work::WorkLease;

use distribution::{SignedIndexDistribution, SourceDistribution};
use runtime::{Runtimes, SourceInstance, SourceRuntime};
use std::{
    collections::BTreeMap,
    fs,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct SourceHost {
    inner: Arc<Inner>,
}
struct Inner {
    config: HostConfig,
    distribution: Box<dyn SourceDistribution>,
    runtime: Box<dyn SourceRuntime>,
    state: Mutex<State>,
    changes: tokio::sync::broadcast::Sender<SourceSnapshot>,
    starts: Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>,
    installs: Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>,
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
    instance: Option<Arc<dyn SourceInstance>>,
    progress: Option<InstallProgress>,
    error: Option<HostError>,
    install: Option<Cancellation>,
    work: BTreeMap<u64, (WorkKind, Cancellation)>,
    failures: u32,
    retry_at: Option<Instant>,
    stopping: bool,
    failed_install: Option<String>,
}
impl Live {
    fn schedule_retry(&mut self, backoff_unit: Duration) {
        self.failures += 1;
        self.retry_at =
            Some(Instant::now() + backoff_unit * 2u32.pow(self.failures.saturating_sub(1).min(2)));
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
const CACHED_OWNER_MISMATCH: &str =
    "Cached source identity conflicts with the installed publisher. Restore source settings from backup.";

impl Inner {
    fn open(
        config: HostConfig,
        distribution: Box<dyn SourceDistribution>,
        state: State,
        changes: tokio::sync::broadcast::Sender<SourceSnapshot>,
        initialization_error: Option<HostError>,
    ) -> Arc<Self> {
        Arc::new(Self {
            config,
            distribution,
            runtime: Box::new(Runtimes::default()),
            state: Mutex::new(state),
            changes,
            starts: Mutex::new(BTreeMap::new()),
            installs: Mutex::new(BTreeMap::new()),
            initialization_error,
        })
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
    fn compatible(&self, release: &Release) -> bool {
        semver::Version::parse(&self.config.host_version)
            .ok()
            .zip(semver::Version::parse(&release.manifest.minimum_host_version).ok())
            .is_some_and(|(host, min)| host >= min)
            && self.runtime.supports(&self.config, release)
    }
    fn blocked(&self, source: &StoredSource, release: &Release) -> bool {
        source.revoked.contains(&release.manifest.version) || !self.compatible(release)
    }
    fn verify_installed(&self, release: &Release) -> HostResult<()> {
        self.distribution.verified_package(&self.config, release)?;
        self.runtime.verify_installed(&self.config, release)
    }
}
impl SourceHost {
    pub fn unavailable(config: HostConfig, error: HostError) -> Self {
        let (changes, _) = tokio::sync::broadcast::channel(1);
        Self {
            inner: Inner::open(
                config,
                Box::new(SignedIndexDistribution),
                State {
                    stored: StoredState::default(),
                    index: None,
                    sources: BTreeMap::new(),
                    sequence: 0,
                },
                changes,
                Some(error),
            ),
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
        let distribution: Box<dyn SourceDistribution> = Box::new(SignedIndexDistribution);
        let (index, cache_error) = distribution.cached_index(&config);
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
            inner: Inner::open(
                config,
                distribution,
                State {
                    stored,
                    index,
                    sources,
                    sequence: 0,
                },
                changes,
                None,
            ),
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
}
#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod host_api_tests;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
