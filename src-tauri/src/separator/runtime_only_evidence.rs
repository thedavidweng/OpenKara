//! OpenKara app-integration evidence for runtime-only promotion.
//!
//! This module is the producer side of the contract in
//! `openkara-models` `docs/runtime-only-promotion.md` and
//! `catalog/schema-runtime-only-v1.json#/$defs/app_report`. It consumes a
//! verified `candidate` channel pointer, loads the candidate runtime's exact
//! shared library, and for every supported edge of one
//! `(runtime, target, provider)` group:
//!
//! 1. decodes real audio through the application's own decode and
//!    preprocessing path;
//! 2. loads the candidate model through the application's model loader with
//!    the requested provider;
//! 3. runs one full spectral-core inference window through the same streaming
//!    separation path the product uses, observing the session output shapes
//!    and finiteness;
//! 4. records provider assignment and node placement from the ONNX Runtime
//!    profile of that one window; and
//! 5. finalizes four stem files and checks every decoded stem sample is
//!    finite.
//!
//! Nothing here fabricates a measurement. A model the application cannot load
//! is never measured nor reported as loadable. This build's contract is
//! spectral-core only (`catalog::resolve_model`, `model::
//! ensure_spectral_core_metadata`), so a supported manifest edge whose model is
//! deprecated or declares the legacy `waveform` interface is catalog-compatible
//! but not app-applicable. Those edges are declared in `skipped_edges` with a
//! `scope` (`interface` or `deprecated`) and do not fail the report. An
//! app-applicable edge that fails to measure keeps the report `failed`; a
//! skipped edge outside the catalog-declared non-loadable set is never silently
//! accepted by the gate, which recomputes both conditions from the manifest.
//!
//! The producer never writes to the model catalog repository. It reads the
//! candidate pointer/manifest and the local artifact archives named by the
//! manifest, and writes one report JSON to the caller-selected path.

