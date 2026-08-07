//! Typed consumer for the openkara-models infrastructure catalog.
//!
//! The catalog has two layers:
//!
//! 1. A mutable **stable pointer** (`catalog/channels/stable.json` on the
//!    `openkara-models` default branch) naming the current generation and the
//!    immutable release manifest's URL, byte size, and SHA-256.
//! 2. An immutable **release manifest** (a content-addressed release asset)
//!    listing model and runtime artifacts with digests and reciprocal
//!    compatibility data.
//!
//! A verbatim snapshot of both files ships inside the binary
//! (`src-tauri/catalog/`). That snapshot is the offline trust anchor: model
//! resolution never requires the network, and a catalog refresh failure can
//! never invalidate a verified installed model. The same snapshot is consumed
//! by `scripts/resolve-model.mjs`, `scripts/setup.sh`, and CI so every
//! consumer resolves artifacts from one contract fixture.
//!
//! Network refreshes fetch the pointer, verify the manifest bytes against the
//! pointer's declared size and SHA-256 **before parsing**, and reject any
//! generation older than the embedded snapshot or the installed model.

use crate::config::{ExecutionProviderPreference, ModelVariant};
use crate::separator::verified_manifest::sha256_hex;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Where the stable channel pointer lives. The repository is the trust root;
/// manifest bytes fetched through it are still digest-verified.
pub const STABLE_POINTER_URL: &str =
    "https://raw.githubusercontent.com/thedavidweng/openkara-models/main/catalog/channels/stable.json";

pub const POINTER_SCHEMA_VERSION: &str = "openkara.catalog/channel-v1";
pub const RELEASE_SCHEMA_VERSION: &str = "openkara.catalog/release-v1";
pub const INSTALLED_IDENTITY_SCHEMA_VERSION: &str = "openkara.app/installed-artifact-v1";

/// The target triple this build installs runtime artifacts for. The catalog
/// keys runtimes by target triple, so this is the single remaining
/// per-platform constant in the runtime path.
pub fn current_target_triple() -> &'static str {
    #[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(all(target_vendor = "apple", target_arch = "x86_64"))]
    {
        "x86_64-apple-darwin"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "x86_64-unknown-linux-gnu"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "aarch64-unknown-linux-gnu"
    }
    #[cfg(target_os = "windows")]
    {
        "x86_64-pc-windows-msvc"
    }
}

/// The pointer is a tiny JSON object; anything larger is malformed or hostile.
const MAX_POINTER_BYTES: u64 = 64 * 1024;
/// Release manifests grow with artifact count but stay far below this bound.
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

const EMBEDDED_POINTER_JSON: &str = include_str!("../../catalog/stable-pointer.json");
const EMBEDDED_MANIFEST_JSON: &str = include_str!("../../catalog/release-manifest.json");

