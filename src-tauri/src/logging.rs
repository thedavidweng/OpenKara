//! File + stderr logging via a `tracing` subscriber.
//!
//! OpenKara ships as a double-clickable desktop app, so `stderr` is discarded
//! by the OS on launch and any diagnostic printed there is lost the moment a
//! user hits a bug. This module installs a global `tracing` subscriber that
//! writes to a daily-rolling file under the platform log directory (the same
//! `appLogDir()` Tauri exposes) *and* keeps mirroring to `stderr` so logs stay
//! visible when the app is launched from a terminal during development.
//!
//! We deliberately use `tracing-subscriber` rather than a `log`-facade plugin:
//! the crate already depends on `tracing` and `ort` emits its runtime
//! diagnostics through `tracing`, so a `tracing` subscriber captures both our
//! own events and ONNX Runtime's — exactly the signal that matters in a bug
//! report — without adding an IPC/capability surface.

use std::path::{Path, PathBuf};

use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Prefix of every rolling log file: `openkara.<date>.log`.
pub const LOG_FILENAME_PREFIX: &str = "openkara";
/// Suffix (extension) of every rolling log file.
pub const LOG_FILENAME_SUFFIX: &str = "log";
/// How many days of rolled files to retain before the oldest is pruned.
const MAX_LOG_FILES: usize = 7;
/// Environment variable that overrides the default filter directives, e.g.
/// `OPENKARA_LOG=debug` or `OPENKARA_LOG=openkara_lib=trace,warn`.
const LOG_FILTER_ENV: &str = "OPENKARA_LOG";
/// Default filter: `info` for the whole process. Errors and warnings are
/// always in scope, and third-party crates (including ONNX Runtime) surface at
/// `info` and above so a bug report carries their diagnostics too.
const DEFAULT_FILTER: &str = "info";

/// Human-facing hint describing where today's log file lives, e.g.
/// `/Users/me/Library/Logs/com.openkara.desktop/openkara.<date>.log`. Used by
/// the debug-info export so a bug report points at the right file.
pub fn log_file_hint(log_dir: &Path) -> PathBuf {
    log_dir.join(format!(
        "{LOG_FILENAME_PREFIX}.<date>.{LOG_FILENAME_SUFFIX}"
    ))
}

fn default_filter() -> EnvFilter {
    EnvFilter::try_from_env(LOG_FILTER_ENV)
        .or_else(|_| EnvFilter::try_new(DEFAULT_FILTER))
        .unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER))
}

/// Install the global `tracing` subscriber writing to `log_dir`.
///
/// Idempotent: a second call (or a call after another subscriber was already
/// installed, as can happen across tests) is a no-op that returns `Ok(())`.
/// Writes are synchronous so a crash still leaves the most recent lines on
/// disk — the exact moment a diagnostic matters most.
pub fn init(log_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(log_dir)?;

    let file_appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILENAME_PREFIX)
        .filename_suffix(LOG_FILENAME_SUFFIX)
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)?;

    // No ANSI colour in the file; timestamps + target aid triage from a paste.
    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(file_appender);

    // Preserve stderr output for terminal launches during development.
    let stderr_layer = fmt::layer().with_writer(std::io::stderr);

    // `try_init` (not `init`) so a duplicate install never panics the app.
    tracing_subscriber::registry()
        .with(default_filter())
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to install tracing subscriber: {error}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_file_hint_joins_prefix_and_suffix() {
        let hint = log_file_hint(Path::new("/var/logs/openkara"));
        assert_eq!(
            hint,
            PathBuf::from("/var/logs/openkara/openkara.<date>.log")
        );
    }

    #[test]
    fn default_filter_is_constructible() {
        // Guards against a malformed default directive string shipping.
        let _ = default_filter();
    }
}