use crate::{
    audio::{decode, encode::StreamingOggWriter},
    config::{ExecutionProviderPreference, StemMode},
    separator::{
        artifacts, catalog,
        inference::{self, StemWriters},
        model, preprocess,
        workspace::SeparationWorkspace,
    },
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub const APP_REPORT_SCHEMA_VERSION: &str = "openkara.openkara-app-integration/v1";
const CANDIDATE_CHANNEL: &str = "candidate";
const MAX_POINTER_BYTES: u64 = 64 * 1024;
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
const DEMUCS_SAMPLE_RATE: u32 = 44_100;
const DEMUCS_CHANNELS: usize = 2;

/// Inputs for one app-integration report.
#[derive(Debug, Clone)]
pub struct EvidenceConfig {
    /// Published `candidate` channel pointer, as an HTTPS URL or local path.
    pub pointer: String,
    /// Optional local manifest path. The bytes are still verified against the
    /// pointer's declared size and SHA-256; this only avoids a network fetch.
    pub manifest_path: Option<PathBuf>,
    /// Directory holding every archive named by the candidate manifest.
    pub artifacts_dir: PathBuf,
    /// Real audio file decoded through the application's audio path.
    pub audio_path: PathBuf,
    /// Exact candidate runtime artifact to load. One process can load only one
    /// ONNX Runtime, so reports are produced per runtime artifact.
    pub runtime_artifact_id: String,
    /// One provider group per report.
    pub provider: ExecutionProviderPreference,
    /// Host target triple; must equal this build's target.
    pub target_triple: String,
    /// Destination report JSON.
    pub report_path: PathBuf,
    /// Optional working directory for extracted artifacts and stem outputs.
    pub work_dir: Option<PathBuf>,
}

/// Result of a producer run.
#[derive(Debug, Clone)]
pub struct EvidenceOutcome {
    pub report_path: PathBuf,
    pub status: String,
    pub measured_edges: usize,
    /// Supported manifest edges this OpenKara build cannot load. These are
    /// catalog-compatible but not app-applicable (deprecated and/or
    /// waveform-interface models).
    pub not_applicable_edges: Vec<String>,
    /// App-applicable edges that were attempted but failed measurement.
    pub failed_edges: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AppReport {
    schema_version: &'static str,
    report_id: String,
    created_at: String,
    candidate_release_id: String,
    generation: u64,
    candidate_manifest_sha256: String,
    openkara: OpenKaraIdentity,
    target_triple: String,
    status: String,
    edges: Vec<AppEdge>,
    /// Supported manifest edges in this group that are not app-loadable.
    /// The gate recomputes the expected set from the manifest; these rows are
    /// an explicit declaration, not a trusted justification.
    skipped_edges: Vec<SkippedEdge>,
}

#[derive(Debug, Clone, Serialize)]
struct SkippedEdge {
    model_artifact_id: String,
    runtime_artifact_id: String,
    target_triple: String,
    execution_provider: String,
    /// Catalog-declared interface. The gate recomputes non-loadability from
    /// the manifest; this is the producer's declaration.
    tensor_interface: String,
    /// True when the model artifact is deprecated in the manifest.
    deprecated: bool,
    /// Which manifest-declared condition makes this edge non-applicable.
    /// The gate recomputes `interface` (`tensor_interface != "spectral-core"`)
    /// and `deprecated` (`deprecation.deprecated`) from the manifest and only
    /// accepts a row whose declared scope matches that recomputation.
    scope: String,
}

#[derive(Debug, Serialize)]
struct OpenKaraIdentity {
    version: String,
    commit: String,
}

#[derive(Debug, Serialize)]
struct AppEdge {
    identity: AppIdentity,
    loaded_library: LoadedLibrary,
    provider: ProviderEvidence,
    output_shapes: Vec<Vec<i64>>,
    finite_output: bool,
    audio_pipeline: AudioPipeline,
    checks: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AppIdentity {
    model_artifact_id: String,
    runtime_artifact_id: String,
    target_triple: String,
    execution_provider: String,
    model_digest: String,
    model_size: u64,
    runtime_digest: String,
    runtime_size: u64,
}

#[derive(Debug, Serialize)]
struct LoadedLibrary {
    name: String,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct ProviderEvidence {
    requested: String,
    assigned: String,
    used_fallback: bool,
    provider_node_count: usize,
    total_node_count: usize,
    fallback_node_count: usize,
}

#[derive(Debug, Serialize)]
struct AudioPipeline {
    frames_processed: usize,
    sample_rate: u32,
    stems: usize,
    finite: bool,
}

struct PreparedRuntime {
    library_path: PathBuf,
    library_name: String,
    library_sha256: String,
}

#[derive(Debug, Clone, Copy)]
struct NodeCounts {
    total: usize,
    provider: usize,
    fallback: usize,
}

/// Produce one app-integration report.
///
/// Returns `Ok` after writing the report even when some supported edges could
/// not be measured; inspect [`EvidenceOutcome::status`] and
/// [`EvidenceOutcome::failed_edges`]. A report with no measurable edge is not
/// written because the schema requires at least one edge.
pub fn produce(config: EvidenceConfig) -> Result<EvidenceOutcome> {
    let current_target = catalog::current_target_triple();
    if config.target_triple != current_target {
        bail!(
            "requested target {} is not this host target {}",
            config.target_triple,
            current_target
        );
    }
    if rosetta_translated() {
        bail!(
            "this process is Rosetta-translated; run the producer on native {} hardware",
            config.target_triple
        );
    }
    if !config.provider.is_compatible_for_current_platform() {
        bail!(
            "provider {} is not compatible with this host",
            config.provider.as_str()
        );
    }

    let artifacts_dir = config.artifacts_dir.canonicalize().with_context(|| {
        format!(
            "failed to resolve artifacts dir {}",
            config.artifacts_dir.display()
        )
    })?;
    let audio_path = config.audio_path.canonicalize().with_context(|| {
        format!(
            "failed to resolve audio file {}",
            config.audio_path.display()
        )
    })?;

    let pointer_bytes = read_source(&config.pointer, MAX_POINTER_BYTES)
        .context("failed to read candidate channel pointer")?;
    let pointer = catalog::parse_channel_pointer(&pointer_bytes, CANDIDATE_CHANNEL)
        .context("candidate pointer is not a valid candidate channel pointer")?;
    let manifest_source = config
        .manifest_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| pointer.release_manifest_url.clone());
    let manifest_bytes = read_source(&manifest_source, MAX_MANIFEST_BYTES)
        .context("failed to read candidate release manifest")?;
    let manifest = catalog::verify_and_parse_manifest(&manifest_bytes, &pointer)
        .context("candidate manifest failed pointer verification")?;
    let manifest_sha256 = crate::separator::verified_manifest::sha256_hex(&manifest_bytes);

    let runtime = manifest
        .artifacts
        .runtimes
        .iter()
        .find(|runtime| runtime.artifact_id == config.runtime_artifact_id)
        .with_context(|| {
            format!(
                "candidate manifest has no runtime artifact {}",
                config.runtime_artifact_id
            )
        })?;
    let runtime_target = runtime
        .target_triple
        .as_deref()
        .context("candidate runtime has no target triple")?;
    if runtime_target != config.target_triple {
        bail!(
            "runtime {} targets {}, not {}",
            runtime.artifact_id,
            runtime_target,
            config.target_triple
        );
    }
    if !runtime
        .runtime
        .execution_providers
        .iter()
        .any(|provider| provider == config.provider.as_str())
    {
        bail!(
            "runtime {} does not ship provider {}",
            runtime.artifact_id,
            config.provider.as_str()
        );
    }

    let edges = supported_edges(&manifest, &config)?;
    if edges.is_empty() {
        bail!(
            "candidate manifest has no supported edge for runtime {} / target {} / provider {}",
            runtime.artifact_id,
            config.target_triple,
            config.provider.as_str()
        );
    }

    let work_dir = create_private_work_dir(config.work_dir.as_deref())?;
    let _work_guard = WorkDirGuard {
        path: work_dir.clone(),
        remove: !config.report_path.starts_with(&work_dir),
    };
    let profile_dir = work_dir.join("profiles");
    let stems_dir = work_dir.join("stems");
    let models_dir = work_dir.join("models");
    let runtime_dir = work_dir.join("runtime");
    for directory in [&profile_dir, &stems_dir, &models_dir, &runtime_dir] {
        create_fresh_directory(directory)?;
    }

    let prepared_runtime = prepare_runtime(&artifacts_dir, runtime, &runtime_dir)?;
    verify_private_directory(&runtime_dir)?;
    if let Some(loaded) = model::loaded_runtime_path() {
        if loaded != prepared_runtime.library_path.as_path() {
            bail!(
                "a different ONNX Runtime is already loaded from {}; start a fresh process for {}",
                loaded.display(),
                prepared_runtime.library_path.display()
            );
        }
    }
    model::ensure_runtime_loaded_from_path(&prepared_runtime.library_path).with_context(|| {
        format!(
            "failed to load candidate ONNX Runtime from {}",
            prepared_runtime.library_path.display()
        )
    })?;

    let decoded = decode::decode_file(&audio_path)
        .with_context(|| format!("failed to decode audio {}", audio_path.display()))?;
    let normalized = preprocess::normalize_audio_for_model(decoded)
        .context("failed to normalize audio through the application preprocessing path")?;
    if normalized.sample_rate_hz != DEMUCS_SAMPLE_RATE || normalized.channels != DEMUCS_CHANNELS {
        bail!(
            "normalized audio is {} Hz / {} channels; expected {} Hz / {} channels",
            normalized.sample_rate_hz,
            normalized.channels,
            DEMUCS_SAMPLE_RATE,
            DEMUCS_CHANNELS
        );
    }

    let mut measured = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();
    for (edge, model_meta) in &edges {
        if let Some(scope) = non_applicable_scope(model_meta) {
            skipped.push(SkippedEdge {
                model_artifact_id: model_meta.artifact_id.clone(),
                runtime_artifact_id: edge.runtime_artifact_id.clone(),
                target_triple: edge.target_triple.clone(),
                execution_provider: edge.execution_provider.clone(),
                tensor_interface: model_meta.model.tensor_interface.clone(),
                deprecated: model_meta.deprecation.deprecated,
                scope: scope.to_owned(),
            });
            continue;
        }

        let model_dir = models_dir.join(sanitize_filename(&model_meta.artifact_id));
        create_fresh_directory(&model_dir)?;
        let model_path = match prepare_model(&artifacts_dir, model_meta, &model_dir) {
            Ok(path) => path,
            Err(error) => {
                failed.push(format!("{}: {error:#}", model_meta.artifact_id));
                continue;
            }
        };

        match measure_edge(
            edge,
            model_meta,
            runtime,
            &model_path,
            &normalized,
            config.provider,
            &prepared_runtime,
            &profile_dir,
            &stems_dir,
        ) {
            Ok(edge_report) => measured.push(edge_report),
            Err(error) => failed.push(format!("{}: {error:#}", model_meta.artifact_id)),
        }
    }

    if measured.is_empty() {
        bail!(
            "no app-applicable edge could be measured; failed edges: {}",
            failed.join("; ")
        );
    }

    // Non-loadable supported edges are declared, not measured. The report is
    // complete when every app-applicable edge in this group was measured;
    // any failed app-applicable edge keeps the status failed.
    let status = if failed.is_empty() {
        "passed"
    } else {
        "failed"
    };
    let created_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| anyhow::anyhow!("failed to format report timestamp: {error}"))?;
    let report_id = format!(
        "openkara-app-{}-{}-{}-{}-{}-{}",
        manifest.release_id,
        config.target_triple,
        sanitize_filename(&runtime.artifact_id),
        config.provider.as_str(),
        unix_now(),
        std::process::id()
    );
    let report = AppReport {
        schema_version: APP_REPORT_SCHEMA_VERSION,
        report_id,
        created_at,
        candidate_release_id: manifest.release_id.clone(),
        generation: manifest.generation,
        candidate_manifest_sha256: manifest_sha256,
        openkara: OpenKaraIdentity {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            commit: openkara_commit()?,
        },
        target_triple: config.target_triple.clone(),
        status: status.to_owned(),
        edges: measured,
        skipped_edges: skipped.clone(),
    };

    if let Some(parent) = config.report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(&report).context("failed to serialize app report")?;
    fs::write(&config.report_path, json).with_context(|| {
        format!(
            "failed to write app report {}",
            config.report_path.display()
        )
    })?;

    Ok(EvidenceOutcome {
        report_path: config.report_path,
        status: status.to_owned(),
        measured_edges: report.edges.len(),
        not_applicable_edges: skipped
            .iter()
            .map(|edge| {
                format!(
                    "{} (tensor_interface={}, deprecated={})",
                    edge.model_artifact_id, edge.tensor_interface, edge.deprecated
                )
            })
            .collect(),
        failed_edges: failed,
    })
}

/// The application's loadable interface. OpenKara deleted the waveform graph
/// path in issue #172 PR 5; `catalog::resolve_model` also filters deprecated
/// artifacts. A supported manifest edge for a deprecated or waveform artifact
/// is catalog-compatible but not app-applicable.
const APP_LOADABLE_TENSOR_INTERFACE: &str = "spectral-core";

/// Why a supported manifest edge is not app-applicable. `interface` wins when
/// both conditions hold: a waveform artifact is also deprecated in current
/// catalogs, and the interface is the stronger, loader-level reason.
fn non_applicable_scope(model: &catalog::CatalogModel) -> Option<&'static str> {
    if model.model.tensor_interface != APP_LOADABLE_TENSOR_INTERFACE {
        Some("interface")
    } else if model.deprecation.deprecated {
        Some("deprecated")
    } else {
        None
    }
}

fn supported_edges(
    manifest: &catalog::ReleaseManifest,
    config: &EvidenceConfig,
) -> Result<Vec<(catalog::CompatibilityEdge, catalog::CatalogModel)>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut edges = Vec::new();
    for edge in &manifest.compatibility {
        if edge.status != "supported"
            || edge.target_triple != config.target_triple
            || edge.runtime_artifact_id != config.runtime_artifact_id
            || edge.execution_provider != config.provider.as_str()
        {
            continue;
        }
        let key = (
            edge.model_artifact_id.clone(),
            edge.runtime_artifact_id.clone(),
            edge.target_triple.clone(),
            edge.execution_provider.clone(),
        );
        if !seen.insert(key) {
            bail!(
                "candidate manifest has duplicate supported edge for {} / {} / {} / {}",
                edge.model_artifact_id,
                edge.runtime_artifact_id,
                edge.target_triple,
                edge.execution_provider
            );
        }
        let model = manifest
            .artifacts
            .models
            .iter()
            .find(|model| model.artifact_id == edge.model_artifact_id)
            .with_context(|| {
                format!(
                    "candidate edge references missing model {}",
                    edge.model_artifact_id
                )
            })?;
        edges.push((edge.clone(), model.clone()));
    }
    Ok(edges)
}

