use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const CONFIG_FILENAME: &str = "config.json";

/// Per-machine configuration stored in `{app_data_dir}/config.json`.
///
/// This is the only file that stays outside the portable library directory.
/// It tells the app where the user's karaoke library lives.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// Absolute path to the library root directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_path: Option<String>,
}

/// Load the per-machine config. Returns `Ok(None)` if the file does not exist.
pub fn load_config(app_data_dir: &Path) -> Result<Option<AppConfig>> {
    let config_path = config_path(app_data_dir);
    if !config_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read config at {}", config_path.display()))?;
    let config: AppConfig = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse config at {}", config_path.display()))?;

    Ok(Some(config))
}

/// Persist the per-machine config to disk.
pub fn save_config(app_data_dir: &Path, config: &AppConfig) -> Result<()> {
    fs::create_dir_all(app_data_dir)
        .with_context(|| format!("failed to create app data dir {}", app_data_dir.display()))?;

    let config_path = config_path(app_data_dir);
    let json = serde_json::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(&config_path, json)
        .with_context(|| format!("failed to write config to {}", config_path.display()))?;

    Ok(())
}

fn config_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CONFIG_FILENAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_returns_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let config = load_config(tmp.path()).unwrap();
        assert!(config.is_none());
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let config = AppConfig {
            library_path: Some("/Users/test/Music/MyLibrary".to_owned()),
        };

        save_config(tmp.path(), &config).unwrap();
        let loaded = load_config(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded.library_path, config.library_path);
    }
}
