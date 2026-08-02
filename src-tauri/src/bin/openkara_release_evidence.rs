use anyhow::{bail, Context, Result};
use openkara_lib::automation_report::{
    AssertionResult, AutomationReport, DesktopE2eReport, ReportStatus,
};
use openkara_lib::release_evidence::{
    artifact_from_file, schema_json, AssertionEvidence, EvidenceFragment, EvidenceStatus,
    EvidenceSubject, ReleaseEvidence,
};
use openkara_lib::smoke::LocalAudioSmokeReport;
use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn main() {
    if let Err(error) = run() {
        eprintln!("OpenKara release evidence failed: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("schema") => {
            let output = arguments
                .next()
                .map(PathBuf::from)
                .context("schema requires an output path")?;
            fs::write(&output, schema_json()?)
                .with_context(|| format!("failed to write schema {}", output.display()))?;
            Ok(())
        }
        Some("validate-fragment") => {
            let input = next_path(&mut arguments, "fragment")?;
            let fragment: EvidenceFragment = read_json(&input)?;
            fragment.validate()
        }
        Some("validate-automation-report") => validate_automation_report(arguments.collect()),
        Some("validate-desktop-e2e") => validate_desktop_e2e(arguments.collect()),
        Some("validate-local-audio-smoke") => validate_local_audio_smoke(arguments.collect()),
        Some("fragment-from-automation-report") => {
            fragment_from_automation_report(arguments.collect())
        }
        Some("aggregate") => aggregate_command(arguments.collect()),
        Some("verify-assets") => verify_assets_command(arguments.collect()),
        Some("latest") => latest_command(arguments.collect()),
        Some("checksums") => checksums_command(arguments.collect()),
        Some(command) => bail!("unknown release evidence command {command}"),
        None => bail!("a release evidence command is required"),
    }
}

fn verify_assets_command(arguments: Vec<String>) -> Result<()> {
    let evidence_path = named_path(&arguments, "evidence")?;
    let target = named_value(&arguments, "target")?;
    let assets_dir = named_path(&arguments, "assets-dir")?;
    let evidence: ReleaseEvidence = read_json(&evidence_path)?;
    evidence.verify_target_assets(&target, &assets_dir)
}

fn validate_automation_report(arguments: Vec<String>) -> Result<()> {
    let input = named_path(&arguments, "input")?;
    let output = named_path(&arguments, "output")?;
    let report: AutomationReport = read_json(&input)?;
    report.validate()?;

    let validation = report.validation_report(&input);
    fs::write(&output, serde_json::to_string_pretty(&validation)?).with_context(|| {
        format!(
            "failed to write automation validation report {}",
            output.display()
        )
    })?;

    if report.status != ReportStatus::Passed {
        bail!(
            "automation report {} did not pass ({} failed assertions)",
            input.display(),
            validation.fail_count
        );
    }

    Ok(())
}

fn validate_local_audio_smoke(arguments: Vec<String>) -> Result<()> {
    let input = named_path(&arguments, "input")?;
    let report: LocalAudioSmokeReport = read_json(&input)?;
    report.validate_release_gate()
}

fn validate_desktop_e2e(arguments: Vec<String>) -> Result<()> {
    let input = named_path(&arguments, "input")?;
    let scenario = named_value(&arguments, "scenario")?;
    let report: DesktopE2eReport = read_json(&input)?;
    report.validate_release_gate(&scenario)
}