fn prepare_runtime(
    artifacts_dir: &Path,
    runtime: &catalog::CatalogRuntime,
    dest_dir: &Path,
) -> Result<PreparedRuntime> {
    if runtime.filename.is_empty()
        || runtime.filename.contains('/')
        || runtime.filename.contains('\\')
    {
        bail!(
            "runtime {} filename {:?} is not a plain file name",
            runtime.artifact_id,
            runtime.filename
        );
    }
    let archive = artifacts_dir.join(&runtime.filename);
    verify_file(&archive, runtime.byte_size, &runtime.archive_digest)
        .with_context(|| format!("runtime archive {} failed verification", archive.display()))?;
    let kind = artifacts::archive_kind_for_filename(&runtime.filename)?;
    let extracted = artifacts::extract_archive_safely(&archive, kind, dest_dir)
        .with_context(|| format!("failed to extract runtime archive {}", archive.display()))?;
    artifacts::verify_extracted_files(dest_dir, &runtime.extracted_file_digests, &extracted)
        .context("runtime archive contents differ from the candidate manifest")?;

    let mut libraries = runtime
        .extracted_file_digests
        .iter()
        .filter(|(path, _)| basename(path) == model::ORT_RUNTIME_FILENAME);
    let (relative, digest) = libraries.next().with_context(|| {
        format!(
            "runtime {} declares no {}",
            runtime.artifact_id,
            model::ORT_RUNTIME_FILENAME
        )
    })?;
    if libraries.next().is_some() {
        bail!(
            "runtime {} declares more than one {}",
            runtime.artifact_id,
            model::ORT_RUNTIME_FILENAME
        );
    }
    let library_path = dest_dir.join(relative);
    verify_file(&library_path, digest.size, &digest.sha256).with_context(|| {
        format!(
            "runtime library {} failed verification",
            library_path.display()
        )
    })?;

    Ok(PreparedRuntime {
        library_path,
        library_name: basename(relative).to_owned(),
        library_sha256: digest.sha256.clone(),
    })
}

