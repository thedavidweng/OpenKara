use anyhow::{bail, Context, Result};
use openkara_lib::automation_driver::{run_scenario, ScenarioConfig};
use openkara_lib::automation_faults::FaultScenario;
use std::env;
use std::path::PathBuf;

fn main() {
    if let Some(exit_code) =
        openkara_lib::commands::runtime_bootstrap::runtime_probe_cli_exit_code()
    {
        std::process::exit(exit_code);
    }
    if let Err(error) = run() {
        eprintln!("OpenKara automation driver failed: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let config = parse_config(env::args())?;

    let report = run_scenario(&config).context("scenario failed")?;
    let report_path = config.output_dir.join("automation-report.json");
    report.write(&report_path)?;

    match report.status {
        openkara_lib::automation_report::ReportStatus::Passed => Ok(()),
        _ => bail!("scenario {} did not pass", report.scenario),
    }
}

fn parse_config(mut arguments: impl Iterator<Item = String>) -> Result<ScenarioConfig> {
    let _program = arguments.next();

    let mut scenario = None;
    let mut app_data_dir = None;
    let mut input_dir = None;
    let mut output_dir = None;
    let mut installed_exe = None;
    let mut execution_provider = None;
    let mut locale = None;
    let mut theme = None;
    let mut inject_faults = false;

    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .with_context(|| format!("missing value for {argument}"))
        };
        match argument.as_str() {
            "--scenario" => scenario = Some(value()?),
            "--app-data-dir" => app_data_dir = Some(PathBuf::from(value()?)),
            "--input-dir" => input_dir = Some(PathBuf::from(value()?)),
            "--output-dir" => output_dir = Some(PathBuf::from(value()?)),
            "--installed-exe" => installed_exe = Some(PathBuf::from(value()?)),
            "--execution-provider" => execution_provider = Some(value()?),
            "--locale" => locale = Some(value()?),
            "--theme" => theme = Some(value()?),
            "--inject-faults" => inject_faults = true,
            _ => bail!("unknown argument {argument}"),
        }
    }

    let scenario = scenario.context("--scenario is required")?;
    let injected_faults =
        if inject_faults || scenario == "fault-injection" || scenario.ends_with("-faults") {
            FaultScenario::recovery_suite()
        } else {
            Vec::new()
        };

    Ok(ScenarioConfig {
        scenario,
        app_data_dir: app_data_dir.context("--app-data-dir is required")?,
        input_dir: input_dir.context("--input-dir is required")?,
        output_dir: output_dir.context("--output-dir is required")?,
        installed_exe,
        execution_provider,
        locale,
        theme,
        seek_iterations: 32,
        injected_faults,
    })
}