fn aggregate_command(arguments: Vec<String>) -> Result<()> {
    let mut subject = None;
    let mut fragments = Vec::new();
    let mut updater_signatures = BTreeMap::new();
    let mut output = None;
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--repository" => {
                subject_mut(&mut subject).repository =
                    next_value(&arguments, &mut index, "repository")?;
            }
            "--commit-sha" => {
                subject_mut(&mut subject).commit_sha =
                    next_value(&arguments, &mut index, "commit-sha")?;
            }
            "--tag" => {
                subject_mut(&mut subject).tag = next_value(&arguments, &mut index, "tag")?;
            }
            "--version" => {
                subject_mut(&mut subject).version = next_value(&arguments, &mut index, "version")?;
            }
            "--fragment" => {
                let path = next_value(&arguments, &mut index, "fragment")?;
                fragments.push(read_json::<EvidenceFragment>(
                    PathBuf::from(path).as_path(),
                )?);
            }
            "--artifact" => {
                let value = next_value(&arguments, &mut index, "artifact")?;
                let (logical_name, target, path) = parse_artifact_spec(&value)?;
                let path = PathBuf::from(path);
                let artifact = artifact_from_file(logical_name, target, &path)?;
                record_updater_signature(&mut updater_signatures, &artifact, &path)?;
                if let Some(fragment) = fragments.last_mut() {
                    fragment.artifacts.push(artifact);
                } else {
                    bail!("--artifact must follow a --fragment");
                }
            }
            "--output" => {
                output = Some(PathBuf::from(next_value(&arguments, &mut index, "output")?));
            }
            value => bail!("unknown aggregate argument {value}"),
        }
        index += 1;
    }

    let subject = subject.context("aggregate requires --commit-sha, --tag, and --version")?;
    if subject.commit_sha.is_empty() || subject.tag.is_empty() || subject.version.is_empty() {
        bail!("aggregate requires --commit-sha, --tag, and --version");
    }
    for fragment in &mut fragments {
        attach_updater_signatures(&mut fragment.artifacts, &updater_signatures)?;
    }
    let evidence = ReleaseEvidence::aggregate(subject, fragments)?;
    let output = output.context("aggregate requires --output")?;
    evidence.write(&output)
}

fn fragment_from_automation_report(arguments: Vec<String>) -> Result<()> {
    let input = named_path(&arguments, "input")?;
    let output = named_path(&arguments, "output")?;
    let report: AutomationReport = read_json(&input)?;
    let subject = EvidenceSubject {
        repository: named_value(&arguments, "repository")?,
        commit_sha: named_value(&arguments, "commit-sha")?,
        tag: named_value(&arguments, "tag")?,
        version: named_value(&arguments, "version")?,
    };
    let platform = named_value(&arguments, "platform")?;
    let scenario = named_value(&arguments, "scenario")?;
    let mut artifacts = Vec::new();
    let mut updater_signatures = BTreeMap::new();
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == "--artifact" {
            let value = arguments
                .get(index + 1)
                .context("artifact requires a value")?;
            let (logical_name, target, path) = parse_artifact_spec(value)?;
            let path = PathBuf::from(path);
            let artifact = artifact_from_file(logical_name, target, &path)?;
            record_updater_signature(&mut updater_signatures, &artifact, &path)?;
            artifacts.push(artifact);
            index += 1;
        }
        index += 1;
    }
    attach_updater_signatures(&mut artifacts, &updater_signatures)?;
    let status = match report.status {
        ReportStatus::Passed => EvidenceStatus::Passed,
        ReportStatus::Failed | ReportStatus::Skipped => EvidenceStatus::Failed,
    };
    let assertions = report
        .assertions
        .into_iter()
        .map(|assertion| AssertionEvidence {
            id: assertion.id,
            status: match assertion.result {
                AssertionResult::Pass => EvidenceStatus::Passed,
                AssertionResult::Fail => EvidenceStatus::Failed,
            },
            detail: Some(format!(
                "expected: {}; observed: {}",
                assertion.expected, assertion.observed
            )),
        })
        .collect();
    let errors = report
        .errors
        .into_iter()
        .map(|error| error.message)
        .collect();
    let fragment = EvidenceFragment {
        schema_version: openkara_lib::release_evidence::SCHEMA_VERSION,
        subject,
        platform,
        scenario,
        status,
        assertions,
        artifacts,
        errors,
    };
    fragment.validate()?;
    fs::write(&output, serde_json::to_string_pretty(&fragment)?)
        .with_context(|| format!("failed to write evidence fragment {}", output.display()))?;
    Ok(())
}