fn prepare_model(
    artifacts_dir: &Path,
    model: &catalog::CatalogModel,
    dest_dir: &Path,
) -> Result<PathBuf> {
    if model.is_archived() {
        let archive = artifacts_dir.join(&model.filename);
        verify_file(&archive, model.byte_size, &model.archive_digest)
            .with_context(|| format!("model archive {} failed verification", archive.display()))?;
        let kind = artifacts::archive_kind_for_filename(&model.filename)?;
        let extracted = artifacts::extract_archive_safely(&archive, kind, dest_dir)
            .with_context(|| format!("failed to extract model archive {}", archive.display()))?;
        artifacts::verify_extracted_files(dest_dir, &model.extracted_file_digests, &extracted)
            .context("model archive contents differ from the candidate manifest")?;
        let (primary, digest) = model.primary_model_file()?;
        let path = dest_dir.join(primary);
        verify_file(&path, digest.size, &digest.sha256)
            .with_context(|| format!("model file {} failed verification", path.display()))?;
        return Ok(path);
    }

    let (primary, digest) = model.primary_model_file()?;
    let primary_path = artifacts_dir.join(primary);
    let path = if primary_path.is_file() {
        primary_path
    } else {
        artifacts_dir.join(&model.filename)
    };
    verify_file(&path, model.byte_size, &model.archive_digest)
        .with_context(|| format!("model file {} failed verification", path.display()))?;
    verify_file(&path, digest.size, &digest.sha256)
        .with_context(|| format!("model file {} failed declared digest", path.display()))?;
    Ok(path)
}

