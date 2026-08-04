//! Canonical release evidence and publication gate.
//!
//! This module owns the release policy. CI adapters may collect observations,
//! but they must pass those observations to this module instead of repeating
//! the policy in shell, Node, or PowerShell code.

use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub const SCHEMA_VERSION: u32 = 1;
pub const REQUIRED_RELEASE_TARGETS: [&str; 4] = [
    "windows-x86_64",
    "darwin-aarch64",
    "darwin-x86_64",
    "linux-x86_64",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceSubject {
    pub repository: String,
    pub commit_sha: String,
    pub tag: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AssertionEvidence {
    pub id: String,
    pub status: EvidenceStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactEvidence {
    pub logical_name: String,
    pub target: String,
    pub file_name: String,
    pub byte_size: u64,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updater_signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceFragment {
    pub schema_version: u32,
    pub subject: EvidenceSubject,
    pub platform: String,
    pub scenario: String,
    pub status: EvidenceStatus,
    pub assertions: Vec<AssertionEvidence>,
    pub artifacts: Vec<ArtifactEvidence>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReleaseEvidence {
    pub schema_version: u32,
    pub subject: EvidenceSubject,
    pub status: EvidenceStatus,
    pub fragments: Vec<EvidenceFragment>,
    pub artifacts: Vec<ArtifactEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct UpdaterManifest {
    version: String,
    platforms: BTreeMap<String, UpdaterPlatform>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct UpdaterPlatform {
    signature: String,
    url: String,
}

impl EvidenceFragment {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported evidence schema version {}",
                self.schema_version
            );
        }
        validate_subject(&self.subject)?;
        if self.platform.trim().is_empty() {
            bail!("evidence platform is empty");
        }
        if self.scenario.trim().is_empty() {
            bail!("evidence scenario is empty");
        }
        if self.assertions.is_empty() {
            bail!("evidence fragment has no assertions");
        }

        let mut assertion_ids = BTreeSet::new();
        for assertion in &self.assertions {
            if !assertion_ids.insert(&assertion.id) {
                bail!("duplicate assertion id {}", assertion.id);
            }
            if assertion.id.trim().is_empty() {
                bail!("evidence assertion id is empty");
            }
            if !is_known_assertion_id(&assertion.id) {
                bail!("unknown evidence assertion id {}", assertion.id);
            }
        }

        let has_failed_assertion = self
            .assertions
            .iter()
            .any(|assertion| assertion.status == EvidenceStatus::Failed);
        let has_errors = !self.errors.is_empty();
        if self.status == EvidenceStatus::Passed && (has_failed_assertion || has_errors) {
            bail!(
                "passed evidence fragment {} contains failures",
                self.scenario
            );
        }
        if self.status == EvidenceStatus::Failed && !has_failed_assertion && !has_errors {
            bail!(
                "failed evidence fragment {} has no failed assertion or error",
                self.scenario
            );
        }

        validate_artifacts(&self.artifacts)?;
        for artifact in &self.artifacts {
            if artifact.target != self.platform {
                bail!(
                    "artifact {} targets {} but fragment targets {}",
                    artifact.logical_name,
                    artifact.target,
                    self.platform
                );
            }
        }
        Ok(())
    }
}

impl ReleaseEvidence {
    pub fn aggregate(subject: EvidenceSubject, fragments: Vec<EvidenceFragment>) -> Result<Self> {
        if fragments.is_empty() {
            bail!("release evidence requires at least one fragment");
        }

        let mut fragments = fragments;
        fragments.sort_by(|left, right| {
            left.platform
                .cmp(&right.platform)
                .then_with(|| left.scenario.cmp(&right.scenario))
        });

        let mut fragment_keys = BTreeSet::new();
        let mut platform_counts = BTreeMap::<&str, usize>::new();
        let mut artifacts_by_key = BTreeMap::new();
        for fragment in &fragments {
            fragment.validate()?;
            if fragment.subject != subject {
                bail!(
                    "fragment {} has a different evidence subject",
                    fragment.scenario
                );
            }
            let key = (fragment.platform.as_str(), fragment.scenario.as_str());
            if !fragment_keys.insert(key) {
                bail!(
                    "duplicate evidence fragment for {} / {}",
                    fragment.platform,
                    fragment.scenario
                );
            }
            if !REQUIRED_RELEASE_TARGETS.contains(&fragment.platform.as_str()) {
                bail!("unknown release target {}", fragment.platform);
            }
            *platform_counts
                .entry(fragment.platform.as_str())
                .or_default() += 1;
            for artifact in &fragment.artifacts {
                let artifact_key = (artifact.target.as_str(), artifact.logical_name.as_str());
                if artifacts_by_key.insert(artifact_key, artifact).is_some() {
                    bail!(
                        "duplicate artifact {} for target {}",
                        artifact.logical_name,
                        artifact.target
                    );
                }
            }
        }

        for target in REQUIRED_RELEASE_TARGETS {
            match platform_counts.get(target) {
                None => bail!("missing release evidence for target {target}"),
                Some(1) => {}
                Some(count) => bail!("duplicate release evidence for target {target}: {count}"),
            }
        }

        let status = if fragments
            .iter()
            .all(|fragment| fragment.status == EvidenceStatus::Passed)
        {
            EvidenceStatus::Passed
        } else {
            EvidenceStatus::Failed
        };

        let mut artifacts: Vec<_> = artifacts_by_key.into_values().cloned().collect();
        artifacts.sort_by(|left, right| {
            left.target
                .cmp(&right.target)
                .then_with(|| left.logical_name.cmp(&right.logical_name))
        });

        Ok(Self {
            schema_version: SCHEMA_VERSION,
            subject,
            status,
            fragments,
            artifacts,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported release evidence schema version {}",
                self.schema_version
            );
        }
        let aggregate = Self::aggregate(self.subject.clone(), self.fragments.clone())?;
        if aggregate.status != self.status
            || aggregate.fragments != self.fragments
            || aggregate.artifacts != self.artifacts
        {
            bail!("release evidence does not match its fragments");
        }
        Ok(())
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let contents =
            serde_json::to_string_pretty(self).context("failed to serialize release evidence")?;
        fs::write(path, contents)
            .with_context(|| format!("failed to write release evidence {}", path.display()))?;
        Ok(())
    }

    pub fn write_latest_json(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let mut platforms = BTreeMap::new();
        for artifact in &self.artifacts {
            let Some(signature) = artifact.updater_signature.as_deref() else {
                continue;
            };
            if artifact.file_name.ends_with(".sig") {
                continue;
            }
            let entry = UpdaterPlatform {
                signature: signature.to_owned(),
                url: format!(
                    "https://github.com/{}/releases/download/{}/{}",
                    self.subject.repository, self.subject.tag, artifact.file_name
                ),
            };
            if platforms
                .insert(artifact.target.clone(), entry.clone())
                .is_some()
            {
                bail!(
                    "release evidence has multiple updater payloads for target {}",
                    artifact.target
                );
            }
            if artifact.target == "windows-x86_64"
                && platforms
                    .insert("windows-x86_64-nsis".to_owned(), entry)
                    .is_some()
            {
                bail!(
                    "release evidence has multiple updater payloads for target windows-x86_64-nsis"
                );
            }
        }
        if platforms.is_empty() {
            bail!("release evidence contains no signed updater payloads");
        }
        let manifest = UpdaterManifest {
            version: self.subject.version.clone(),
            platforms,
        };
        let contents =
            serde_json::to_string_pretty(&manifest).context("failed to serialize latest.json")?;
        fs::write(path, contents)
            .with_context(|| format!("failed to write latest.json {}", path.display()))?;
        Ok(())
    }

    pub fn write_checksums(&self, assets_dir: &Path, path: &Path) -> Result<()> {
        self.validate()?;
        let mut seen_file_names = BTreeSet::new();
        let mut lines = Vec::with_capacity(self.artifacts.len());
        for artifact in &self.artifacts {
            if !seen_file_names.insert(&artifact.file_name) {
                bail!(
                    "release evidence contains duplicate asset file name {}",
                    artifact.file_name
                );
            }
            let asset_path = assets_dir.join(&artifact.file_name);
            let actual = artifact_from_file(
                artifact.logical_name.clone(),
                artifact.target.clone(),
                &asset_path,
            )?;
            if actual.byte_size != artifact.byte_size || actual.sha256 != artifact.sha256 {
                bail!(
                    "release asset {} does not match release evidence",
                    artifact.file_name
                );
            }
            lines.push(format!("{}  {}", artifact.sha256, artifact.file_name));
        }
        if lines.is_empty() {
            bail!("release evidence contains no assets to checksum");
        }
        fs::write(path, format!("{}\n", lines.join("\n")))
            .with_context(|| format!("failed to write SHA256SUMS {}", path.display()))?;
        Ok(())
    }

    pub fn verify_target_assets(&self, target: &str, assets_dir: &Path) -> Result<()> {
        self.validate()?;
        let artifacts = self
            .artifacts
            .iter()
            .filter(|artifact| artifact.target == target)
            .collect::<Vec<_>>();
        if artifacts.is_empty() {
            bail!("release evidence contains no assets for target {target}");
        }

        for artifact in artifacts {
            let asset_path = find_asset_file(assets_dir, &artifact.file_name)?;
            let actual = artifact_from_file(
                artifact.logical_name.clone(),
                artifact.target.clone(),
                &asset_path,
            )?;
            if actual.byte_size != artifact.byte_size || actual.sha256 != artifact.sha256 {
                bail!(
                    "release candidate {} does not match release evidence",
                    artifact.file_name
                );
            }
        }
        Ok(())
    }
}

pub fn artifact_from_file(
    logical_name: impl Into<String>,
    target: impl Into<String>,
    path: &Path,
) -> Result<ArtifactEvidence> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to stat release artifact {}", path.display()))?;
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read release artifact {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(ArtifactEvidence {
        logical_name: logical_name.into(),
        target: target.into(),
        file_name: path
            .file_name()
            .context("release artifact has no file name")?
            .to_string_lossy()
            .into_owned(),
        byte_size: metadata.len(),
        sha256: hex_digest(hasher.finalize().as_ref()),
        updater_signature: None,
    })
}

pub fn schema_json() -> Result<String> {
    serde_json::to_string_pretty(&schemars::schema_for!(ReleaseEvidence))
        .context("failed to serialize release evidence schema")
}

fn validate_artifacts(artifacts: &[ArtifactEvidence]) -> Result<()> {
    let mut keys = BTreeSet::new();
    for artifact in artifacts {
        if artifact.logical_name.trim().is_empty() {
            bail!("release artifact logical name is empty");
        }
        if artifact.target.trim().is_empty() {
            bail!("release artifact target is empty");
        }
        if artifact.file_name.trim().is_empty() {
            bail!("release artifact file name is empty");
        }
        if artifact.sha256.len() != 64
            || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!(
                "release artifact {} has an invalid SHA-256",
                artifact.logical_name
            );
        }
        if artifact
            .updater_signature
            .as_deref()
            .is_some_and(str::is_empty)
        {
            bail!(
                "release artifact {} has an empty updater signature",
                artifact.logical_name
            );
        }
        if !keys.insert((artifact.target.as_str(), artifact.logical_name.as_str())) {
            bail!(
                "duplicate artifact {} for target {}",
                artifact.logical_name,
                artifact.target
            );
        }
    }
    Ok(())
}

fn find_asset_file(root: &Path, file_name: &str) -> Result<std::path::PathBuf> {
    let mut matches = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).with_context(|| {
            format!(
                "failed to read release asset directory {}",
                directory.display()
            )
        })? {
            let entry = entry.with_context(|| {
                format!(
                    "failed to inspect release asset directory {}",
                    directory.display()
                )
            })?;
            let path = entry.path();
            if entry
                .file_type()
                .with_context(|| format!("failed to inspect release asset {}", path.display()))?
                .is_dir()
            {
                pending.push(path);
            } else if entry.file_name() == file_name {
                matches.push(path);
            }
        }
    }

    match matches.as_slice() {
        [] => bail!("release candidate asset {} was not found", file_name),
        [path] => Ok(path.clone()),
        _ => bail!("release candidate asset {} is duplicated", file_name),
    }
}

fn validate_subject(subject: &EvidenceSubject) -> Result<()> {
    if subject.repository.trim().is_empty()
        || subject.commit_sha.trim().is_empty()
        || subject.tag.trim().is_empty()
        || subject.version.trim().is_empty()
    {
        bail!("release evidence subject is incomplete");
    }
    Ok(())
}

fn is_known_assertion_id(id: &str) -> bool {
    const EXACT_IDS: &[&str] = &[
        "OKA-284-MODEL-ARCHIVE-DIGEST",
        "OKA-284-MODEL-ARCHIVE-REPORT-CONSISTENCY",
        "OKA-284-MODEL-ARTIFACT-ID",
        "OKA-284-MODEL-CATALOG-SCHEMA",
        "OKA-284-MODEL-COLD-RESTART",
        "OKA-284-MODEL-FILE-DIGEST",
        "OKA-284-MODEL-GENERATION",
        "OKA-284-MODEL-ONNX-REPORT-CONSISTENCY",
        "OKA-284-MODEL-RELEASE-ID",
        "OKA-284-MODEL-VERIFICATION-MANIFEST",
        "OKA-284-RUNTIME-ARCHIVE-DIGEST",
        "OKA-284-RUNTIME-ARCHIVE-REPORT-CONSISTENCY",
        "OKA-284-RUNTIME-ARTIFACT-ID",
        "OKA-284-RUNTIME-CATALOG-SCHEMA",
        "OKA-284-RUNTIME-COLD-RESTART",
        "OKA-284-RUNTIME-FILE-DIGEST",
        "OKA-284-RUNTIME-FILE-DIGEST-CATALOG",
        "OKA-284-RUNTIME-FILE-DIGEST-CROSS",
        "OKA-284-RUNTIME-GENERATION",
        "OKA-284-RUNTIME-INSTALL-PHASES",
        "OKA-284-RUNTIME-LIBRARY-REPORT-CONSISTENCY",
        "OKA-284-RUNTIME-NO-DOWNLOADING-100",
        "OKA-284-RUNTIME-RELEASE-ID",
        "OKA-284-SEPARATION-AFTER-RECOVERY",
        "OKA-284-STALE-DOWNLOAD-RECOVERY",
        "OKA-AUDIO-INPUT-EXISTS",
        "OKA-AUDIO-STEMS-DIFFERENT",
        "OKA-AUDIO-STEMS-EXIST",
        "OKA-MODEL-FIRST-INSTALL",
        "OKA-RUNTIME-FIRST-INSTALL",
        "OKA-SEEK-COUNT",
        "OKA-SEEK-LATENCY-MAX",
        "OKA-SEEK-LATENCY-P95",
        "OKA-SMOKE-DISCOVERED-FILES",
        "OKA-SMOKE-IMPORTED",
        "OKA-SMOKE-MODEL-VERIFIED",
        "OKA-SMOKE-PLAYBACK-FAILURES",
        "OKA-SMOKE-SEPARATION-FAILURES",
        "OKA-SMOKE-SEPARATION-PASSED",
        "OKA-SMOKE-SEPARATION-SKIPPED",
        "OKA-SMOKE-STEMS-PRODUCED",
    ];
    const DYNAMIC_PREFIXES: &[&str] = &[
        "OKA-284-MODEL-ARTIFACT-ID-",
        "OKA-284-MODEL-CATALOG-SCHEMA-",
        "OKA-284-MODEL-GENERATION-",
        "OKA-284-RUNTIME-COMPANION-",
        "OKA-284-FAULT-",
        "OKA-AUDIO-CHANNELS-",
        "OKA-AUDIO-DURATION-",
        "OKA-AUDIO-HEADER-",
        "OKA-AUDIO-NO-NAN-",
        "OKA-AUDIO-NON-SILENT-",
        "OKA-AUDIO-SAMPLE-RATE-",
        "OKA-LOCAL-AUDIO-SMOKE-",
        "OKA-MANAGED-MODEL-PATH-",
        "OKA-MANAGED-MODEL-STATUS-PATH-",
        "OKA-MANAGED-RUNTIME-PATH-",
        "OKA-MODEL-FIRST-INSTALL-",
        "OKA-MODEL-READY-",
        "OKA-PHASE-",
        "OKA-RUNTIME-FIRST-INSTALL-",
        "OKA-RUNTIME-READY-",
    ];

    EXACT_IDS.contains(&id) || DYNAMIC_PREFIXES.iter().any(|prefix| id.starts_with(prefix))
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn subject() -> EvidenceSubject {
        EvidenceSubject {
            repository: "thedavidweng/OpenKara".to_owned(),
            commit_sha: "a".repeat(40),
            tag: "v0.11.0".to_owned(),
            version: "0.11.0".to_owned(),
        }
    }

    fn fragment(platform: &str, scenario: &str, status: EvidenceStatus) -> EvidenceFragment {
        EvidenceFragment {
            schema_version: SCHEMA_VERSION,
            subject: subject(),
            platform: platform.to_owned(),
            scenario: scenario.to_owned(),
            status,
            assertions: vec![AssertionEvidence {
                id: "OKA-PHASE-PREPARE".to_owned(),
                status,
                detail: None,
            }],
            artifacts: vec![],
            errors: if status == EvidenceStatus::Failed {
                vec!["failed".to_owned()]
            } else {
                vec![]
            },
        }
    }

    fn complete_fragments() -> Vec<EvidenceFragment> {
        REQUIRED_RELEASE_TARGETS
            .into_iter()
            .map(|target| fragment(target, "smoke", EvidenceStatus::Passed))
            .collect()
    }

    #[test]
    fn aggregate_requires_matching_subjects_and_unique_fragments() {
        let mut other = fragment("linux-x86_64", "smoke", EvidenceStatus::Passed);
        other.subject.version = "0.11.1".to_owned();
        assert!(ReleaseEvidence::aggregate(
            subject(),
            vec![
                fragment("linux-x86_64", "smoke", EvidenceStatus::Passed),
                other,
                fragment("windows-x86_64", "smoke", EvidenceStatus::Passed),
                fragment("darwin-aarch64", "smoke", EvidenceStatus::Passed),
                fragment("darwin-x86_64", "smoke", EvidenceStatus::Passed),
            ]
        )
        .is_err());

        let mut duplicate = complete_fragments();
        duplicate.push(fragment(
            "linux-x86_64",
            "smoke-retry",
            EvidenceStatus::Passed,
        ));
        assert!(ReleaseEvidence::aggregate(subject(), duplicate).is_err());

        assert!(ReleaseEvidence::aggregate(
            subject(),
            complete_fragments().into_iter().take(3).collect()
        )
        .is_err());
    }

    #[test]
    fn aggregate_normalizes_fragment_order() {
        let mut fragments = complete_fragments();
        fragments.reverse();
        let evidence = ReleaseEvidence::aggregate(subject(), fragments).unwrap();
        let targets = evidence
            .fragments
            .iter()
            .map(|fragment| fragment.platform.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            targets,
            vec![
                "darwin-aarch64",
                "darwin-x86_64",
                "linux-x86_64",
                "windows-x86_64"
            ]
        );
        evidence.validate().unwrap();
    }

    #[test]
    fn failed_assertion_cannot_be_marked_passed() {
        let mut value = fragment("linux-x86_64", "smoke", EvidenceStatus::Passed);
        value.assertions[0].status = EvidenceStatus::Failed;
        assert!(value.validate().is_err());
    }

    #[test]
    fn unknown_assertion_id_is_rejected() {
        let mut value = fragment("linux-x86_64", "smoke", EvidenceStatus::Passed);
        value.assertions[0].id = "OKA-UNKNOWN".to_owned();
        assert!(value.validate().is_err());
    }

    #[test]
    fn artifact_identity_is_calculated_from_exact_bytes() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("candidate.AppImage");
        fs::write(&path, b"candidate").unwrap();
        let artifact = artifact_from_file("appimage", "x86_64-unknown-linux-gnu", &path).unwrap();
        assert_eq!(artifact.byte_size, 9);
        assert_eq!(
            artifact.sha256,
            "dda18a0e21ae47c53b4309434cbc02ae8bf764fa83a6defbb719431242722aa7"
        );
    }

    #[test]
    fn latest_json_is_generated_from_signed_evidence() {
        let directory = tempdir().unwrap();
        let payload = directory.path().join("OpenKara_0.11.0_x64-setup.exe");
        fs::write(&payload, b"candidate").unwrap();
        let mut artifact = artifact_from_file("installer", "windows-x86_64", &payload).unwrap();
        artifact.updater_signature = Some("signed-payload".to_owned());
        let mut value = fragment("windows-x86_64", "smoke", EvidenceStatus::Passed);
        value.artifacts.push(artifact);
        let mut fragments = complete_fragments();
        fragments[0] = value;
        let evidence = ReleaseEvidence::aggregate(subject(), fragments).unwrap();
        let output = directory.path().join("latest.json");

        evidence.write_latest_json(&output).unwrap();

        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(output).unwrap()).unwrap();
        assert_eq!(manifest["version"], "0.11.0");
        assert_eq!(
            manifest["platforms"]["windows-x86_64"]["signature"],
            "signed-payload"
        );
        assert_eq!(
            manifest["platforms"]["windows-x86_64-nsis"]["url"],
            "https://github.com/thedavidweng/OpenKara/releases/download/v0.11.0/OpenKara_0.11.0_x64-setup.exe"
        );
    }

    #[test]
    fn checksums_are_generated_only_for_matching_evidence_bytes() {
        let directory = tempdir().unwrap();
        let payload = directory.path().join("OpenKara_0.11.0.AppImage");
        fs::write(&payload, b"candidate").unwrap();
        let artifact = artifact_from_file("appimage", "linux-x86_64", &payload).unwrap();
        let mut value = fragment("linux-x86_64", "smoke", EvidenceStatus::Passed);
        value.artifacts.push(artifact);
        let mut fragments = complete_fragments();
        fragments[3] = value;
        let evidence = ReleaseEvidence::aggregate(subject(), fragments).unwrap();
        let output = directory.path().join("SHA256SUMS");

        evidence.write_checksums(directory.path(), &output).unwrap();

        assert_eq!(
            fs::read_to_string(output).unwrap(),
            "dda18a0e21ae47c53b4309434cbc02ae8bf764fa83a6defbb719431242722aa7  OpenKara_0.11.0.AppImage\n"
        );
    }

    #[test]
    fn target_candidates_are_verified_against_exact_evidence_bytes() {
        let directory = tempdir().unwrap();
        let candidate_dir = directory.path().join("candidate").join("installer");
        fs::create_dir_all(&candidate_dir).unwrap();
        let payload = candidate_dir.join("OpenKara_0.11.0_x64-setup.exe");
        fs::write(&payload, b"candidate").unwrap();

        let artifact = artifact_from_file("installer", "windows-x86_64", &payload).unwrap();
        let mut value = fragment("windows-x86_64", "smoke", EvidenceStatus::Passed);
        value.artifacts.push(artifact);
        let mut fragments = complete_fragments();
        fragments[0] = value;
        let evidence = ReleaseEvidence::aggregate(subject(), fragments).unwrap();

        evidence
            .verify_target_assets("windows-x86_64", directory.path())
            .unwrap();
        fs::write(&payload, b"changed").unwrap();
        assert!(evidence
            .verify_target_assets("windows-x86_64", directory.path())
            .is_err());
    }
}