fn latest_command(arguments: Vec<String>) -> Result<()> {
    let evidence_path = named_path(&arguments, "evidence")?;
    let output = named_path(&arguments, "output")?;
    let evidence: ReleaseEvidence = read_json(&evidence_path)?;
    evidence.write_latest_json(&output)
}

fn checksums_command(arguments: Vec<String>) -> Result<()> {
    let evidence_path = named_path(&arguments, "evidence")?;
    let assets_dir = named_path(&arguments, "assets-dir")?;
    let output = named_path(&arguments, "output")?;
    let evidence: ReleaseEvidence = read_json(&evidence_path)?;
    evidence.write_checksums(&assets_dir, &output)
}

fn record_updater_signature(
    signatures: &mut BTreeMap<(String, String), String>,
    artifact: &openkara_lib::release_evidence::ArtifactEvidence,
    path: &std::path::Path,
) -> Result<()> {
    if !artifact.file_name.ends_with(".sig") {
        return Ok(());
    }
    let candidate_name = artifact
        .file_name
        .strip_suffix(".sig")
        .context("updater signature has no payload file name")?;
    let signature = fs::read_to_string(path)
        .with_context(|| format!("failed to read updater signature {}", path.display()))?
        .trim()
        .to_owned();
    if signature.is_empty() {
        bail!("updater signature {} is empty", artifact.file_name);
    }
    let key = (artifact.target.clone(), candidate_name.to_owned());
    if signatures.insert(key, signature).is_some() {
        bail!("duplicate updater signature for {}", candidate_name);
    }
    Ok(())
}

fn attach_updater_signatures(
    artifacts: &mut [openkara_lib::release_evidence::ArtifactEvidence],
    signatures: &BTreeMap<(String, String), String>,
) -> Result<()> {
    for artifact in artifacts {
        let key = (artifact.target.clone(), artifact.file_name.clone());
        if let Some(signature) = signatures.get(&key) {
            if artifact
                .updater_signature
                .replace(signature.clone())
                .is_some()
            {
                bail!("duplicate updater signature for {}", artifact.file_name);
            }
        }
    }
    Ok(())
}

fn named_path(arguments: &[String], name: &str) -> Result<PathBuf> {
    named_value(arguments, name).map(PathBuf::from)
}

fn named_value(arguments: &[String], name: &str) -> Result<String> {
    let flag = format!("--{name}");
    let index = arguments
        .iter()
        .position(|argument| argument == &flag)
        .with_context(|| format!("{name} requires a value"))?;
    arguments
        .get(index + 1)
        .cloned()
        .with_context(|| format!("{name} requires a value"))
}

fn next_path(arguments: &mut impl Iterator<Item = String>, name: &str) -> Result<PathBuf> {
    arguments
        .next()
        .map(PathBuf::from)
        .with_context(|| format!("{name} requires a path"))
}

fn next_value(arguments: &[String], index: &mut usize, name: &str) -> Result<String> {
    *index += 1;
    arguments
        .get(*index)
        .cloned()
        .with_context(|| format!("{name} requires a value"))
}

fn parse_artifact_spec(value: &str) -> Result<(String, String, String)> {
    let mut parts = value.splitn(3, ':');
    let logical_name = parts
        .next()
        .filter(|value| !value.is_empty())
        .context("artifact must use logical_name:target:path")?;
    let target = parts
        .next()
        .filter(|value| !value.is_empty())
        .context("artifact must use logical_name:target:path")?;
    let path = parts
        .next()
        .filter(|value| !value.is_empty())
        .context("artifact must use logical_name:target:path")?;
    Ok((logical_name.to_owned(), target.to_owned(), path.to_owned()))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read JSON {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse JSON {}", path.display()))
}

fn subject_mut(subject: &mut Option<EvidenceSubject>) -> &mut EvidenceSubject {
    subject.get_or_insert_with(default_subject)
}

fn default_subject() -> EvidenceSubject {
    EvidenceSubject {
        repository: "thedavidweng/OpenKara".to_owned(),
        commit_sha: String::new(),
        tag: String::new(),
        version: String::new(),
    }
}