#[allow(clippy::too_many_arguments)]
fn measure_edge(
    edge: &catalog::CompatibilityEdge,
    model_meta: &catalog::CatalogModel,
    runtime_meta: &catalog::CatalogRuntime,
    model_path: &Path,
    normalized_audio: &decode::DecodedAudio,
    provider: ExecutionProviderPreference,
    prepared_runtime: &PreparedRuntime,
    profile_dir: &Path,
    stems_dir: &Path,
) -> Result<AppEdge> {
    let profile_prefix = profile_dir.join(format!(
        "{}-{}",
        sanitize_filename(&model_meta.artifact_id),
        provider.as_str()
    ));
    let loaded = model::load_from_path_with_profiling(
        model_path,
        provider,
        model::SessionOptions::default(),
        &profile_prefix,
    )
    .with_context(|| {
        format!(
            "failed to load {} with provider {}",
            model_meta.artifact_id,
            provider.as_str()
        )
    })?;
    if loaded.execution_provider != provider {
        bail!(
            "requested provider {} fell back to {}",
            provider.as_str(),
            loaded.execution_provider.as_str()
        );
    }

    let frames = loaded.spectral.segment_frames;
    let window = window_audio(normalized_audio, frames)?;
    let channels = window.channels;
    let stem_dir = stems_dir
        .join(sanitize_filename(&model_meta.artifact_id))
        .join(provider.as_str());
    create_fresh_directory(&stem_dir)?;

    let mut writers = StemWriters {
        mode: StemMode::FourStem,
        vocals: StreamingOggWriter::new(
            &stem_dir.join("vocals.ogg"),
            DEMUCS_SAMPLE_RATE,
            channels,
            None,
            None,
        )
        .context("failed to create vocals writer")?,
        accompaniment: None,
        drums: Some(
            StreamingOggWriter::new(
                &stem_dir.join("drums.ogg"),
                DEMUCS_SAMPLE_RATE,
                channels,
                None,
                None,
            )
            .context("failed to create drums writer")?,
        ),
        bass: Some(
            StreamingOggWriter::new(
                &stem_dir.join("bass.ogg"),
                DEMUCS_SAMPLE_RATE,
                channels,
                None,
                None,
            )
            .context("failed to create bass writer")?,
        ),
        other: Some(
            StreamingOggWriter::new(
                &stem_dir.join("other.ogg"),
                DEMUCS_SAMPLE_RATE,
                channels,
                None,
                None,
            )
            .context("failed to create other writer")?,
        ),
    };

    let mut workspace =
        SeparationWorkspace::new(StemMode::FourStem, channels, frames, frames / 2, frames);
    let cancel = AtomicBool::new(false);
    let (outcome, observations) = inference::separate_streaming_with_observation(
        &loaded,
        &window,
        StemMode::FourStem,
        &mut writers,
        &mut workspace,
        &cancel,
        |_, _| {},
    )
    .with_context(|| {
        format!(
            "failed to separate one window for {} via {}",
            model_meta.artifact_id,
            provider.as_str()
        )
    })?;
    writers
        .finish_all()
        .context("failed to finalize the four stem files")?;

    if outcome.frames_written != frames {
        bail!(
            "streaming path wrote {} frames, expected one full {} frame window",
            outcome.frames_written,
            frames
        );
    }
    if observations.len() != 1 {
        bail!(
            "expected exactly one inference window, observed {}",
            observations.len()
        );
    }
    let observation = observations
        .into_iter()
        .next()
        .context("inference window observation was missing")?;
    if !observation.finite_output {
        bail!("session outputs contained a non-finite value");
    }
    if observation.output_shapes.is_empty()
        || observation
            .output_shapes
            .iter()
            .any(|shape| shape.is_empty() || shape.iter().any(|dim| *dim <= 0))
    {
        bail!("session reported an empty or non-positive output shape");
    }

    let profile_path = loaded
        .end_profiling()?
        .context("ONNX Runtime did not return a profile path")?;
    let counts = parse_profile(Path::new(&profile_path), provider)?;
    if counts.provider == 0 || counts.total == 0 {
        bail!(
            "profile recorded {} total node(s) and {} {} node(s)",
            counts.total,
            counts.provider,
            provider.as_str()
        );
    }
    if provider != ExecutionProviderPreference::Cpu && counts.fallback != 0 {
        bail!(
            "{} edge has {} CPU fallback node(s)",
            provider.as_str(),
            counts.fallback
        );
    }

    let stems = check_stems(&stem_dir)?;

    Ok(AppEdge {
        identity: AppIdentity {
            model_artifact_id: model_meta.artifact_id.clone(),
            runtime_artifact_id: runtime_meta.artifact_id.clone(),
            target_triple: edge.target_triple.clone(),
            execution_provider: provider.as_str().to_owned(),
            model_digest: model_meta.archive_digest.clone(),
            model_size: model_meta.byte_size,
            runtime_digest: runtime_meta.archive_digest.clone(),
            runtime_size: runtime_meta.byte_size,
        },
        loaded_library: LoadedLibrary {
            name: prepared_runtime.library_name.clone(),
            sha256: prepared_runtime.library_sha256.clone(),
        },
        provider: ProviderEvidence {
            requested: provider.as_str().to_owned(),
            assigned: loaded.execution_provider.as_str().to_owned(),
            used_fallback: false,
            provider_node_count: counts.provider,
            total_node_count: counts.total,
            fallback_node_count: counts.fallback,
        },
        output_shapes: observation.output_shapes,
        finite_output: observation.finite_output,
        audio_pipeline: AudioPipeline {
            frames_processed: frames,
            sample_rate: DEMUCS_SAMPLE_RATE,
            stems,
            finite: true,
        },
        checks: Vec::new(),
    })
}

