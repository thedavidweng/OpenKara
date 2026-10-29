#[cfg(test)]
pub(crate) mod describe;
mod discovery;
mod engine;
mod install;
mod lifecycle;
mod market;
mod media;
mod migration;
mod network;
mod registry;
mod runtime;
mod snapshot;
mod supervisor;
mod types;
mod work;

pub use network::MediaResponse;
pub use runtime::RequestHandle;
pub use types::*;
pub use work::WorkLease;

use runtime::SourceInstance;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

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
    registry_refresh: tokio::sync::Mutex<()>,
    initialization_error: Option<HostError>,
}
struct State {
    stored: StoredState,
    registries: Vec<registry::RegistryEntry>,
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
    start_failures: u32,
    stopping: bool,
    failed_install: Option<String>,
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
        "This source is not installed. Install it from an added Source Registry.",
    )
}
fn incompatible() -> HostError {
    HostError::new(
        "incompatible_plugin",
        "This source needs a compatible update. Update the source or OpenKara.",
    )
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
impl Inner {
    fn open(
        config: HostConfig,
        state: State,
        changes: tokio::sync::broadcast::Sender<SourceSnapshot>,
        initialization_error: Option<HostError>,
    ) -> Arc<Self> {
        Arc::new(Self {
            config,
            state: Mutex::new(state),
            changes,
            starts: Mutex::new(BTreeMap::new()),
            installs: Mutex::new(BTreeMap::new()),
            registry_refresh: tokio::sync::Mutex::new(()),
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
            && engine::supports(release)
    }
    fn verified_package(&self, release: &Release) -> HostResult<PathBuf> {
        registry::verified_package(&self.config, release)
    }
}
impl SourceHost {
    pub fn unavailable(config: HostConfig, error: HostError) -> Self {
        let (changes, _) = tokio::sync::broadcast::channel(1);
        Self {
            inner: Inner::open(
                config,
                State {
                    stored: StoredState::default(),
                    registries: Vec::new(),
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
        if migration::retire_node_installs(&mut stored) {
            save(&config, &stored)?;
        }
        migration::remove_node_files(&config);
        let registries = registry::restore(&config, &stored.registries)?;
        let (changes, _) = tokio::sync::broadcast::channel(128);
        let sources = stored
            .sources
            .keys()
            .map(|id| (id.clone(), Live::default()))
            .collect();
        let host = Self {
            inner: Inner::open(
                config,
                State {
                    stored,
                    registries,
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
        let weak = Arc::downgrade(&host.inner);
        let poll = host
            .inner
            .config
            .registry_refresh_interval
            .min(Duration::from_secs(60));
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(poll).await;
                let Some(inner) = weak.upgrade() else { return };
                SourceHost { inner }.refresh_due_registries().await;
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
mod media_tests;
#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod registry_tests;
#[cfg(test)]
pub(crate) mod test_support;
