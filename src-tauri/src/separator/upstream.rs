use crate::config::ModelVariant;
use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;

/// Stable URL that maps each model variant to its newest release tag, download
/// URL, SHA-256, and size. The openkara-models repository updates this file on
/// every release via the `publish-latest-manifest` CI job.
///
/// RATIONALE: Querying the GitHub Releases API at runtime is rate-limited
/// (60 req/hour per IP for unauthenticated clients) and requires parsing
/// release lists + sha256 sidecar files. A static JSON manifest served via
/// raw.githubusercontent.com avoids both problems — one HTTP GET, no auth,
/// no rate limit, 5-minute CDN cache which is negligible for infrequent
/// model releases.
const LATEST_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/thedavidweng/openkara-models/main/latest.json";

#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamModelEntry {
    pub tag: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamManifest {
    pub htdemucs: UpstreamModelEntry,
    pub htdemucs_ft: UpstreamModelEntry,
}

pub fn fetch_upstream_manifest() -> Result<UpstreamManifest> {
    let client = Client::builder()
        .build()
        .context("failed to build HTTP client for upstream manifest")?;
    let response = client
        .get(LATEST_MANIFEST_URL)
        .send()
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("failed to fetch model manifest from {LATEST_MANIFEST_URL}"))?;
    response
        .json::<UpstreamManifest>()
        .with_context(|| format!("failed to parse model manifest from {LATEST_MANIFEST_URL}"))
}

pub fn latest_for_variant(
    manifest: &UpstreamManifest,
    variant: ModelVariant,
) -> &UpstreamModelEntry {
    match variant {
        ModelVariant::Htdemucs => &manifest.htdemucs,
        ModelVariant::HtdemucsFt => &manifest.htdemucs_ft,
    }
}