fn window_audio(normalized: &decode::DecodedAudio, frames: usize) -> Result<decode::DecodedAudio> {
    let available = normalized.samples.len() / normalized.channels;
    if available < frames {
        bail!(
            "audio has {available} frames at {} Hz, need at least one full {frames} frame window",
            normalized.sample_rate_hz
        );
    }
    let sample_count = frames * normalized.channels;
    Ok(decode::DecodedAudio {
        sample_rate_hz: normalized.sample_rate_hz,
        channels: normalized.channels,
        duration_ms: ((frames as f64 / normalized.sample_rate_hz as f64) * 1000.0).round() as u64,
        samples: normalized.samples[..sample_count].to_vec(),
    })
}

fn check_stems(stem_dir: &Path) -> Result<usize> {
    let names = ["vocals", "drums", "bass", "other"];
    for name in names {
        let path = stem_dir.join(format!("{name}.ogg"));
        let decoded = decode::decode_file(&path)
            .with_context(|| format!("failed to decode produced stem {}", path.display()))?;
        if decoded.samples.is_empty() {
            bail!("produced stem {} has no samples", path.display());
        }
        if decoded.samples.iter().any(|sample| !sample.is_finite()) {
            bail!(
                "produced stem {} contains a non-finite sample",
                path.display()
            );
        }
    }
    Ok(names.len())
}

