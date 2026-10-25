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

/// The Node version that the Flatpak bundles from `packaging/source-runtime.json`.
pub(crate) const BUNDLED_RUNTIME_VERSION: &str = "24.22.1";

#[derive(Clone)]
pub enum RuntimePolicy {
    Downloaded,
    ChannelBundled {
        executable: PathBuf,
        version: String,
        target: String,
    },
}
#[derive(Clone)]
pub struct HostConfig {
    pub data_root: PathBuf,
    pub resource_root: PathBuf,
    pub host_version: String,
    pub runtime_policy: RuntimePolicy,
    pub(crate) index_url: String,
    pub(crate) trusted_keys: BTreeMap<String, [u8; 32]>,
    pub(crate) target: String,
    pub(crate) current_protocol: u32,
    pub(crate) idle_timeout: Duration,
    pub(crate) operation_timeout: Duration,
    pub(crate) backoff_unit: Duration,
    #[cfg(test)]
    pub(crate) allow_test_distribution_http: bool,
    #[cfg(test)]
    pub(crate) fixture_resources: BTreeMap<String, (Vec<std::net::IpAddr>, Vec<u8>)>,
    #[cfg(test)]
    pub(crate) fixture_statuses: BTreeMap<String, u16>,
}
impl HostConfig {
    pub fn production(data_root: PathBuf, resource_root: PathBuf, host_version: String) -> Self {
        use base64::Engine;
        let target = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        let runtime_policy = if std::env::var_os("FLATPAK_ID").is_some() {
            RuntimePolicy::ChannelBundled {
                executable: PathBuf::from("/app/bin/openkara-source-node"),
                version: BUNDLED_RUNTIME_VERSION.into(),
                target: target.clone(),
            }
        } else {
            RuntimePolicy::Downloaded
        };
        let trusted_keys = [
            (
                "0374d430d3cf1aa6",
                "SGkXN6xbau63VKXOSU8kGVJtMN0DhxFDmxlFvelqUXU=",
            ),
            (
                "71a135edb0fedcf0",
                "VfvOxNOgYgIbKkzMDr8cLgqKIUbH2H9QcsSlc8qc28w=",
            ),
        ]
        .into_iter()
        .map(|(id, key)| {
            (
                id.into(),
                base64::engine::general_purpose::STANDARD
                    .decode(key)
                    .expect("embedded source public key")
                    .try_into()
                    .expect("32-byte source public key"),
            )
        })
        .collect();
        Self {
            data_root: data_root.join("streaming-sources"),
            resource_root,
            host_version,
            runtime_policy,
            index_url: "https://github.com/thedavidweng/openkara-sources/releases/latest/download/source-index.json".into(),
            trusted_keys,
            target,
            current_protocol: 1,
            idle_timeout: Duration::from_secs(60),
            operation_timeout: Duration::from_secs(30),
            backoff_unit: Duration::from_secs(1),
            #[cfg(test)]
            allow_test_distribution_http: false,
            #[cfg(test)]
            fixture_resources: BTreeMap::new(),
            #[cfg(test)]
            fixture_statuses: BTreeMap::new(),
        }
    }
    #[cfg(test)]
    pub(crate) fn fixture(
        data_root: PathBuf,
        host_version: String,
        index_url: String,
        trusted_keys: BTreeMap<String, [u8; 32]>,
        runtime_policy: RuntimePolicy,
    ) -> Self {
        let mut config = Self::production(data_root.clone(), data_root, host_version);
        config.index_url = index_url;
        config.trusted_keys = trusted_keys;
        config.runtime_policy = runtime_policy;
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
    pub name: String,
    pub description: String,
    pub license: String,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    pub staged_version: Option<String>,
    pub pinned_version: Option<String>,
    pub enabled: bool,
    pub runtime_ready: bool,
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
    #[serde(default)]
    pub executable: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RuntimeRelease {
    pub version: String,
    pub targets: BTreeMap<String, Artifact>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Release {
    pub manifest: SourceManifest,
    pub archive: Artifact,
    pub runtime: RuntimeRelease,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_runtime: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IndexSource {
    pub source_id: String,
    pub publisher: String,
    pub package_id: String,
    pub releases: Vec<Release>,
    pub revoked_versions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SourceIndex {
    pub schema_version: u32,
    pub generation: u64,
    pub sources: Vec<IndexSource>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SignedIndex {
    pub key_id: String,
    pub payload: String,
    pub signature: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct StoredSource {
    pub owner_binding: String,
    pub enabled: bool,
    pub pinned_version: Option<String>,
    pub active: Option<Release>,
    pub staged: Option<Release>,
    #[serde(default)]
    pub revoked: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct StoredState {
    pub generation: u64,
    pub sources: BTreeMap<String, StoredSource>,
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_bundled_runtime_version_matches_the_packaged_pin() {
        let pin: serde_json::Value =
            serde_json::from_str(include_str!("../../../packaging/source-runtime.json")).unwrap();
        assert_eq!(pin["version"], super::BUNDLED_RUNTIME_VERSION);
    }
}
