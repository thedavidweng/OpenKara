use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub type HostResult<T> = Result<T, HostError>;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}
impl HostError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: matches!(code, "network" | "source_unavailable" | "busy"),
        }
    }
}
impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for HostError {}
impl From<std::io::Error> for HostError {
    fn from(_: std::io::Error) -> Self {
        Self::new("storage", "Cannot read or write source files. Check available storage and permissions, then retry.")
    }
}

#[derive(Clone, Default)]
pub struct Cancellation {
    inner: Arc<CancelInner>,
}
#[derive(Default)]
struct CancelInner {
    cancelled: AtomicBool,
    changed: tokio::sync::Notify,
}
impl Cancellation {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Release);
        self.inner.changed.notify_waiters();
    }
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }
    pub async fn cancelled(&self) {
        loop {
            let changed = self.inner.changed.notified();
            if self.is_cancelled() {
                return;
            }
            changed.await;
        }
    }
    pub(crate) fn check(&self) -> HostResult<()> {
        if self.is_cancelled() {
            Err(HostError::new(
                "cancelled",
                "Source operation cancelled. Retry when ready.",
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone)]
pub struct HostConfig {
    pub data_root: PathBuf,
    pub resource_root: PathBuf,
    pub host_version: String,
    pub(crate) idle_timeout: Duration,
    pub(crate) describe_timeout: Duration,
    pub(crate) operation_timeout: Duration,
    pub(crate) engine_memory_limit: usize,
    pub(crate) registry_refresh_interval: Duration,
    #[cfg(test)]
    pub(crate) allow_test_distribution_http: bool,
    #[cfg(test)]
    pub(crate) fixture_resources: BTreeMap<String, (Vec<std::net::IpAddr>, Vec<u8>)>,
    #[cfg(test)]
    pub(crate) fixture_statuses: BTreeMap<String, u16>,
    #[cfg(test)]
    pub(crate) fixture_upstreams: BTreeMap<String, String>,
}
impl HostConfig {
    pub fn production(data_root: PathBuf, resource_root: PathBuf, host_version: String) -> Self {
        Self {
            data_root: data_root.join("streaming-sources"),
            resource_root,
            host_version,
            idle_timeout: Duration::from_secs(60),
            describe_timeout: Duration::from_secs(10),
            operation_timeout: Duration::from_secs(30),
            engine_memory_limit: 64 * 1024 * 1024,
            registry_refresh_interval: Duration::from_secs(6 * 60 * 60),
            #[cfg(test)]
            allow_test_distribution_http: false,
            #[cfg(test)]
            fixture_resources: BTreeMap::new(),
            #[cfg(test)]
            fixture_statuses: BTreeMap::new(),
            #[cfg(test)]
            fixture_upstreams: BTreeMap::new(),
        }
    }
    #[cfg(test)]
    pub(crate) fn fixture(data_root: PathBuf, host_version: String) -> Self {
        let mut config = Self::production(data_root.clone(), data_root, host_version);
        config.allow_test_distribution_http = true;
        config
    }
    #[cfg(test)]
    pub(crate) fn add_fixture_resource(
        &mut self,
        url: String,
        addresses: Vec<std::net::IpAddr>,
        bytes: Vec<u8>,
    ) {
        self.fixture_resources.insert(url, (addresses, bytes));
    }
    #[cfg(test)]
    pub(crate) fn set_fixture_status(&mut self, url: String, status: u16) {
        self.fixture_statuses.insert(url, status);
    }
    /// Serves source requests for a declared HTTPS origin from a local plain-HTTP server.
    #[cfg(test)]
    pub(crate) fn add_fixture_upstream(&mut self, origin: String, server: String) {
        self.fixture_upstreams.insert(origin, server);
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolDeclaration {
    pub majors: Vec<u32>,
    pub minor: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeRequirement {
    pub kind: String,
    pub major: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceManifest {
    pub manifest_version: u32,
    pub source_id: String,
    pub package_id: String,
    pub publisher: String,
    pub version: String,
    pub minimum_host_version: String,
    pub protocol: ProtocolDeclaration,
    pub runtime: RuntimeRequirement,
    pub entry: String,
    pub name: String,
    pub description: String,
    pub license: String,
    pub capabilities: Vec<String>,
    pub authentication: Value,
    pub requires_authentication: Vec<String>,
    pub resource_origins: Vec<String>,
    pub media_origins: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedSource {
    pub source_id: String,
    pub owner_binding: String,
    pub manifest: SourceManifest,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    NotInstalled,
    Installing,
    Ready,
    Disabled,
    UpdateRequired,
    Unavailable,
    WaitingForIdle,
    Cancelling,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallProgress {
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Authentication,
    Preview,
    Import,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWork {
    pub id: u64,
    pub kind: WorkKind,
    pub cancelling: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSnapshot {
    pub source_id: String,
    pub owner_binding: String,
    /// The Source Registry and package that own this source, when the source
    /// came from a registry.
    pub registry_id: Option<String>,
    pub package_id: Option<String>,
    pub name: String,
    pub description: String,
    pub license: String,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    pub staged_version: Option<String>,
    pub pinned_version: Option<String>,
    pub enabled: bool,
    /// The source has a verified active release that this host can run.
    pub loaded: bool,
    pub running: bool,
    pub state: SourceState,
    pub progress: Option<InstallProgress>,
    pub last_error: Option<HostError>,
    pub active_work: Vec<ActiveWork>,
    pub manifest: Option<SourceManifest>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDescriptor {
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Artifact {
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub format: String,
}
/// An installed package version. Registry packages carry their identity;
/// a release without one predates Source Registries and can no longer run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Release {
    pub manifest: SourceManifest,
    pub archive: Artifact,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<PackageIdentity>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct StoredSource {
    pub owner_binding: String,
    pub enabled: bool,
    pub pinned_version: Option<String>,
    pub active: Option<Release>,
    pub staged: Option<Release>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<PackageIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired: Option<RetiredInstall>,
}
/// A Node-era installation that the embedded engine cannot run. Its files are
/// deleted; the source id waits for a replacement from a Source Registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RetiredInstall {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub credentials_removed: bool,
    #[serde(default)]
    pub notice_dismissed: bool,
}
/// A source that the one-time replacement notice names.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetiredSource {
    pub source_id: String,
    pub name: String,
    pub version: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct StoredState {
    pub sources: BTreeMap<String, StoredSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registries: Vec<StoredRegistry>,
}
/// An installed package's identity: the Source Registry that lists it and its package id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PackageIdentity {
    pub registry_id: String,
    pub package_id: String,
}
impl PackageIdentity {
    /// The prefix keeps registry identities apart from the `publisher/package`
    /// bindings that Node-era installs left in stored state and credentials.
    pub(crate) fn owner_binding(&self) -> String {
        format!("registry:{}/{}", self.registry_id, self.package_id)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredRegistry {
    pub url: String,
    pub registry_id: String,
}
/// One package version as the market lists it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketVersion {
    pub version: String,
    pub size: u64,
    pub resource_origins: Vec<String>,
    pub media_origins: Vec<String>,
    pub capabilities: Vec<String>,
}
/// The install state of a market package on this device.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketInstall {
    pub version: String,
    pub pinned_version: Option<String>,
}
/// An update the market offers for an installed package, with the origins and
/// capabilities it adds. `requires_consent` is true when it adds any.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketUpdate {
    pub version: String,
    pub added_origins: Vec<String>,
    pub added_capabilities: Vec<String>,
    pub requires_consent: bool,
}
/// One package that an added Source Registry lists on the market.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketPackage {
    pub registry_id: String,
    pub package_id: String,
    pub source_id: String,
    pub name: String,
    pub description: String,
    pub license: String,
    pub versions: Vec<MarketVersion>,
    pub installed: Option<MarketInstall>,
    pub update: Option<MarketUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryStatus {
    pub url: String,
    pub registry_id: String,
    pub display_name: String,
    pub homepage: String,
    /// Unix time in milliseconds of the last successful fetch.
    pub refreshed_at: Option<u64>,
    pub last_error: Option<HostError>,
}
