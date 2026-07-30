use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssertionResult {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationIdentity {
    pub name: String,
    pub version: String,
    pub commit_sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub os_version: String,
    pub webview2_version: String,
    pub selected_execution_provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub name: String,
    pub status: StepStatus,
    pub started_at: i64,
    pub finished_at: i64,
    pub duration_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assertion {
    pub id: String,
    pub expected: String,
    pub observed: String,
    pub result: AssertionResult,
    pub artifact_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeIdentity {
    pub archive_sha256: String,
    pub extracted_library_sha256: String,
    pub companion_dll_sha256s: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelIdentity {
    pub archive_sha256: String,
    pub extracted_onnx_sha256: String,
    pub verification_manifest: String,
    pub catalog_generation: String,
    pub release_id: String,
    pub artifact_id: String,
    pub selected_variant: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSummary {
    pub schema_version: i64,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilitySummary {
    pub violations_count: i64,
    pub keyboard_trap_count: i64,
    pub ui_automation_errors_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom_levels_tested: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSummary {
    pub sample_rate: i64,
    pub channel_count: i64,
    pub non_silent_samples: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_duration_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_duration_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_delta_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocals_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accompaniment_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assertion_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationReport {
    pub scenario: String,
    pub status: ReportStatus,
    pub started_at: i64,
    pub finished_at: i64,
    pub duration_ms: i64,
    pub application: ApplicationIdentity,
    pub environment: Environment,
    pub steps: Vec<Step>,
    pub assertions: Vec<Assertion>,
    pub artifacts: Vec<Artifact>,
    pub runtime: RuntimeIdentity,
    pub model: ModelIdentity,
    pub database: DatabaseSummary,
    pub accessibility: AccessibilitySummary,
    pub audio: AudioSummary,
    pub errors: Vec<ReportError>,
}

impl AutomationReport {
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        std::fs::write(
            path,
            serde_json::to_string_pretty(self)
                .with_context(|| "failed to serialize automation report")?,
        )
        .with_context(|| format!("failed to write automation report {}", path.display()))?;
        Ok(())
    }

    pub fn report_path(output_dir: &Path) -> PathBuf {
        output_dir.join("automation-report.json")
    }
}