fn parse_profile(profile_path: &Path, provider: ExecutionProviderPreference) -> Result<NodeCounts> {
    let bytes = fs::read(profile_path)
        .with_context(|| format!("failed to read ORT profile {}", profile_path.display()))?;
    let events: Vec<serde_json::Value> = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse ORT profile {}", profile_path.display()))?;
    let expected = profile_provider_name(provider);
    let mut total = 0usize;
    let mut provider_nodes = 0usize;
    let mut cpu_nodes = 0usize;
    for event in &events {
        if event.get("cat").and_then(serde_json::Value::as_str) != Some("Node") {
            continue;
        }
        let Some(name) = event
            .get("args")
            .and_then(|args| args.get("provider"))
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        total += 1;
        if name == "CPUExecutionProvider" {
            cpu_nodes += 1;
        }
        if name == expected {
            provider_nodes += 1;
        }
    }
    Ok(NodeCounts {
        total,
        provider: provider_nodes,
        fallback: if provider == ExecutionProviderPreference::Cpu {
            0
        } else {
            cpu_nodes
        },
    })
}

fn profile_provider_name(provider: ExecutionProviderPreference) -> &'static str {
    match provider {
        ExecutionProviderPreference::Cpu => "CPUExecutionProvider",
        ExecutionProviderPreference::CoreMl => "CoreMLExecutionProvider",
        ExecutionProviderPreference::Xnnpack => "XnnpackExecutionProvider",
        ExecutionProviderPreference::DirectMl => "DmlExecutionProvider",
    }
}

fn verify_file(path: &Path, expected_size: u64, expected_sha256: &str) -> Result<()> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to inspect {}", path.display()))?;
    if metadata.len() != expected_size {
        bail!(
            "{} has {} bytes, expected {}",
            path.display(),
            metadata.len(),
            expected_size
        );
    }
    let actual = artifacts::sha256_file(path)?;
    if actual != expected_sha256 {
        bail!(
            "{} has SHA-256 {}, expected {}",
            path.display(),
            actual,
            expected_sha256
        );
    }
    Ok(())
}

fn read_source(source: &str, max_bytes: u64) -> Result<Vec<u8>> {
    if source.starts_with("https://") {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .user_agent("openkara-runtime-only-evidence")
            .build()
            .context("failed to build evidence HTTP client")?;
        let response = client
            .get(source)
            .send()
            .and_then(|response| response.error_for_status())
            .with_context(|| format!("failed to fetch {source}"))?;
        let mut bytes = Vec::new();
        response
            .take(max_bytes + 1)
            .read_to_end(&mut bytes)
            .with_context(|| format!("failed to read {source}"))?;
        if bytes.len() as u64 > max_bytes {
            bail!("{source} exceeded the {max_bytes}-byte bound");
        }
        return Ok(bytes);
    }

    let path = Path::new(source);
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to inspect {}", path.display()))?;
    if metadata.len() > max_bytes {
        bail!(
            "{} is {} bytes, more than the {max_bytes}-byte bound",
            path.display(),
            metadata.len()
        );
    }
    fs::read(path).with_context(|| format!("failed to read {}", path.display()))
}

fn openkara_commit() -> Result<String> {
    // `GIT_BUILD_COMMIT` / `GIT_BUILD_DIRTY` are embedded by build.rs from the
    // source tree that produced this executable. A runtime `git rev-parse` is
    // deliberately not used: it would describe whatever checkout is nearby,
    // not the binary. This is build provenance, not cryptographic attestation.
    let dirty = env!("GIT_BUILD_DIRTY");
    if dirty != "false" {
        bail!(
            "producer binary was built from a dirty or unknown source tree \
             (GIT_BUILD_DIRTY={dirty}); the report commit would not be bound \
             to the executable. Rebuild from a clean commit."
        );
    }
    normalize_commit(env!("GIT_BUILD_COMMIT"))
}

fn normalize_commit(raw: &str) -> Result<String> {
    let value = raw.trim().to_ascii_lowercase();
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("OpenKara commit {raw:?} is not a full 40-character hexadecimal SHA");
    }
    Ok(value)
}

/// Resolve the private work directory and create it exclusively.
///
/// Without `--work-dir` a fresh directory is created under the system temp
/// directory with `create_dir`, so two producers (or tractor an attacker's
/// pre-created symlink) can never share it. No intermediate parents are ever
/// created: every nested directory is created once and then verified to be a
/// real directory, not a symlink, owned by this user.
fn create_private_work_dir(requested: Option<&Path>) -> Result<PathBuf> {
    let work_dir = match requested {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => std::env::current_dir()
            .context("failed to resolve current directory")?
            .join(path),
        None => free_tempdir()?,
    };

    if requested.is_some() {
        fs::create_dir(&work_dir).with_context(|| {
            format!(
                "failed to create work dir {}; it must not already exist",
                work_dir.display()
            )
        })?;
    }
    verify_private_directory(&work_dir)?;
    Ok(work_dir)
}

