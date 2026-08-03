// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn run_runtime_probe_if_requested() {
    if let Some(exit_code) =
        openkara_lib::commands::runtime_bootstrap::runtime_probe_cli_exit_code()
    {
        std::process::exit(exit_code);
    }
}

#[cfg(feature = "automation-smoke")]
fn main() {
    run_runtime_probe_if_requested();
    match openkara_lib::runtime_bootstrap_regression::maybe_run_from_cli() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("OpenKara runtime bootstrap regression failed: {error:#}");
            std::process::exit(1);
        }
    }
    match openkara_lib::automation_smoke::maybe_run_from_cli() {
        Ok(true) => {}
        Ok(false) => openkara_lib::run(),
        Err(error) => {
            eprintln!("OpenKara automation smoke failed: {error:#}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(feature = "automation-smoke"))]
fn main() {
    run_runtime_probe_if_requested();
    openkara_lib::run()
}
