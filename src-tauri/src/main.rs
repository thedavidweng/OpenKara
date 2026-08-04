// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = run_app_or_worker() {
        eprintln!("OpenKara startup command failed: {error:#}");
        std::process::exit(1);
    }
}

fn run_app_or_worker() -> anyhow::Result<()> {
    if openkara_lib::commands::runtime_worker::maybe_run_from_cli()? {
        return Ok(());
    }

    #[cfg(feature = "automation-smoke")]
    if openkara_lib::runtime_bootstrap_regression::maybe_run_from_cli()? {
        return Ok(());
    }

    #[cfg(feature = "automation-smoke")]
    if openkara_lib::automation_smoke::maybe_run_from_cli()? {
        return Ok(());
    }

    openkara_lib::run();
    Ok(())
}
