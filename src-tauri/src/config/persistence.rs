use anyhow::{Context, Result};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{library_registry::migrate_legacy_library_path, AppConfig};

pub(super) const CONFIG_FILENAME: &str = "config.json";

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Load config; missing or unparseable file → `Ok(None)` (corrupt file quarantined).
/// Genuine I/O read failures return `Err`.
pub fn load_config(app_data_dir: &Path) -> Result<Option<AppConfig>> {
    let config_path = config_path(app_data_dir);
    if !config_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read config at {}", config_path.display()))?;
    let mut config: AppConfig = match serde_json::from_str(&contents) {
        Ok(config) => config,
        Err(parse_err) => {
            match quarantine_corrupt_config(&config_path) {
                Ok(backup) => eprintln!(
                    "warning: config at {} is corrupt ({parse_err}); moved aside to {} and starting with defaults",
                    config_path.display(),
                    backup.display()
                ),
                Err(move_err) => eprintln!(
                    "warning: config at {} is corrupt ({parse_err}) and could not be moved aside ({move_err}); starting with defaults",
                    config_path.display()
                ),
            }
            return Ok(None);
        }
    };

    migrate_legacy_library_path(&mut config);

    Ok(Some(config.normalize_for_save()))
}

pub fn save_config(app_data_dir: &Path, config: &AppConfig) -> Result<()> {
    fs::create_dir_all(app_data_dir)
        .with_context(|| format!("failed to create app data dir {}", app_data_dir.display()))?;

    let config_path = config_path(app_data_dir);
    let json = serde_json::to_string_pretty(&config.clone().normalize_for_save())
        .context("failed to serialize config")?;
    write_atomically(&config_path, json.as_bytes())
        .with_context(|| format!("failed to write config to {}", config_path.display()))?;

    Ok(())
}

/// Write-temp + fsync + atomic rename; cleans up the temp on any failure.
fn write_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp_path = temp_path_for(path);

    let _ = fs::remove_file(&tmp_path);

    let write_result = (|| -> Result<()> {
        let mut file = fs::File::create(&tmp_path)
            .with_context(|| format!("failed to create temp config {}", tmp_path.display()))?;
        file.write_all(contents)
            .with_context(|| format!("failed to write temp config {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to fsync temp config {}", tmp_path.display()))?;
        Ok(())
    })();
    if let Err(err) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    // Same-volume rename is atomic on POSIX and on Windows; cross-volume is not.
    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(anyhow::Error::from(err).context(format!(
            "failed to atomically rename {} -> {}",
            tmp_path.display(),
            path.display()
        )));
    }

    // Best-effort: rename already completed.
    fsync_parent_dir(path);

    Ok(())
}

/// Sibling temp: `<name>.tmp.<pid>.<nanos>.<counter>` (no shared temps under race).
fn temp_path_for(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let file_name = path
        .file_name()
        .map(|name| format!("{}.tmp.{pid}.{nanos}.{counter}", name.to_string_lossy()))
        .unwrap_or_else(|| format!("{CONFIG_FILENAME}.tmp.{pid}.{nanos}.{counter}"));
    path.with_file_name(file_name)
}

/// POSIX parent-dir fsync after rename; no-op on non-Unix.
fn fsync_parent_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn quarantine_corrupt_config(config_path: &Path) -> std::io::Result<PathBuf> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = config_path
        .file_name()
        .map(|name| format!("{}.corrupt-{millis}", name.to_string_lossy()))
        .unwrap_or_else(|| format!("{CONFIG_FILENAME}.corrupt-{millis}"));
    let backup_path = config_path.with_file_name(file_name);
    fs::rename(config_path, &backup_path)?;
    Ok(backup_path)
}

