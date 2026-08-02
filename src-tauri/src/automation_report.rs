use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

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

#[derive(Debug, Clone, Serialize)]
pub struct AutomationValidationReport {
    pub generated_at: i64,
    pub source_report: String,
    pub assertions: Vec<Assertion>,
    pub pass_count: usize,
    pub fail_count: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DesktopAssertionResult {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DesktopE2eAssertion {
    pub id: String,
    pub expected: String,
    pub observed: String,
    pub result: DesktopAssertionResult,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DesktopE2eReport {
    pub scenario: String,
    pub status: ReportStatus,
    pub assertions: Vec<DesktopE2eAssertion>,
}

impl DesktopE2eReport {
    pub fn validate_release_gate(&self, expected_scenario: &str) -> anyhow::Result<()> {
        if self.scenario.trim().is_empty() {
            anyhow::bail!("desktop E2E report scenario must be a non-empty string");
        }
        if self.scenario != expected_scenario {
            anyhow::bail!(
                "desktop E2E report scenario mismatch: expected {}, found {}",
                expected_scenario,
                self.scenario
            );
        }
        if self.assertions.is_empty() {
            anyhow::bail!("desktop E2E report assertions must be a non-empty array");
        }

        let mut assertion_ids = BTreeSet::new();
        let mut failed_assertions = 0;
        for assertion in &self.assertions {
            if assertion.id.trim().is_empty()
                || assertion.expected.trim().is_empty()
                || assertion.observed.trim().is_empty()
            {
                anyhow::bail!(
                    "desktop E2E assertions require non-empty id, expected, and observed values"
                );
            }
            if !assertion_ids.insert(&assertion.id) {
                anyhow::bail!(
                    "desktop E2E report contains duplicate assertion {}",
                    assertion.id
                );
            }
            if assertion.result == DesktopAssertionResult::Fail {
                failed_assertions += 1;
            }
        }

        if failed_assertions > 0 {
            anyhow::bail!("desktop E2E report contains {failed_assertions} failed assertions");
        }
        if self.status != ReportStatus::Passed {
            anyhow::bail!(
                "desktop E2E report status is {:?}; expected passed",
                self.status
            );
        }

        Ok(())
    }
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

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.assertions.is_empty() {
            anyhow::bail!("automation report assertions must be a non-empty array");
        }
        let mut assertion_ids = BTreeSet::new();
        for assertion in &self.assertions {
            if assertion.id.trim().is_empty() {
                anyhow::bail!("automation report contains an assertion with an empty id");
            }
            if !assertion_ids.insert(&assertion.id) {
                anyhow::bail!(
                    "automation report contains duplicate assertion {}",
                    assertion.id
                );
            }
        }

        let failed_assertions = self
            .assertions
            .iter()
            .filter(|assertion| assertion.result == AssertionResult::Fail)
            .count();
        match self.status {
            ReportStatus::Passed if failed_assertions > 0 || !self.errors.is_empty() => {
                anyhow::bail!(
                    "passed automation report contains {} failed assertions and {} errors",
                    failed_assertions,
                    self.errors.len()
                );
            }
            ReportStatus::Failed if failed_assertions == 0 && self.errors.is_empty() => {
                anyhow::bail!("failed automation report contains no failure evidence");
            }
            ReportStatus::Skipped => anyhow::bail!("automation report is skipped"),
            ReportStatus::Passed | ReportStatus::Failed => {}
        }

        Ok(())
    }

    pub fn validation_report(&self, source_report: &Path) -> AutomationValidationReport {
        let pass_count = self
            .assertions
            .iter()
            .filter(|assertion| assertion.result == AssertionResult::Pass)
            .count();
        let fail_count = self
            .assertions
            .iter()
            .filter(|assertion| assertion.result == AssertionResult::Fail)
            .count();

        AutomationValidationReport {
            generated_at: self.finished_at,
            source_report: source_report.display().to_string(),
            assertions: self.assertions.clone(),
            pass_count,
            fail_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(
        status: ReportStatus,
        assertions: Vec<Assertion>,
        errors: Vec<ReportError>,
    ) -> AutomationReport {
        AutomationReport {
            scenario: "test".to_owned(),
            status,
            started_at: 1,
            finished_at: 2,
            duration_ms: 1,
            application: ApplicationIdentity {
                name: "openkara".to_owned(),
                version: "0.0.0".to_owned(),
                commit_sha: "test".to_owned(),
            },
            environment: Environment {
                os_version: "test".to_owned(),
                webview2_version: "test".to_owned(),
                selected_execution_provider: "cpu".to_owned(),
                locale: None,
                theme: None,
            },
            steps: Vec::new(),
            assertions,
            artifacts: Vec::new(),
            runtime: RuntimeIdentity {
                archive_sha256: String::new(),
                extracted_library_sha256: String::new(),
                companion_dll_sha256s: Vec::new(),
            },
            model: ModelIdentity {
                archive_sha256: String::new(),
                extracted_onnx_sha256: String::new(),
                verification_manifest: String::new(),
                catalog_generation: String::new(),
                release_id: String::new(),
                artifact_id: String::new(),
                selected_variant: String::new(),
            },
            database: DatabaseSummary {
                schema_version: 1,
                path: "test".to_owned(),
            },
            accessibility: AccessibilitySummary {
                violations_count: 0,
                keyboard_trap_count: 0,
                ui_automation_errors_count: 0,
                zoom_levels_tested: None,
            },
            audio: AudioSummary {
                sample_rate: 0,
                channel_count: 0,
                non_silent_samples: false,
                input_duration_seconds: None,
                output_duration_seconds: None,
                duration_delta_seconds: None,
                vocals_path: None,
                accompaniment_path: None,
            },
            errors,
        }
    }

    fn assertion(id: &str, result: AssertionResult) -> Assertion {
        Assertion {
            id: id.to_owned(),
            expected: "expected".to_owned(),
            observed: "observed".to_owned(),
            result,
            artifact_path: String::new(),
        }
    }

    #[test]
    fn validation_accepts_a_complete_passed_report() {
        let value = report(
            ReportStatus::Passed,
            vec![assertion("OKA-TEST", AssertionResult::Pass)],
            Vec::new(),
        );

        assert!(value.validate().is_ok());
        let validation = value.validation_report(Path::new("automation-report.json"));
        assert_eq!(validation.pass_count, 1);
        assert_eq!(validation.fail_count, 0);
    }

    #[test]
    fn validation_rejects_failed_status_without_failure_evidence() {
        let value = report(
            ReportStatus::Failed,
            vec![assertion("OKA-TEST", AssertionResult::Pass)],
            Vec::new(),
        );

        assert!(value.validate().is_err());
    }

    #[test]
    fn validation_rejects_duplicate_assertion_ids() {
        let value = report(
            ReportStatus::Passed,
            vec![
                assertion("OKA-TEST", AssertionResult::Pass),
                assertion("OKA-TEST", AssertionResult::Pass),
            ],
            Vec::new(),
        );

        assert!(value.validate().is_err());
    }

    fn desktop_report(status: ReportStatus, result: DesktopAssertionResult) -> DesktopE2eReport {
        DesktopE2eReport {
            scenario: "keyboard-workflow".to_owned(),
            status,
            assertions: vec![DesktopE2eAssertion {
                id: "UIA-READY".to_owned(),
                expected: "ready".to_owned(),
                observed: "ready".to_owned(),
                result,
            }],
        }
    }

    #[test]
    fn desktop_validation_accepts_a_complete_passed_report() {
        assert!(
            desktop_report(ReportStatus::Passed, DesktopAssertionResult::Pass)
                .validate_release_gate("keyboard-workflow")
                .is_ok()
        );
    }

    #[test]
    fn desktop_validation_rejects_failed_assertions_and_statuses() {
        assert!(
            desktop_report(ReportStatus::Passed, DesktopAssertionResult::Fail)
                .validate_release_gate("keyboard-workflow")
                .is_err()
        );
        assert!(
            desktop_report(ReportStatus::Failed, DesktopAssertionResult::Pass)
                .validate_release_gate("keyboard-workflow")
                .is_err()
        );
    }

    #[test]
    fn desktop_validation_rejects_wrong_scenario_and_empty_assertions() {
        let mut report = desktop_report(ReportStatus::Passed, DesktopAssertionResult::Pass);
        assert!(report.validate_release_gate("installed-workflow").is_err());
        report.assertions.clear();
        assert!(report.validate_release_gate("keyboard-workflow").is_err());
    }
}