// ---------------------------------------------------------------------------
// Catalog wire types (field names match the published catalog verbatim).
// Unknown fields are tolerated: the catalog is additive across generations.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StablePointer {
    pub schema_version: String,
    pub channel: String,
    pub generation: u64,
    pub release_id: String,
    pub release_manifest_url: String,
    pub release_manifest_sha256: String,
    pub release_manifest_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub schema_version: String,
    pub generation: u64,
    pub release_id: String,
    pub artifacts: CatalogArtifacts,
    pub compatibility: Vec<CompatibilityEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogArtifacts {
    pub models: Vec<CatalogModel>,
    pub runtimes: Vec<CatalogRuntime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogModel {
    pub artifact_id: String,
    pub variant: String,
    pub profile: String,
    pub filename: String,
    pub byte_size: u64,
    /// Digest of the downloaded payload. For unarchived models this equals
    /// the `.onnx` file digest; for compressed models it is the archive.
    pub archive_digest: String,
    pub download_url: String,
    /// Per-file digests of the installed content. For unarchived models this
    /// holds the single `.onnx` entry.
    pub extracted_file_digests: std::collections::BTreeMap<String, CatalogFileDigest>,
    #[serde(default)]
    pub deprecation: CatalogDeprecation,
    pub upstream: CatalogUpstream,
    pub model: CatalogModelMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_fixture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_limit: Option<CapabilityLimit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_user_effect: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityLimit {
    pub max_segment_seconds: f64,
    pub sample_rate_hz: u32,
    pub channels: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CatalogDeprecation {
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub replacement_artifact_id: Option<String>,
}

impl CatalogModel {
    /// The installed `.onnx` file this artifact provides. Compressed
    /// artifacts extract it from the archive; raw artifacts ARE it.
    pub fn primary_model_file(&self) -> Result<(&str, &CatalogFileDigest)> {
        let mut onnx_entries = self
            .extracted_file_digests
            .iter()
            .filter(|(path, _)| path.ends_with(".onnx"));
        let (path, digest) = onnx_entries
            .next()
            .with_context(|| format!("model {} declares no .onnx file", self.artifact_id))?;
        if onnx_entries.next().is_some() {
            bail!(
                "model {} declares more than one .onnx file",
                self.artifact_id
            );
        }
        Ok((path.as_str(), digest))
    }

    /// True when the download payload is an archive that must be extracted.
    pub fn is_archived(&self) -> bool {
        self.filename.ends_with(".tar.gz")
            || self.filename.ends_with(".tgz")
            || self.filename.ends_with(".zip")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogUpstream {
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogModelMetadata {
    pub cache_key: String,
    pub compatible_runtime_ids: Vec<String>,
    pub format: String,
    pub precision: String,
    pub tensor_interface: String,
    pub stem_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogRuntime {
    pub artifact_id: String,
    pub target_triple: Option<String>,
    pub filename: String,
    pub byte_size: u64,
    pub archive_digest: String,
    pub download_url: String,
    /// Per-file digests of the archive contents, keyed by relative path.
    pub extracted_file_digests: std::collections::BTreeMap<String, CatalogFileDigest>,
    pub runtime: CatalogRuntimeMetadata,
    #[serde(default)]
    pub deprecation: CatalogDeprecation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogFileDigest {
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogRuntimeMetadata {
    pub version: String,
    /// Published as a string (e.g. `"27"`).
    pub ort_c_api_level: String,
    pub execution_providers: Vec<String>,
    pub supported_model_artifact_ids: Vec<String>,
    pub companion_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityEdge {
    pub execution_provider: String,
    pub model_artifact_id: String,
    pub runtime_artifact_id: String,
    pub status: String,
    pub target_triple: String,
}

/// A manifest whose bytes have been verified against a pointer and whose
/// content passed structural validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedCatalog {
    pub generation: u64,
    pub release_id: String,
    pub manifest: ReleaseManifest,
}

// ---------------------------------------------------------------------------
// Installed artifact records (one schema for models and runtimes)
// ---------------------------------------------------------------------------

/// A file installed as part of an artifact, relative to the artifact's
/// install directory, with the digest verified at install time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledFileRecord {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// Identity of an installed artifact — the ONE record schema shared by
/// models and runtimes. For models it is written next to the model file as
/// `<model>.identity.json`; for runtimes it is `record.json` inside the
/// artifact's install directory. This is what makes an artifact installed
/// from a newer catalog generation stay usable when the app binary still
/// embeds an older snapshot, and what update comparisons run against.
///
/// `installed_at_unix` is informational metadata only — identity decisions
/// never rely on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledArtifactRecord {
    pub record_schema: String,
    pub catalog_schema: String,
    pub generation: u64,
    pub release_id: String,
    pub artifact_id: String,
    pub kind: String,
    pub target: Option<String>,
    pub variant: Option<String>,
    pub profile: Option<String>,
    pub format: Option<String>,
    pub precision: Option<String>,
    pub tensor_interface: Option<String>,
    pub upstream_version: String,
    pub download_url: String,
    pub archive_size: u64,
    pub archive_sha256: String,
    pub files: Vec<InstalledFileRecord>,
    pub compatible_ids: Vec<String>,
    pub installed_at_unix: u64,
}

impl InstalledArtifactRecord {
    fn is_structurally_valid(&self) -> bool {
        self.record_schema == INSTALLED_IDENTITY_SCHEMA_VERSION
            && is_sha256_hex(&self.archive_sha256)
            && self.archive_size > 0
            && self.generation > 0
            && !self.artifact_id.is_empty()
            && self
                .files
                .iter()
                .all(|file| is_sha256_hex(&file.sha256) && !file.path.is_empty())
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

pub fn installed_identity_path(model_path: &Path) -> Result<PathBuf> {
    let filename = model_path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("path {} has no filename", model_path.display()))?;
    Ok(model_path.with_file_name(format!("{filename}.identity.json")))
}

/// Read and structurally validate the installed identity for a model file.
/// Returns `None` when the record is absent, unreadable, or malformed —
/// callers treat that the same as "identity unknown".
pub fn read_installed_identity(model_path: &Path) -> Option<InstalledArtifactRecord> {
    let identity_path = installed_identity_path(model_path).ok()?;
    read_artifact_record(&identity_path)
}

/// Read and structurally validate an installed artifact record at an exact
/// path (`<model>.identity.json` for models, `record.json` for runtimes).
pub fn read_artifact_record(record_path: &Path) -> Option<InstalledArtifactRecord> {
    let contents = std::fs::read_to_string(record_path).ok()?;
    let record: InstalledArtifactRecord = serde_json::from_str(&contents).ok()?;
    record.is_structurally_valid().then_some(record)
}

pub fn write_installed_identity(
    model_path: &Path,
    identity: &InstalledArtifactRecord,
) -> Result<()> {
    let identity_path = installed_identity_path(model_path)?;
    write_artifact_record(&identity_path, identity)
}

/// Atomically persist an installed artifact record (temp file + rename).
pub fn write_artifact_record(record_path: &Path, record: &InstalledArtifactRecord) -> Result<()> {
    let json =
        serde_json::to_string_pretty(record).context("failed to serialize artifact record")?;
    let temp_path = record_path.with_extension("json.tmp");
    std::fs::write(&temp_path, json).with_context(|| {
        format!(
            "failed to write artifact record temp file {}",
            temp_path.display()
        )
    })?;
    std::fs::rename(&temp_path, record_path).with_context(|| {
        format!(
            "failed to promote artifact record {}",
            record_path.display()
        )
    })?;
    Ok(())
}

pub fn delete_installed_identity(model_path: &Path) -> Result<()> {
    let identity_path = installed_identity_path(model_path)?;
    if identity_path.exists() {
        std::fs::remove_file(&identity_path).with_context(|| {
            format!(
                "failed to delete model identity record {}",
                identity_path.display()
            )
        })?;
    }
    Ok(())
}

pub fn identity_from_catalog_model(
    model: &CatalogModel,
    catalog: &VerifiedCatalog,
) -> InstalledArtifactRecord {
    InstalledArtifactRecord {
        record_schema: INSTALLED_IDENTITY_SCHEMA_VERSION.to_owned(),
        catalog_schema: catalog.manifest.schema_version.clone(),
        generation: catalog.generation,
        release_id: catalog.release_id.clone(),
        artifact_id: model.artifact_id.clone(),
        kind: "model".to_owned(),
        target: None,
        variant: Some(model.variant.clone()),
        profile: Some(model.profile.clone()),
        format: Some(model.model.format.clone()),
        precision: Some(model.model.precision.clone()),
        tensor_interface: Some(model.model.tensor_interface.clone()),
        upstream_version: model.upstream.tag.clone(),
        download_url: model.download_url.clone(),
        archive_size: model.byte_size,
        archive_sha256: model.archive_digest.clone(),
        files: model
            .extracted_file_digests
            .iter()
            .map(|(path, digest)| InstalledFileRecord {
                path: path.clone(),
                size: digest.size,
                sha256: digest.sha256.clone(),
            })
            .collect(),
        compatible_ids: model.model.compatible_runtime_ids.clone(),
        installed_at_unix: unix_now(),
    }
}

pub fn record_from_catalog_runtime(
    runtime: &CatalogRuntime,
    catalog: &VerifiedCatalog,
) -> InstalledArtifactRecord {
    InstalledArtifactRecord {
        record_schema: INSTALLED_IDENTITY_SCHEMA_VERSION.to_owned(),
        catalog_schema: catalog.manifest.schema_version.clone(),
        generation: catalog.generation,
        release_id: catalog.release_id.clone(),
        artifact_id: runtime.artifact_id.clone(),
        kind: "runtime".to_owned(),
        target: runtime.target_triple.clone(),
        variant: None,
        profile: None,
        format: None,
        precision: None,
        tensor_interface: None,
        upstream_version: runtime.runtime.version.clone(),
        download_url: runtime.download_url.clone(),
        archive_size: runtime.byte_size,
        archive_sha256: runtime.archive_digest.clone(),
        files: runtime
            .extracted_file_digests
            .iter()
            .map(|(path, digest)| InstalledFileRecord {
                path: path.clone(),
                size: digest.size,
                sha256: digest.sha256.clone(),
            })
            .collect(),
        compatible_ids: runtime.runtime.supported_model_artifact_ids.clone(),
        installed_at_unix: unix_now(),
    }
}

// ---------------------------------------------------------------------------
// Parsing and validation
// ---------------------------------------------------------------------------

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_trusted_https_url(value: &str) -> bool {
    value.starts_with("https://")
}

pub fn parse_stable_pointer(bytes: &[u8]) -> Result<StablePointer> {
    let pointer: StablePointer =
        serde_json::from_slice(bytes).context("failed to parse stable catalog pointer")?;

    if pointer.schema_version != POINTER_SCHEMA_VERSION {
        bail!(
            "unsupported catalog pointer schema {} (expected {})",
            pointer.schema_version,
            POINTER_SCHEMA_VERSION
        );
    }
    if pointer.channel != "stable" {
        bail!("unexpected catalog channel {}", pointer.channel);
    }
    if pointer.generation == 0 {
        bail!("catalog pointer generation must be positive");
    }
    if pointer.release_id.is_empty() {
        bail!("catalog pointer release_id is empty");
    }
    if !is_sha256_hex(&pointer.release_manifest_sha256) {
        bail!(
            "catalog pointer manifest digest {} is not a SHA-256 hex string",
            pointer.release_manifest_sha256
        );
    }
    if pointer.release_manifest_size == 0 || pointer.release_manifest_size > MAX_MANIFEST_BYTES {
        bail!(
            "catalog pointer manifest size {} is outside the accepted bounds",
            pointer.release_manifest_size
        );
    }
    if !is_trusted_https_url(&pointer.release_manifest_url) {
        bail!(
            "catalog pointer manifest URL {} is not HTTPS",
            pointer.release_manifest_url
        );
    }

    Ok(pointer)
}

/// Verify manifest bytes against the pointer's declared size and SHA-256,
/// then parse and structurally validate the manifest. The parse only happens
/// after the byte-level verification succeeds.
pub fn verify_and_parse_manifest(bytes: &[u8], pointer: &StablePointer) -> Result<ReleaseManifest> {
    if bytes.len() as u64 != pointer.release_manifest_size {
        bail!(
            "release manifest size mismatch: pointer declares {} bytes, got {}",
            pointer.release_manifest_size,
            bytes.len()
        );
    }
    let actual_sha256 = sha256_hex(bytes);
    if actual_sha256 != pointer.release_manifest_sha256 {
        bail!(
            "release manifest digest mismatch: pointer declares {}, got {}",
            pointer.release_manifest_sha256,
            actual_sha256
        );
    }

    let manifest: ReleaseManifest =
        serde_json::from_slice(bytes).context("failed to parse release manifest")?;

    if manifest.schema_version != RELEASE_SCHEMA_VERSION {
        bail!(
            "unsupported release manifest schema {} (expected {})",
            manifest.schema_version,
            RELEASE_SCHEMA_VERSION
        );
    }
    if manifest.generation != pointer.generation {
        bail!(
            "release manifest generation {} does not match pointer generation {}",
            manifest.generation,
            pointer.generation
        );
    }
    if manifest.release_id != pointer.release_id {
        bail!(
            "release manifest release_id {} does not match pointer release_id {}",
            manifest.release_id,
            pointer.release_id
        );
    }

    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &ReleaseManifest) -> Result<()> {
    if manifest.artifacts.models.is_empty() {
        bail!("release manifest lists no models");
    }
    if manifest.artifacts.runtimes.is_empty() {
        bail!("release manifest lists no runtimes");
    }
    if manifest.compatibility.is_empty() {
        bail!("release manifest has no compatibility edges");
    }

    let mut artifact_ids = HashSet::new();
    let mut runtime_ids = HashSet::new();

    for runtime in &manifest.artifacts.runtimes {
        if !artifact_ids.insert(runtime.artifact_id.as_str()) {
            bail!("duplicate artifact id {}", runtime.artifact_id);
        }
        runtime_ids.insert(runtime.artifact_id.as_str());
        if runtime.byte_size == 0 {
            bail!("runtime {} declares zero byte size", runtime.artifact_id);
        }
        if !is_sha256_hex(&runtime.archive_digest) {
            bail!(
                "runtime {} digest is not a SHA-256 hex string",
                runtime.artifact_id
            );
        }
        if !is_trusted_https_url(&runtime.download_url) {
            bail!("runtime {} URL is not HTTPS", runtime.artifact_id);
        }
    }

    for model in &manifest.artifacts.models {
        if !artifact_ids.insert(model.artifact_id.as_str()) {
            bail!("duplicate artifact id {}", model.artifact_id);
        }
        if model.byte_size == 0 {
            bail!("model {} declares zero byte size", model.artifact_id);
        }
        if !is_sha256_hex(&model.archive_digest) {
            bail!(
                "model {} digest is not a SHA-256 hex string",
                model.artifact_id
            );
        }
        if !is_trusted_https_url(&model.download_url) {
            bail!("model {} URL is not HTTPS", model.artifact_id);
        }
        if model.filename.is_empty()
            || model.filename.contains('/')
            || model.filename.contains('\\')
        {
            bail!(
                "model {} filename {:?} is not a plain file name",
                model.artifact_id,
                model.filename
            );
        }
        model.primary_model_file()?;
        if model.model.format != "onnx" {
            bail!(
                "model {} format {} is not consumable (expected onnx)",
                model.artifact_id,
                model.model.format
            );
        }
        if !matches!(
            model.model.tensor_interface.as_str(),
            "waveform" | "spectral-core"
        ) {
            bail!(
                "model {} tensor interface {} is not consumable \
                 (expected waveform or spectral-core)",
                model.artifact_id,
                model.model.tensor_interface
            );
        }
        if model.model.compatible_runtime_ids.is_empty() {
            bail!(
                "model {} declares no compatible runtimes",
                model.artifact_id
            );
        }
        for runtime_id in &model.model.compatible_runtime_ids {
            if !runtime_ids.contains(runtime_id.as_str()) {
                bail!(
                    "model {} references unknown runtime {}",
                    model.artifact_id,
                    runtime_id
                );
            }
        }
        let has_edge = manifest
            .compatibility
            .iter()
            .any(|edge| edge.model_artifact_id == model.artifact_id);
        if !has_edge {
            bail!("model {} has no compatibility edges", model.artifact_id);
        }
    }

    for edge in &manifest.compatibility {
        if !runtime_ids.contains(edge.runtime_artifact_id.as_str()) {
            bail!(
                "compatibility edge references unknown runtime {}",
                edge.runtime_artifact_id
            );
        }
        if !manifest
            .artifacts
            .models
            .iter()
            .any(|model| model.artifact_id == edge.model_artifact_id)
        {
            bail!(
                "compatibility edge references unknown model {}",
                edge.model_artifact_id
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Embedded snapshot (offline trust anchor)
// ---------------------------------------------------------------------------

/// The catalog snapshot compiled into the binary. Panics only when the
/// checked-in snapshot itself is inconsistent, which the test suite rejects
/// before such a build could ship.
pub fn embedded_catalog() -> &'static VerifiedCatalog {
    static EMBEDDED: OnceLock<VerifiedCatalog> = OnceLock::new();
    EMBEDDED.get_or_init(|| {
        load_embedded_catalog().expect("embedded catalog snapshot must be self-consistent")
    })
}

fn load_embedded_catalog() -> Result<VerifiedCatalog> {
    let pointer = parse_stable_pointer(EMBEDDED_POINTER_JSON.as_bytes())
        .context("embedded stable pointer is invalid")?;
    let manifest = verify_and_parse_manifest(EMBEDDED_MANIFEST_JSON.as_bytes(), &pointer)
        .context("embedded release manifest is invalid")?;
    Ok(VerifiedCatalog {
        generation: manifest.generation,
        release_id: manifest.release_id.clone(),
        manifest,
    })
}

// ---------------------------------------------------------------------------
// Model resolution
// ---------------------------------------------------------------------------

/// Resolve the runtime artifact for a target triple.
///
/// A target may publish more than one active runtime when the host capability
/// alone cannot pick a safe artifact (issue OpenKara/OpenKara#284): the Windows
/// catalog ships both a DirectML-linked runtime and a CPU-only runtime. A
/// virtual display adapter can pass the D3D12 capability probe yet deadlock
/// loading `DirectML.dll`, so the EP preference disambiguates — a CPU/DirectML
/// catalog pair resolves to the CPU artifact when the caller prefers CPU, and
/// to the DirectML artifact when it prefers DirectML.
///
/// When exactly one active runtime matches the target, that runtime wins
/// regardless of preference (the legacy single-runtime-per-target behavior),
/// so older catalogs keep resolving as before.
pub fn resolve_runtime<'a>(
    manifest: &'a ReleaseManifest,
    target_triple: &str,
    preferred_ep: ExecutionProviderPreference,
) -> Result<&'a CatalogRuntime> {
    // Superseded runtimes stay listed for provenance (generation 9 keeps the
    // full-operator builds deprecated next to their reduced replacements), so
    // resolution must skip them the same way resolve_model skips deprecated
    // and non-loadable model deliveries.
    let matches: Vec<&CatalogRuntime> = manifest
        .artifacts
        .runtimes
        .iter()
        .filter(|runtime| {
            runtime.target_triple.as_deref() == Some(target_triple)
                && !runtime.deprecation.deprecated
        })
        .collect();

    match matches.len() {
        0 => bail!("catalog has no active runtime for target {target_triple}"),
        1 => Ok(matches[0]),
        _ => {
            let preferred = runtime_supporting_ep(&matches, preferred_ep)
                .with_context(|| format!("catalog lists more than one active runtime for target {target_triple} and none matches the preferred execution provider {preferred_ep:?}"))?;
            Ok(preferred)
        }
    }
}

pub fn runtime_by_artifact_id<'a>(
    manifest: &'a ReleaseManifest,
    artifact_id: &str,
) -> Option<&'a CatalogRuntime> {
    manifest
        .artifacts
        .runtimes
        .iter()
        .find(|runtime| runtime.artifact_id == artifact_id)
}

/// Pick the catalog runtime that declares the preferred execution provider.
///
/// DirectML-preference selects a runtime advertising `directml`; any other
/// preference selects the CPU-only runtime (the runtime whose provider list
/// is exactly `["cpu"]`, or, failing an exact match, the runtime that does not
/// advertise `directml`). This is what keeps a CPU-preferred host off the
/// DirectML-linked DLL that deadlocks on virtual adapters.
fn runtime_supporting_ep<'a>(
    runtimes: &[&'a CatalogRuntime],
    preferred_ep: ExecutionProviderPreference,
) -> Option<&'a CatalogRuntime> {
    let advertises = |rt: &CatalogRuntime, ep: &str| {
        rt.runtime
            .execution_providers
            .iter()
            .any(|provider| provider == ep)
    };

    if matches!(preferred_ep, ExecutionProviderPreference::DirectMl) {
        return runtimes
            .iter()
            .copied()
            .find(|rt| advertises(rt, "directml"))
            .or_else(|| {
                runtimes
                    .iter()
                    .copied()
                    .find(|rt| rt.runtime.execution_providers == ["cpu"])
            });
    }

    runtimes
        .iter()
        .copied()
        .find(|rt| rt.runtime.execution_providers == ["cpu"])
        .or_else(|| {
            runtimes
                .iter()
                .copied()
                .find(|rt| !advertises(rt, "directml"))
        })
}

/// The only tensor interface this build can load (the spectral session is
/// the sole production path). Resolution must never select an artifact the
/// loader will refuse: manifests keep listing waveform deliveries for
/// compatibility, but they are not candidates here.
const LOADABLE_TENSOR_INTERFACE: &str = "spectral-core";

/// Resolve the preferred artifact for a variant. Newer generations publish
/// several deliveries per variant; among non-deprecated candidates with a
/// loadable tensor interface the smallest download wins — deterministic,
/// and never an artifact the model loader would refuse (the fine-tuned
/// variant's waveform dual archive is smaller than its spectral-core
/// delivery, so a size-only rule would resolve a model that cannot load).
pub fn resolve_model(manifest: &ReleaseManifest, variant: ModelVariant) -> Result<&CatalogModel> {
    manifest
        .artifacts
        .models
        .iter()
        .filter(|model| {
            model.variant == variant.as_str()
                && !model.deprecation.deprecated
                && model.model.tensor_interface == LOADABLE_TENSOR_INTERFACE
        })
        .min_by_key(|model| model.byte_size)
        .with_context(|| {
            format!(
                "catalog has no loadable ({LOADABLE_TENSOR_INTERFACE}) model for variant {}",
                variant.as_str()
            )
        })
}

// ---------------------------------------------------------------------------
// Network refresh
// ---------------------------------------------------------------------------

fn read_bounded(response: &mut impl Read, max_bytes: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() as u64 > max_bytes {
            bail!("response exceeded the {max_bytes}-byte bound");
        }
    }
    Ok(bytes)
}

/// Fetch and verify the current stable catalog from the network.
///
/// The fetched generation must be at least the embedded snapshot's
/// generation — the stable channel is monotonic, so anything older is a
/// stale mirror or a rollback attempt and is rejected.
pub fn fetch_stable_catalog() -> Result<VerifiedCatalog> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build catalog HTTP client")?;

    let mut pointer_response = client
        .get(STABLE_POINTER_URL)
        .send()
        .and_then(|response| response.error_for_status())
        .with_context(|| format!("failed to fetch catalog pointer from {STABLE_POINTER_URL}"))?;
    let pointer_bytes = read_bounded(&mut pointer_response, MAX_POINTER_BYTES)
        .context("failed while reading catalog pointer")?;
    let pointer = parse_stable_pointer(&pointer_bytes)?;

    if pointer.generation < embedded_catalog().generation {
        bail!(
            "stable catalog generation {} is older than the embedded snapshot generation {}",
            pointer.generation,
            embedded_catalog().generation
        );
    }

    let mut manifest_response = client
        .get(&pointer.release_manifest_url)
        .send()
        .and_then(|response| response.error_for_status())
        .with_context(|| {
            format!(
                "failed to fetch release manifest from {}",
                pointer.release_manifest_url
            )
        })?;
    let manifest_bytes = read_bounded(&mut manifest_response, MAX_MANIFEST_BYTES)
        .context("failed while reading release manifest")?;
    let manifest = verify_and_parse_manifest(&manifest_bytes, &pointer)?;

    Ok(VerifiedCatalog {
        generation: manifest.generation,
        release_id: manifest.release_id.clone(),
        manifest,
    })
}

// ---------------------------------------------------------------------------
// Update comparison
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelUpdateState {
    NotInstalled,
    UpToDate,
    UpdateAvailable,
    /// A model file is installed but carries no identity record (installed by
    /// an older app build). Downloading the catalog artifact adopts it.
    InstalledWithoutIdentity,
}

#[derive(Debug, Clone)]
pub struct ModelUpdateComparison {
    pub state: ModelUpdateState,
    pub installed: Option<InstalledArtifactRecord>,
}

/// Compare an installed artifact record against the catalog's current
/// artifact for the same slot (a model variant, or the target's runtime).
///
/// Implicit downgrades are rejected: when the installed record's generation
/// is newer than the catalog's, the catalog is stale for this artifact and
/// the comparison fails instead of offering the older artifact as an
/// "update".
pub fn compare_installed_artifact(
    installed: Option<InstalledArtifactRecord>,
    catalog_artifact_id: &str,
    catalog_archive_digest: &str,
    catalog: &VerifiedCatalog,
    file_exists: bool,
) -> Result<ModelUpdateComparison> {
    let Some(identity) = installed else {
        let state = if file_exists {
            ModelUpdateState::InstalledWithoutIdentity
        } else {
            ModelUpdateState::NotInstalled
        };
        return Ok(ModelUpdateComparison {
            state,
            installed: None,
        });
    };

    if identity.generation > catalog.generation {
        bail!(
            "catalog generation {} is older than the installed artifact generation {}; refusing implicit downgrade",
            catalog.generation,
            identity.generation
        );
    }

    let state = if identity.artifact_id == catalog_artifact_id
        && identity.archive_sha256 == catalog_archive_digest
    {
        ModelUpdateState::UpToDate
    } else {
        ModelUpdateState::UpdateAvailable
    };

    Ok(ModelUpdateComparison {
        state,
        installed: Some(identity),
    })
}

pub fn compare_installed_model(
    installed: Option<InstalledArtifactRecord>,
    catalog_model: &CatalogModel,
    catalog: &VerifiedCatalog,
    model_file_exists: bool,
) -> Result<ModelUpdateComparison> {
    compare_installed_artifact(
        installed,
        &catalog_model.artifact_id,
        &catalog_model.archive_digest,
        catalog,
        model_file_exists,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedded_pointer() -> StablePointer {
        parse_stable_pointer(EMBEDDED_POINTER_JSON.as_bytes()).expect("embedded pointer")
    }

    fn manifest_json() -> serde_json::Value {
        serde_json::from_str(EMBEDDED_MANIFEST_JSON).expect("embedded manifest JSON")
    }

    /// Re-point a pointer at mutated manifest bytes so validation reaches the
    /// structural checks instead of failing on digest mismatch.
    fn pointer_for(bytes: &[u8]) -> StablePointer {
        let mut pointer = embedded_pointer();
        pointer.release_manifest_sha256 = sha256_hex(bytes);
        pointer.release_manifest_size = bytes.len() as u64;
        pointer
    }

    fn parse_mutated(mutate: impl FnOnce(&mut serde_json::Value)) -> Result<ReleaseManifest> {
        let mut manifest = manifest_json();
        mutate(&mut manifest);
        let bytes = serde_json::to_vec(&manifest).expect("serialize mutated manifest");
        verify_and_parse_manifest(&bytes, &pointer_for(&bytes))
    }

    #[test]
    fn embedded_snapshot_is_self_consistent() {
        use crate::config::ExecutionProviderPreference as Ep;

        let catalog = load_embedded_catalog().expect("embedded catalog must load");
        assert!(catalog.generation >= 6);
        // Generations publish several deliveries per variant (raw kept for
        // older consumers, compressed preferred); both variants must resolve.
        assert!(catalog.manifest.artifacts.models.len() >= 2);
        // Superseded runtimes stay listed but deprecated; at least one active
        // runtime per supported target must remain resolvable. Windows may
        // publish more than one active runtime (CPU + DirectML) once issue
        // OpenKara/OpenKara#284's CPU artifact lands; the count assertion is a
        // floor, not an exact match.
        let active: Vec<_> = catalog
            .manifest
            .artifacts
            .runtimes
            .iter()
            .filter(|runtime| !runtime.deprecation.deprecated)
            .collect();
        assert!(active.len() >= 5, "expected at least 5 active runtimes");
        let targets: std::collections::BTreeSet<_> = active
            .iter()
            .filter_map(|r| r.target_triple.as_deref())
            .collect();
        assert!(
            targets.len() >= 5,
            "expected at least 5 active runtime targets"
        );
        // Each active runtime target must resolve for both EP preferences so
        // the consumer never sees an ambiguous catalog it cannot disambiguate.
        for target in &targets {
            resolve_runtime(&catalog.manifest, target, Ep::Cpu)
                .expect("active target must resolve for CPU preference");
            resolve_runtime(&catalog.manifest, target, Ep::DirectMl)
                .expect("active target must resolve for DirectML preference");
        }
        assert!(!catalog.manifest.compatibility.is_empty());
    }

    #[test]
    fn resolves_both_model_variants_from_embedded_snapshot() {
        let catalog = embedded_catalog();
        let htdemucs =
            resolve_model(&catalog.manifest, ModelVariant::Htdemucs).expect("htdemucs model");
        assert!(!htdemucs.deprecation.deprecated);
        let (model_file, _) = htdemucs.primary_model_file().expect("primary file");
        assert!(model_file.ends_with(".onnx"));
        assert_eq!(htdemucs.model.precision, "fp32");
        assert!(!htdemucs.model.compatible_runtime_ids.is_empty());

        let ft = resolve_model(&catalog.manifest, ModelVariant::HtdemucsFt).expect("ft model");
        assert!(!ft.deprecation.deprecated);
        assert_ne!(ft.artifact_id, htdemucs.artifact_id);

        // Every resolved artifact must be LOADABLE: the spectral session is
        // the sole production path, and for the ft variant the smallest
        // artifact by size is a waveform archive the loader would refuse —
        // interface-blind resolution would brick the fine-tuned variant.
        for resolved in [htdemucs, ft] {
            assert_eq!(
                resolved.model.tensor_interface, "spectral-core",
                "resolved artifact {} must be loadable",
                resolved.artifact_id
            );
        }
        assert_eq!(htdemucs.artifact_id, "htdemucs.spectral.fp32.onnx");
        assert_eq!(ft.artifact_id, "htdemucs_ft.spectral.fp32.onnx");
    }

    #[test]
    fn every_runtime_in_embedded_snapshot_records_api_level_27() {
        for runtime in &embedded_catalog().manifest.artifacts.runtimes {
            assert_eq!(
                runtime.runtime.ort_c_api_level, "27",
                "runtime {} must record ORT C API level 27",
                runtime.artifact_id
            );
        }
    }

    /// Build a minimal runtime catalog entry for resolve_runtime tests.
    fn runtime_fixture(
        artifact_id: &str,
        target_triple: &str,
        execution_providers: &[&str],
    ) -> CatalogRuntime {
        CatalogRuntime {
            artifact_id: artifact_id.to_owned(),
            target_triple: Some(target_triple.to_owned()),
            filename: format!("{artifact_id}.zip"),
            byte_size: 1,
            archive_digest: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_owned(),
            download_url: format!("https://example.invalid/{artifact_id}.zip"),
            extracted_file_digests: Default::default(),
            runtime: CatalogRuntimeMetadata {
                version: "v1.27.1".to_owned(),
                ort_c_api_level: "27".to_owned(),
                execution_providers: execution_providers
                    .iter()
                    .map(|provider| provider.to_string())
                    .collect(),
                supported_model_artifact_ids: vec![],
                companion_files: vec![],
            },
            deprecation: CatalogDeprecation::default(),
        }
    }

    fn manifest_with_runtimes(runtimes: Vec<CatalogRuntime>) -> ReleaseManifest {
        ReleaseManifest {
            schema_version: RELEASE_SCHEMA_VERSION.to_owned(),
            generation: 11,
            release_id: "2099-01-01-001".to_owned(),
            artifacts: CatalogArtifacts {
                models: vec![],
                runtimes,
            },
            compatibility: vec![],
        }
    }

    #[test]
    fn resolve_runtime_picks_single_active_runtime_regardless_of_ep() {
        use crate::config::ExecutionProviderPreference as Ep;

        let manifest = manifest_with_runtimes(vec![runtime_fixture(
            "rt-windows-dml",
            "x86_64-pc-windows-msvc",
            &["cpu", "directml"],
        )]);

        let cpu =
            resolve_runtime(&manifest, "x86_64-pc-windows-msvc", Ep::Cpu).expect("cpu resolves");
        let dml = resolve_runtime(&manifest, "x86_64-pc-windows-msvc", Ep::DirectMl)
            .expect("dml resolves");
        assert_eq!(cpu.artifact_id, "rt-windows-dml");
        assert_eq!(dml.artifact_id, "rt-windows-dml");
    }

    #[test]
    fn resolve_runtime_disambiguates_windows_cpu_and_directml_by_ep() {
        use crate::config::ExecutionProviderPreference as Ep;

        // Issue #284 catalog shape: a CPU-only runtime ships alongside the
        // DirectML runtime on the same Windows target.
        let manifest = manifest_with_runtimes(vec![
            runtime_fixture("rt-windows-cpu", "x86_64-pc-windows-msvc", &["cpu"]),
            runtime_fixture(
                "rt-windows-dml",
                "x86_64-pc-windows-msvc",
                &["cpu", "directml"],
            ),
        ]);

        // CPU preference must pick the CPU-only runtime — loading the
        // DirectML runtime is what deadlocks on virtual adapters.
        let cpu = resolve_runtime(&manifest, "x86_64-pc-windows-msvc", Ep::Cpu)
            .expect("cpu preference resolves the cpu-only runtime");
        assert_eq!(cpu.artifact_id, "rt-windows-cpu");
        assert_eq!(cpu.runtime.execution_providers, vec!["cpu".to_owned()]);

        let dml = resolve_runtime(&manifest, "x86_64-pc-windows-msvc", Ep::DirectMl)
            .expect("directml preference resolves the dml runtime");
        assert_eq!(dml.artifact_id, "rt-windows-dml");
        assert!(dml
            .runtime
            .execution_providers
            .iter()
            .any(|provider| provider == "directml"));
    }

    #[test]
    fn resolve_runtime_errors_when_no_active_runtime_matches_target() {
        use crate::config::ExecutionProviderPreference as Ep;

        let manifest = manifest_with_runtimes(vec![]);
        let err = resolve_runtime(&manifest, "x86_64-pc-windows-msvc", Ep::Cpu)
            .expect_err("empty catalog must not resolve");
        assert!(format!("{err}").contains("no active runtime"));
    }

    #[test]
    fn resolve_runtime_skips_deprecated_runtimes() {
        use crate::config::ExecutionProviderPreference as Ep;

        let mut deprecated_full = runtime_fixture(
            "rt-windows-full",
            "x86_64-pc-windows-msvc",
            &["cpu", "directml"],
        );
        deprecated_full.deprecation = CatalogDeprecation {
            deprecated: true,
            replacement_artifact_id: Some("rt-windows-reduced".to_owned()),
        };
        let reduced = runtime_fixture(
            "rt-windows-reduced",
            "x86_64-pc-windows-msvc",
            &["cpu", "directml"],
        );
        let manifest = manifest_with_runtimes(vec![deprecated_full, reduced]);

        let resolved = resolve_runtime(&manifest, "x86_64-pc-windows-msvc", Ep::Cpu)
            .expect("deprecated runtime must be skipped");
        assert_eq!(resolved.artifact_id, "rt-windows-reduced");
        assert!(!resolved.deprecation.deprecated);
    }

    #[test]
    fn rejects_manifest_digest_mismatch() {
        let mut pointer = embedded_pointer();
        pointer.release_manifest_sha256 = "0".repeat(64);
        let error = verify_and_parse_manifest(EMBEDDED_MANIFEST_JSON.as_bytes(), &pointer)
            .expect_err("digest mismatch must be rejected");
        assert!(error.to_string().contains("digest mismatch"));
    }

    #[test]
    fn rejects_manifest_size_mismatch() {
        let mut pointer = embedded_pointer();
        pointer.release_manifest_size += 1;
        let error = verify_and_parse_manifest(EMBEDDED_MANIFEST_JSON.as_bytes(), &pointer)
            .expect_err("size mismatch must be rejected");
        assert!(error.to_string().contains("size mismatch"));
    }

    #[test]
    fn rejects_unsupported_pointer_schema() {
        let mut pointer_json: serde_json::Value =
            serde_json::from_str(EMBEDDED_POINTER_JSON).expect("pointer JSON");
        pointer_json["schema_version"] = "openkara.catalog/channel-v999".into();
        let bytes = serde_json::to_vec(&pointer_json).expect("serialize pointer");
        let error =
            parse_stable_pointer(&bytes).expect_err("unsupported pointer schema must be rejected");
        assert!(error
            .to_string()
            .contains("unsupported catalog pointer schema"));
    }

    #[test]
    fn rejects_unsupported_release_schema() {
        let error = parse_mutated(|manifest| {
            manifest["schema_version"] = "openkara.catalog/release-v999".into();
        })
        .expect_err("unsupported release schema must be rejected");
        assert!(error
            .to_string()
            .contains("unsupported release manifest schema"));
    }

    #[test]
    fn rejects_generation_mismatch_between_pointer_and_manifest() {
        let error = parse_mutated(|manifest| {
            manifest["generation"] = 999.into();
        })
        .expect_err("generation mismatch must be rejected");
        assert!(error
            .to_string()
            .contains("does not match pointer generation"));
    }

    #[test]
    fn rejects_duplicate_artifact_ids() {
        let error = parse_mutated(|manifest| {
            let duplicate = manifest["artifacts"]["models"][0].clone();
            manifest["artifacts"]["models"]
                .as_array_mut()
                .expect("models array")
                .push(duplicate);
        })
        .expect_err("duplicate artifact ids must be rejected");
        assert!(error.to_string().contains("duplicate artifact id"));
    }

    #[test]
    fn rejects_malformed_model_digest() {
        let error = parse_mutated(|manifest| {
            manifest["artifacts"]["models"][0]["archive_digest"] = "not-a-digest".into();
        })
        .expect_err("malformed digest must be rejected");
        assert!(error.to_string().contains("not a SHA-256 hex string"));
    }

    #[test]
    fn rejects_zero_model_size() {
        let error = parse_mutated(|manifest| {
            manifest["artifacts"]["models"][0]["byte_size"] = 0.into();
        })
        .expect_err("zero byte size must be rejected");
        assert!(error.to_string().contains("zero byte size"));
    }

    #[test]
    fn rejects_non_https_model_url() {
        let error = parse_mutated(|manifest| {
            manifest["artifacts"]["models"][0]["download_url"] =
                "http://example.com/htdemucs.onnx".into();
        })
        .expect_err("non-HTTPS URL must be rejected");
        assert!(error.to_string().contains("not HTTPS"));
    }

    #[test]
    fn rejects_wrong_tensor_interface() {
        let error = parse_mutated(|manifest| {
            manifest["artifacts"]["models"][0]["model"]["tensor_interface"] = "spectral".into();
        })
        .expect_err("unknown tensor interface must be rejected");
        assert!(error.to_string().contains("tensor interface"));
    }

    #[test]
    fn accepts_spectral_core_tensor_interface() {
        // Spectral-core artifacts (openkara-models#23) are consumable; the
        // session path is selected by the model's own embedded metadata.
        parse_mutated(|manifest| {
            manifest["artifacts"]["models"][0]["model"]["tensor_interface"] =
                "spectral-core".into();
        })
        .expect("spectral-core tensor interface must be accepted");
    }

    #[test]
    fn rejects_model_without_runtime_compatibility() {
        let error = parse_mutated(|manifest| {
            manifest["artifacts"]["models"][0]["model"]["compatible_runtime_ids"] =
                serde_json::Value::Array(Vec::new());
        })
        .expect_err("empty runtime compatibility must be rejected");
        assert!(error.to_string().contains("no compatible runtimes"));
    }

    #[test]
    fn rejects_model_without_compatibility_edges() {
        let error = parse_mutated(|manifest| {
            let model_id = manifest["artifacts"]["models"][0]["artifact_id"]
                .as_str()
                .expect("model artifact id")
                .to_owned();
            let edges = manifest["compatibility"].as_array().expect("edges").clone();
            manifest["compatibility"] = serde_json::Value::Array(
                edges
                    .into_iter()
                    .filter(|edge| edge["model_artifact_id"] != model_id.as_str())
                    .collect(),
            );
        })
        .expect_err("model without edges must be rejected");
        assert!(error.to_string().contains("no compatibility edges"));
    }

    #[test]
    fn update_comparison_reports_not_installed() {
        let catalog = embedded_catalog();
        let model = resolve_model(&catalog.manifest, ModelVariant::Htdemucs).expect("model");
        let comparison = compare_installed_model(None, model, catalog, false).expect("comparison");
        assert_eq!(comparison.state, ModelUpdateState::NotInstalled);
    }

    #[test]
    fn update_comparison_reports_legacy_install_without_identity() {
        let catalog = embedded_catalog();
        let model = resolve_model(&catalog.manifest, ModelVariant::Htdemucs).expect("model");
        let comparison = compare_installed_model(None, model, catalog, true).expect("comparison");
        assert_eq!(comparison.state, ModelUpdateState::InstalledWithoutIdentity);
    }

    #[test]
    fn update_comparison_reports_up_to_date_for_same_artifact() {
        let catalog = embedded_catalog();
        let model = resolve_model(&catalog.manifest, ModelVariant::Htdemucs).expect("model");
        let identity = identity_from_catalog_model(model, catalog);
        let comparison =
            compare_installed_model(Some(identity), model, catalog, true).expect("comparison");
        assert_eq!(comparison.state, ModelUpdateState::UpToDate);
    }

    #[test]
    fn update_comparison_reports_update_for_changed_digest() {
        let catalog = embedded_catalog();
        let model = resolve_model(&catalog.manifest, ModelVariant::Htdemucs).expect("model");
        let mut identity = identity_from_catalog_model(model, catalog);
        identity.archive_sha256 = "1".repeat(64);
        identity.artifact_id = "htdemucs.balanced.fp32.older".to_owned();
        let comparison =
            compare_installed_model(Some(identity), model, catalog, true).expect("comparison");
        assert_eq!(comparison.state, ModelUpdateState::UpdateAvailable);
    }

    #[test]
    fn update_comparison_rejects_implicit_downgrade() {
        let catalog = embedded_catalog();
        let model = resolve_model(&catalog.manifest, ModelVariant::Htdemucs).expect("model");
        let mut identity = identity_from_catalog_model(model, catalog);
        identity.generation = catalog.generation + 1;
        identity.archive_sha256 = "1".repeat(64);
        let error = compare_installed_model(Some(identity), model, catalog, true)
            .expect_err("downgrade must be rejected");
        assert!(error.to_string().contains("refusing implicit downgrade"));
    }

    #[test]
    fn corrupt_installed_identity_reads_as_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let model_path = dir.path().join("htdemucs.onnx");
        std::fs::write(&model_path, b"model").expect("write model");
        let identity_path = installed_identity_path(&model_path).expect("identity path");
        std::fs::write(&identity_path, b"{not json").expect("write corrupt identity");
        assert!(read_installed_identity(&model_path).is_none());

        // A record with the wrong schema is also treated as unknown.
        std::fs::write(
            &identity_path,
            serde_json::json!({
                "record_schema": "openkara.app/installed-model-v999",
                "generation": 3,
                "release_id": "r",
                "artifact_id": "a",
                "variant": "htdemucs",
                "upstream_tag": "t",
                "format": "onnx",
                "tensor_interface": "waveform",
                "sha256": "0".repeat(64),
                "byte_size": 5,
                "compatible_runtime_ids": ["x"],
            })
            .to_string(),
        )
        .expect("write wrong-schema identity");
        assert!(read_installed_identity(&model_path).is_none());
    }

    #[test]
    fn identity_round_trips_through_disk() {
        let catalog = embedded_catalog();
        let model = resolve_model(&catalog.manifest, ModelVariant::Htdemucs).expect("model");
        let identity = identity_from_catalog_model(model, catalog);

        let dir = tempfile::tempdir().expect("tempdir");
        let model_path = dir.path().join("htdemucs.onnx");
        std::fs::write(&model_path, b"model").expect("write model");
        write_installed_identity(&model_path, &identity).expect("write identity");
        let read_back = read_installed_identity(&model_path).expect("read identity");
        assert_eq!(read_back, identity);

        delete_installed_identity(&model_path).expect("delete identity");
        assert!(read_installed_identity(&model_path).is_none());
    }
}