fn config_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CONFIG_FILENAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{library_id_for_path, RegisteredLibrary, StemMode};

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
            libraries: vec![RegisteredLibrary::local(
                "/Users/test/Music/MyLibrary".to_owned(),
                "MyLibrary".to_owned(),
            )],
            active_library_id: Some(library_id_for_path("/Users/test/Music/MyLibrary")),
            stem_mode: Some(StemMode::FourStem),
            language: None,
            hide_batch_separate: None,
            cover_art_backdrop: None,
            hide_upgrade_all: None,
            model_variant: None,
            lyrics_font_step: Some(1),
            execution_provider: None,
            library_sort_mode: None,
            theme_preference: None,
            update_policy: None,
            library_path: None,
            eq_enabled: None,
            eq_gains_db: None,
            crossfade_enabled: None,
            crossfade_duration_ms: None,
            remote_cache_bytes_limit: None,
            pending_mirror_restore: false,
            pending_mirror_restore_active_library_id: None,
            directml_disabled_by_runtime_timeout: None,
        };

        save_config(tmp.path(), &config).unwrap();
        let loaded = load_config(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded.libraries.len(), 1);
        assert_eq!(loaded.active_library_id, config.active_library_id);
        assert_eq!(loaded.stem_mode, Some(StemMode::FourStem));
        assert_eq!(loaded.lyrics_font_step, Some(1));
    }

    // ── Atomic write + corruption recovery (issue #208) ──────────────────

    fn sibling_file_names(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn load_recovers_from_corrupt_config_and_quarantines_it() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join(CONFIG_FILENAME);
        fs::write(&config_path, "{ this is not valid json ]").unwrap();

        let loaded = load_config(tmp.path()).unwrap();
        assert!(loaded.is_none(), "corrupt config recovers to defaults");

        assert!(
            !config_path.exists(),
            "corrupt config.json is moved out of the way"
        );
        let names = sibling_file_names(tmp.path());
        assert!(
            names
                .iter()
                .any(|name| name.starts_with("config.json.corrupt-")),
            "a corrupt-* backup is created, found: {names:?}"
        );
    }

    #[test]
    fn load_recovers_from_empty_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join(CONFIG_FILENAME);
        fs::write(&config_path, "").unwrap();

        let loaded = load_config(tmp.path()).unwrap();
        assert!(loaded.is_none());
        assert!(!config_path.exists());
        assert!(sibling_file_names(tmp.path())
            .iter()
            .any(|name| name.starts_with("config.json.corrupt-")));
    }

    #[test]
    fn save_after_corruption_recovery_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(CONFIG_FILENAME), "garbage").unwrap();

        assert!(load_config(tmp.path()).unwrap().is_none());

        let config = AppConfig {
            stem_mode: Some(StemMode::TwoStem),
            ..AppConfig::default()
        };
        save_config(tmp.path(), &config).unwrap();
        let reloaded = load_config(tmp.path()).unwrap().unwrap();
        assert_eq!(reloaded.stem_mode, Some(StemMode::TwoStem));
    }

    #[test]
    fn interrupted_save_leaves_previous_config_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let good = AppConfig {
            stem_mode: Some(StemMode::FourStem),
            ..AppConfig::default()
        };
        save_config(tmp.path(), &good).unwrap();
        let good_bytes = fs::read(tmp.path().join(CONFIG_FILENAME)).unwrap();

        let leftover_tmp = temp_path_for(&tmp.path().join(CONFIG_FILENAME));
        fs::write(&leftover_tmp, "half-written garbage {").unwrap();

        assert_eq!(
            fs::read(tmp.path().join(CONFIG_FILENAME)).unwrap(),
            good_bytes,
            "interrupted save never touched the live config.json"
        );
        let loaded = load_config(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded.stem_mode, Some(StemMode::FourStem));
    }

    #[test]
    fn atomic_save_leaves_no_temp_file() {
        let tmp = tempfile::tempdir().unwrap();
        save_config(tmp.path(), &AppConfig::default()).unwrap();

        let leftovers: Vec<String> = sibling_file_names(tmp.path())
            .into_iter()
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "no temp file lingers: {leftovers:?}");
    }

    #[test]
    fn atomic_save_overwrites_existing_config() {
        let tmp = tempfile::tempdir().unwrap();
        save_config(
            tmp.path(),
            &AppConfig {
                stem_mode: Some(StemMode::TwoStem),
                ..AppConfig::default()
            },
        )
        .unwrap();
        save_config(
            tmp.path(),
            &AppConfig {
                stem_mode: Some(StemMode::FourStem),
                ..AppConfig::default()
            },
        )
        .unwrap();

        let loaded = load_config(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded.stem_mode, Some(StemMode::FourStem));
        assert!(sibling_file_names(tmp.path())
            .iter()
            .all(|name| !name.contains(".tmp.")));
    }
}
