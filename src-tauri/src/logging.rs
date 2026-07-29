use std::path::{Path, PathBuf};

use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub const LOG_FILENAME_PREFIX: &str = "openkara";
pub const LOG_FILENAME_SUFFIX: &str = "log";
const MAX_LOG_FILES: usize = 7;
const LOG_FILTER_ENV: &str = "OPENKARA_LOG";
const DEFAULT_FILTER: &str = "info";

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

pub fn init(log_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(log_dir)?;

    let file_appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILENAME_PREFIX)
        .filename_suffix(LOG_FILENAME_SUFFIX)
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)?;

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(file_appender);

    let stderr_layer = fmt::layer().with_writer(std::io::stderr);

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
        let _ = default_filter();
    }
}