fn free_tempdir() -> Result<PathBuf> {
    let base = std::env::temp_dir();
    for attempt in 0..16 {
        let candidate = base.join(format!(
            "openkara-runtime-evidence-{}-{}-{}",
            std::process::id(),
            unix_now(),
            attempt
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to create work dir {}", candidate.display()))
            }
        }
    }
    bail!("failed to create a fresh work dir under {}", base.display())
}

/// Create one directory exactly once. An existing path — including any
/// symlink, file, or directory planted by another process — is an error; the
/// producer never reuses an existing tree. Intermediate parents must already
/// exist as real directories created by this producer.
fn create_fresh_directory(path: &Path) -> Result<()> {
    fs::create_dir(path).with_context(|| {
        format!(
            "failed to create fresh directory {}; it must not already exist",
            path.display()
        )
    })?;
    verify_private_directory(path)
}

/// Verify that `path` is a real directory created by this process, not a
/// symlink, and that this user owns it.
fn verify_private_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect work directory {}", path.display()))?;
    if !metadata.is_dir() {
        bail!("work directory {} is not a real directory", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            bail!(
                "work directory {} is owned by uid {}, not the current user",
                path.display(),
                metadata.uid()
            );
        }
        if metadata.mode() & 0o077 != 0 {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(path, permissions)
                .with_context(|| format!("failed to restrict work directory {}", path.display()))?;
        }
    }
    Ok(())
}

fn basename(path: &str) -> &str {
    path.rsplit(|character| character == '/' || character == '\\')
        .next()
        .unwrap_or(path)
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '.' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

/// Remove the producer's private temporary directory on drop. A caller-owned
/// `--work-dir` is never deleted.
struct WorkDirGuard {
    path: PathBuf,
    remove: bool,
}

impl Drop for WorkDirGuard {
    fn drop(&mut self) {
        if self.remove {
            // The loaded runtime may still hold its library open on Windows;
            // cleanup is best-effort and must never mask the producer result.
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(target_os = "macos")]
fn rosetta_translated() -> bool {
    let mut value: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    let name = c"sysctl.proc_translated";
    // SAFETY: `name` is a valid NUL-terminated C string; `value` and `size`
    // point to a live `c_int` and its byte length. sysctlbyname writes at most
    // `size` bytes into `value` and updates `size`.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            &mut value as *mut libc::c_int as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    rc == 0 && value == 1
}

#[cfg(not(target_os = "macos"))]
fn rosetta_translated() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_both_separators() {
        assert_eq!(basename("lib/libonnxruntime.dylib"), "libonnxruntime.dylib");
        assert_eq!(basename("bin\\onnxruntime.dll"), "onnxruntime.dll");
        assert_eq!(basename("onnxruntime.dll"), "onnxruntime.dll");
    }

    #[test]
    fn commit_requires_full_lowercase_sha() {
        let upper = "A".repeat(40);
        assert_eq!(normalize_commit(&upper).unwrap(), "a".repeat(40));
        assert!(normalize_commit("abc123").is_err());
        assert!(normalize_commit(&"g".repeat(40)).is_err());
    }

    #[test]
    fn report_filename_component_is_safe() {
        assert_eq!(
            sanitize_filename("htdemucs.spectral.fp32.onnx"),
            "htdemucs.spectral.fp32.onnx"
        );
        assert_eq!(sanitize_filename("a/b c"), "a_b_c");
    }

    #[test]
    fn window_audio_truncates_to_one_model_window() {
        let audio = decode::DecodedAudio {
            sample_rate_hz: DEMUCS_SAMPLE_RATE,
            channels: DEMUCS_CHANNELS,
            duration_ms: 1000,
            samples: vec![0.25; 4 * DEMUCS_CHANNELS],
        };
        let window = window_audio(&audio, 3).expect("window");
        assert_eq!(window.samples.len(), 3 * DEMUCS_CHANNELS);
        assert_eq!(window.duration_ms, 0);
    }

    #[test]
    fn window_audio_rejects_short_input() {
        let audio = decode::DecodedAudio {
            sample_rate_hz: DEMUCS_SAMPLE_RATE,
            channels: DEMUCS_CHANNELS,
            duration_ms: 1000,
            samples: vec![0.25; 2 * DEMUCS_CHANNELS],
        };
        assert!(window_audio(&audio, 3).is_err());
    }
}
