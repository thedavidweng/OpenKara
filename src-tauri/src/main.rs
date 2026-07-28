// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "automation-smoke")]
fn main() {
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
    openkara_lib::run()
}
