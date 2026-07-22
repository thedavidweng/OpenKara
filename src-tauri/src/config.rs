use crate::hash;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

const CONFIG_FILENAME: &str = "config.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StemMode {
    #[default]
    TwoStem,
    FourStem,
}

/// Library sort order persisted through the settings authority. The frontend
/// comparator mirrors these exact total orders; see `src/lib/song-sort.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySortMode {
    #[default]
    RecentlyImported,
    TitleAsc,
    ArtistAsc,
}

impl LibrarySortMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RecentlyImported => "recently_imported",
            Self::TitleAsc => "title_asc",
            Self::ArtistAsc => "artist_asc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    System,
    Light,
    #[default]
    Dark,
}

impl ThemePreference {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelVariant {
    #[default]
    Htdemucs,
    HtdemucsFt,
}

impl ModelVariant {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelVariant::Htdemucs => "htdemucs",
            ModelVariant::HtdemucsFt => "htdemucs_ft",
        }
    }

    pub fn parse(s: &str) -> Option<ModelVariant> {
        match s {
            "htdemucs" => Some(ModelVariant::Htdemucs),
            "htdemucs_ft" => Some(ModelVariant::HtdemucsFt),
            _ => None,
        }
    }
}

/// Hardware acceleration preference for ONNX Runtime inference.
///
/// The persisted value is always explicit. When config does not yet contain an
/// execution provider, the app chooses a platform default at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProviderPreference {
    Cpu,
    // XNNPACK uses NEON on ARM64 and AVX2/AVX-512 on x86-64 for conv/matmul,
    // avoids CoreML AOT compile overhead, and ships inside the existing ORT dylib.
    Xnnpack,
    #[serde(alias = "directml")]
    DirectMl,
}

/// Platform seam used to test the execution-provider policy table without
/// depending on the host running the test. Private: not an IPC type.
/// Variants for platforms other than the compile target are only constructed
/// in tests, so silence the non-test dead-code warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum ExecutionProviderPlatform {
    Macos,
    Windows,
    Linux,
    Other,
}

impl ExecutionProviderPlatform {
    fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::Macos
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Self::Other
        }
    }
}

impl Default for ExecutionProviderPreference {
    fn default() -> Self {
        Self::default_for_current_platform()
    }
}

impl ExecutionProviderPreference {
    /// DirectML is Windows-only; CPU and XNNPACK are available everywhere.
    fn available_for(platform: ExecutionProviderPlatform) -> &'static [Self] {
        match platform {
            ExecutionProviderPlatform::Windows => &[Self::Cpu, Self::Xnnpack, Self::DirectMl],
            ExecutionProviderPlatform::Macos
            | ExecutionProviderPlatform::Linux
            | ExecutionProviderPlatform::Other => &[Self::Cpu, Self::Xnnpack],
        }
    }

    fn default_for(platform: ExecutionProviderPlatform) -> Self {
        match platform {
            ExecutionProviderPlatform::Windows => Self::DirectMl,
            ExecutionProviderPlatform::Macos
            | ExecutionProviderPlatform::Linux
            | ExecutionProviderPlatform::Other => Self::Xnnpack,
        }
    }

    fn is_available_for(self, platform: ExecutionProviderPlatform) -> bool {
        Self::available_for(platform).contains(&self)
    }

    pub fn default_for_current_platform() -> Self {
        Self::default_for(ExecutionProviderPlatform::current())
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Xnnpack => "xnnpack",
            Self::DirectMl => "directml",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cpu" => Some(Self::Cpu),
            "xnnpack" => Some(Self::Xnnpack),
            "directml" => Some(Self::DirectMl),
            _ => None,
        }
    }

    /// Used by the frontend to populate the settings dropdown.
    pub fn available_for_current_platform() -> Vec<&'static str> {
        Self::available_for(ExecutionProviderPlatform::current())
            .iter()
            .map(|ep| ep.as_str())
            .collect()
    }

    pub fn is_available_for_current_platform(self) -> bool {
        self.is_available_for(ExecutionProviderPlatform::current())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteLibraryProvider {
    GoogleDrive,
    Dropbox,
    WebDav,
}

impl RemoteLibraryProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GoogleDrive => "google_drive",
            Self::Dropbox => "dropbox",
            Self::WebDav => "webdav",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "google_drive" => Some(Self::GoogleDrive),
            "dropbox" => Some(Self::Dropbox),
            "webdav" => Some(Self::WebDav),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemoteLibraryConnectionConfig {
    GoogleDrive { oauth_client_id: String },
    Dropbox { app_key: String },
    WebDav { server_url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RegisteredLibrary {
    Local {
        id: String,
        display_name: String,
        root_path: String,
    },
    Remote {
        id: String,
        display_name: String,
        provider: RemoteLibraryProvider,
        account_id: String,
        remote_root_locator: String,
        remote_path_display: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        connection_config: Option<RemoteLibraryConnectionConfig>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cached_db_path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        remote_revision: Option<String>,
    },
}

impl RegisteredLibrary {
    pub fn local(root_path: String, display_name: String) -> Self {
        Self::Local {
            id: library_id_for_path(&root_path),
            display_name,
            root_path,
        }
    }

    pub fn remote(
        id: String,
        display_name: String,
        provider: RemoteLibraryProvider,
        account_id: String,
        remote_root_locator: String,
        remote_path_display: String,
        connection_config: Option<RemoteLibraryConnectionConfig>,
        cached_db_path: Option<String>,
        remote_revision: Option<String>,
    ) -> Self {
        Self::Remote {
            id,
            display_name,
            provider,
            account_id,
            remote_root_locator,
            remote_path_display,
            connection_config,
            cached_db_path,
            remote_revision,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Local { id, .. } | Self::Remote { id, .. } => id,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Local { display_name, .. } | Self::Remote { display_name, .. } => display_name,
        }
    }

    pub fn provider(&self) -> Option<RemoteLibraryProvider> {
        match self {
            Self::Remote { provider, .. } => Some(*provider),
            Self::Local { .. } => None,
        }
    }

    pub fn account_id(&self) -> Option<&str> {
        match self {
            Self::Remote { account_id, .. } => Some(account_id.as_str()),
            Self::Local { .. } => None,
        }
    }

    pub fn remote_root_locator(&self) -> Option<&str> {
        match self {
            Self::Remote {
                remote_root_locator,
                ..
            } => Some(remote_root_locator.as_str()),
            Self::Local { .. } => None,
        }
    }

    pub fn remote_path_display(&self) -> Option<&str> {
        match self {
            Self::Remote {
                remote_path_display,
                ..
            } => Some(remote_path_display.as_str()),
            Self::Local { .. } => None,
        }
    }

    pub fn connection_config(&self) -> Option<&RemoteLibraryConnectionConfig> {
        match self {
            Self::Remote {
                connection_config, ..
            } => connection_config.as_ref(),
            Self::Local { .. } => None,
        }
    }

    pub fn cached_db_path(&self) -> Option<&str> {
        match self {
            Self::Remote {
                cached_db_path: Some(cached_db_path),
                ..
            } => Some(cached_db_path.as_str()),
            Self::Remote {
                cached_db_path: None,
                ..
            }
            | Self::Local { .. } => None,
        }
    }

    pub fn remote_revision(&self) -> Option<&str> {
        match self {
            Self::Remote {
                remote_revision: Some(remote_revision),
                ..
            } => Some(remote_revision.as_str()),
            Self::Remote {
                remote_revision: None,
                ..
            }
            | Self::Local { .. } => None,
        }
    }

    pub fn google_drive_client_id(&self) -> Option<&str> {
        match self.connection_config() {
            Some(RemoteLibraryConnectionConfig::GoogleDrive { oauth_client_id }) => {
                Some(oauth_client_id.as_str())
            }
            _ => None,
        }
    }

    pub fn dropbox_app_key(&self) -> Option<&str> {
        match self.connection_config() {
            Some(RemoteLibraryConnectionConfig::Dropbox { app_key }) => Some(app_key.as_str()),
            _ => None,
        }
    }

    pub fn webdav_server_url(&self) -> Option<&str> {
        match self.connection_config() {
            Some(RemoteLibraryConnectionConfig::WebDav { server_url }) => Some(server_url.as_str()),
            _ => None,
        }
    }

    pub fn kind(&self) -> LibraryKind {
        match self {
            Self::Local { .. } => LibraryKind::Local,
            Self::Remote { .. } => LibraryKind::Remote,
        }
    }

    pub fn working_copy_root(&self) -> Option<PathBuf> {
        match self {
            Self::Local { root_path, .. } => Some(PathBuf::from(root_path)),
            Self::Remote {
                cached_db_path: Some(cached_db_path),
                ..
            } => Path::new(cached_db_path).parent().map(Path::to_path_buf),
            Self::Remote {
                cached_db_path: None,
                ..
            } => None,
        }
    }

    pub fn root_path(&self) -> Option<&str> {
        match self {
            Self::Local { root_path, .. } => Some(root_path.as_str()),
            Self::Remote {
                remote_path_display,
                ..
            } => Some(remote_path_display.as_str()),
        }
    }
}

/// This is the only file that stays outside the portable library directory.
/// It tracks user preferences and the registered library set.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// Legacy single-library path. Kept only for migration from older config
    /// files and omitted from new saves once the registry exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<RegisteredLibrary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_library_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stem_mode: Option<StemMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_batch_separate: Option<bool>,
    /// When false, the lyrics stage renders without the blurred album-art backdrop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_art_backdrop: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_variant: Option<ModelVariant>,
    // Lyrics font size is a per-machine display preference, so it belongs in
    // config.json with the rest of app settings rather than in the lyrics table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics_font_step: Option<i8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_provider: Option<ExecutionProviderPreference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eq_enabled: Option<bool>,
    /// Per-band EQ gains in dB for the five fixed bands
    /// (60, 230, 910, 3600, 14000 Hz). Each gain is clamped to
    /// -12.0..=12.0 dB on read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eq_gains_db: Option<[f32; 5]>,
    /// Only applies to fully decoded local tracks — streaming
    /// and stems tracks always use gapless transition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crossfade_enabled: Option<bool>,
    /// Crossfade overlap duration in milliseconds. Clamped to
    /// 500..=10_000 on read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crossfade_duration_ms: Option<u32>,
    /// Persisted so the selected order survives restarts and is shared
    /// across WebViews via the settings sync snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_sort_mode: Option<LibrarySortMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_preference: Option<ThemePreference>,
    /// When set, the cache directory is trimmed to stay under this limit,
    /// deleting the least-recently-used files. When absent the cache defaults
    /// to a finite 2 GiB budget (see `DEFAULT_CACHE_BYTES_LIMIT`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_cache_bytes_limit: Option<u64>,
    /// Crash-recovery marker for mirror operations. `mirror_local_library_to_remote`
    /// temporarily swaps `active_library_id` to the remote library for the sync
    /// duration. If the app crashes mid-sync, this flag is true so startup
    /// knows to restore `pending_mirror_restore_active_library_id` (which may
    /// be None if the original active library was unset). Cleared after a
    /// successful sync or on startup recovery.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending_mirror_restore: bool,
    /// Only meaningful when `pending_mirror_restore` is true. May be None
    /// if the original active library was unset — that is a valid restore
    /// target, not an absence of a pending operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_mirror_restore_active_library_id: Option<String>,
}

impl AppConfig {
    pub fn normalize_for_save(mut self) -> Self {
        if !self.libraries.is_empty() {
            self.library_path = None;
        }

        if self.active_library_id.is_none() {
            self.active_library_id = self
                .libraries
                .first()
                .map(|library| library.id().to_owned());
        }

        self
    }

    pub fn active_library(&self) -> Option<&RegisteredLibrary> {
        if let Some(active_id) = self.active_library_id.as_deref() {
            self.libraries
                .iter()
                .find(|library| library.id() == active_id)
        } else {
            self.libraries.first()
        }
    }

    pub fn effective_stem_mode(&self) -> StemMode {
        self.stem_mode.unwrap_or_default()
    }

    pub fn effective_model_variant(&self) -> ModelVariant {
        self.model_variant.unwrap_or_default()
    }

    pub fn effective_lyrics_font_step(&self) -> i8 {
        self.lyrics_font_step.unwrap_or(0)
    }

    /// A stale known cross-platform value (e.g. `directml` on macOS) is
    /// normalized without writing to disk.
    fn effective_execution_provider_for(
        &self,
        platform: ExecutionProviderPlatform,
    ) -> ExecutionProviderPreference {
        match self.execution_provider {
            Some(ep) if ep.is_available_for(platform) => ep,
            _ => ExecutionProviderPreference::default_for(platform),
        }
    }

    pub fn effective_execution_provider(&self) -> ExecutionProviderPreference {
        self.effective_execution_provider_for(ExecutionProviderPlatform::current())
    }

    pub fn effective_eq_enabled(&self) -> bool {
        self.eq_enabled.unwrap_or(false)
    }

    /// Returns the per-band EQ gains, clamped to -12.0..=12.0 dB.
    /// Non-finite values are replaced with 0.0.
    pub fn effective_eq_gains_db(&self) -> [f32; 5] {
        let mut gains = self.eq_gains_db.unwrap_or([0.0; 5]);
        for g in gains.iter_mut() {
            if !g.is_finite() {
                *g = 0.0;
            }
            *g = g.clamp(-12.0, 12.0);
        }
        gains
    }

    pub fn effective_library_sort_mode(&self) -> LibrarySortMode {
        self.library_sort_mode.unwrap_or_default()
    }

    pub fn effective_crossfade_enabled(&self) -> bool {
        self.crossfade_enabled.unwrap_or(false)
    }

    /// Clamped to 500..=10_000.
    pub fn effective_crossfade_duration_ms(&self) -> u32 {
        self.crossfade_duration_ms
            .unwrap_or(3_000)
            .clamp(500, 10_000)
    }

    pub fn effective_theme_preference(&self) -> ThemePreference {
        self.theme_preference.unwrap_or_default()
    }
}

/// Returns `Ok(None)` if the file does not exist.
pub fn load_config(app_data_dir: &Path) -> Result<Option<AppConfig>> {
    let config_path = config_path(app_data_dir);
    if !config_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read config at {}", config_path.display()))?;
    let mut config: AppConfig = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse config at {}", config_path.display()))?;

    if config.libraries.is_empty() {
        if let Some(library_path) = config.library_path.clone() {
            let display_name = Path::new(&library_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("OpenKara Library")
                .to_owned();
            config
                .libraries
                .push(RegisteredLibrary::local(library_path, display_name));
        }
    }

    Ok(Some(config.normalize_for_save()))
}

pub fn save_config(app_data_dir: &Path, config: &AppConfig) -> Result<()> {
    fs::create_dir_all(app_data_dir)
        .with_context(|| format!("failed to create app data dir {}", app_data_dir.display()))?;

    let config_path = config_path(app_data_dir);
    let json = serde_json::to_string_pretty(&config.clone().normalize_for_save())
        .context("failed to serialize config")?;
    fs::write(&config_path, json)
        .with_context(|| format!("failed to write config to {}", config_path.display()))?;

    Ok(())
}

fn config_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CONFIG_FILENAME)
}

pub fn library_id_for_path(path: &str) -> String {
    let digest = Sha256::digest(path.as_bytes());
    format!("library-{}", hash::hex_lower(digest))
}

pub fn library_display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_owned()
}

pub fn migrate_legacy_library_path(config: &mut AppConfig) {
    if config.libraries.is_empty() {
        if let Some(path) = config.library_path.clone() {
            config.libraries.push(RegisteredLibrary::local(
                path.clone(),
                library_display_name(&path),
            ));
            config.active_library_id = config
                .libraries
                .first()
                .map(|library| library.id().to_owned());
        }
    }
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
            libraries: vec![RegisteredLibrary::local(
                "/Users/test/Music/MyLibrary".to_owned(),
                "MyLibrary".to_owned(),
            )],
            active_library_id: Some(library_id_for_path("/Users/test/Music/MyLibrary")),
            stem_mode: Some(StemMode::FourStem),
            language: None,
            hide_batch_separate: None,
            cover_art_backdrop: None,
            model_variant: None,
            lyrics_font_step: Some(1),
            execution_provider: None,
            library_sort_mode: None,
            theme_preference: None,
            library_path: None,
            eq_enabled: None,
            eq_gains_db: None,
            crossfade_enabled: None,
            crossfade_duration_ms: None,
            remote_cache_bytes_limit: None,
            pending_mirror_restore: false,
            pending_mirror_restore_active_library_id: None,
        };

        save_config(tmp.path(), &config).unwrap();
        let loaded = load_config(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded.libraries.len(), 1);
        assert_eq!(loaded.active_library_id, config.active_library_id);
        assert_eq!(loaded.stem_mode, Some(StemMode::FourStem));
        assert_eq!(loaded.lyrics_font_step, Some(1));
    }

    #[test]
    fn effective_stem_mode_defaults_to_two_stem() {
        let config = AppConfig::default();
        assert_eq!(config.effective_stem_mode(), StemMode::TwoStem);
    }

    #[test]
    fn stem_mode_none_is_omitted_from_json() {
        let config = AppConfig {
            library_path: None,
            libraries: vec![],
            active_library_id: None,
            stem_mode: None,
            language: None,
            hide_batch_separate: None,
            cover_art_backdrop: None,
            model_variant: None,
            lyrics_font_step: None,
            execution_provider: None,
            eq_enabled: None,
            eq_gains_db: None,
            crossfade_enabled: None,
            crossfade_duration_ms: None,
            library_sort_mode: None,
            theme_preference: None,
            remote_cache_bytes_limit: None,
            pending_mirror_restore: false,
            pending_mirror_restore_active_library_id: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("stem_mode"));
    }

    #[test]
    fn effective_lyrics_font_step_defaults_to_zero() {
        let config = AppConfig::default();
        assert_eq!(config.effective_lyrics_font_step(), 0);
    }

    #[test]
    fn lyrics_font_step_none_is_omitted_from_json() {
        let config = AppConfig {
            library_path: None,
            libraries: vec![],
            active_library_id: None,
            stem_mode: None,
            language: None,
            hide_batch_separate: None,
            cover_art_backdrop: None,
            model_variant: None,
            lyrics_font_step: None,
            execution_provider: None,
            eq_enabled: None,
            eq_gains_db: None,
            crossfade_enabled: None,
            crossfade_duration_ms: None,
            library_sort_mode: None,
            theme_preference: None,
            remote_cache_bytes_limit: None,
            pending_mirror_restore: false,
            pending_mirror_restore_active_library_id: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("lyrics_font_step"));
    }

    #[test]
    fn execution_provider_none_is_omitted_from_json() {
        let config = AppConfig {
            library_path: None,
            libraries: vec![],
            active_library_id: None,
            stem_mode: None,
            language: None,
            hide_batch_separate: None,
            cover_art_backdrop: None,
            model_variant: None,
            lyrics_font_step: None,
            execution_provider: None,
            eq_enabled: None,
            eq_gains_db: None,
            crossfade_enabled: None,
            crossfade_duration_ms: None,
            library_sort_mode: None,
            theme_preference: None,
            remote_cache_bytes_limit: None,
            pending_mirror_restore: false,
            pending_mirror_restore_active_library_id: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("execution_provider"));
    }

    #[test]
    fn execution_provider_round_trips_through_json() {
        let config = AppConfig {
            execution_provider: Some(ExecutionProviderPreference::Xnnpack),
            library_sort_mode: None,
            theme_preference: None,
            ..AppConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            loaded.execution_provider,
            Some(ExecutionProviderPreference::Xnnpack)
        );
    }

    #[test]
    fn legacy_library_path_is_migrated_to_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = AppConfig {
            library_path: Some("/Users/test/Music/Legacy".to_owned()),
            stem_mode: Some(StemMode::TwoStem),
            language: Some("zh-CN".to_owned()),
            hide_batch_separate: Some(true),
            cover_art_backdrop: None,
            model_variant: Some(ModelVariant::HtdemucsFt),
            lyrics_font_step: Some(1),
            execution_provider: None,
            eq_enabled: None,
            eq_gains_db: None,
            crossfade_enabled: None,
            crossfade_duration_ms: None,
            library_sort_mode: None,
            theme_preference: None,
            libraries: vec![],
            active_library_id: None,
            remote_cache_bytes_limit: None,
            pending_mirror_restore: false,
            pending_mirror_restore_active_library_id: None,
        };

        save_config(tmp.path(), &legacy).unwrap();
        let loaded = load_config(tmp.path()).unwrap().unwrap();

        assert!(loaded.library_path.is_none());
        assert_eq!(loaded.libraries.len(), 1);
        assert_eq!(loaded.active_library(), loaded.libraries.first());
    }

    #[test]
    fn stale_remote_mirror_binding_is_dropped_on_save() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = r#"{
          "libraries": [
            {
              "kind": "remote",
              "id": "remote-1",
              "display_name": "OpenKara",
              "provider": "dropbox",
              "account_id": "account-1",
              "remote_root_locator": "/OpenKara",
              "remote_path_display": "/OpenKara",
              "connection_config": { "type": "dropbox", "app_key": "key" },
              "cached_db_path": "/tmp/openkara.db",
              "remote_revision": "rev-1",
              "bound_local_library_id": "local-1"
            }
          ],
          "active_library_id": "remote-1"
        }"#;
        fs::write(tmp.path().join(CONFIG_FILENAME), raw).unwrap();

        let loaded = load_config(tmp.path()).unwrap().unwrap();
        save_config(tmp.path(), &loaded).unwrap();
        let saved = fs::read_to_string(tmp.path().join(CONFIG_FILENAME)).unwrap();

        assert!(!saved.contains("bound_local_library_id"));
    }

    #[test]
    fn available_execution_providers_are_explicit_only() {
        let providers = ExecutionProviderPreference::available_for_current_platform();
        assert!(!providers.contains(&"auto"));
        assert!(providers.contains(&"cpu"));
        assert!(providers.contains(&"xnnpack"));

        #[cfg(target_os = "windows")]
        assert!(providers.contains(&"directml"));
    }

    #[test]
    fn effective_execution_provider_defaults_to_platform_default() {
        let config = AppConfig::default();

        #[cfg(target_os = "windows")]
        assert_eq!(
            config.effective_execution_provider(),
            ExecutionProviderPreference::DirectMl
        );

        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            config.effective_execution_provider(),
            ExecutionProviderPreference::Xnnpack
        );
    }

    // These exercise the pure platform parameter so they pass on every host.

    #[test]
    fn execution_provider_available_table_is_exact_and_ordered() {
        use ExecutionProviderPlatform::*;

        assert_eq!(
            ExecutionProviderPreference::available_for(Macos),
            &[
                ExecutionProviderPreference::Cpu,
                ExecutionProviderPreference::Xnnpack
            ]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(Linux),
            &[
                ExecutionProviderPreference::Cpu,
                ExecutionProviderPreference::Xnnpack
            ]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(Other),
            &[
                ExecutionProviderPreference::Cpu,
                ExecutionProviderPreference::Xnnpack
            ]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(Windows),
            &[
                ExecutionProviderPreference::Cpu,
                ExecutionProviderPreference::Xnnpack,
                ExecutionProviderPreference::DirectMl,
            ]
        );
    }

    #[test]
    fn library_sort_mode_defaults_to_recently_imported() {
        assert_eq!(
            AppConfig::default().effective_library_sort_mode(),
            LibrarySortMode::RecentlyImported
        );
    }

    #[test]
    fn directml_is_present_only_for_windows() {
        use ExecutionProviderPlatform::*;
        for &platform in &[Macos, Linux, Other] {
            assert!(!ExecutionProviderPreference::available_for(platform)
                .contains(&ExecutionProviderPreference::DirectMl));
        }
        assert!(ExecutionProviderPreference::available_for(Windows)
            .contains(&ExecutionProviderPreference::DirectMl));
    }

    #[test]
    fn cpu_and_xnnpack_are_present_for_every_target() {
        use ExecutionProviderPlatform::*;
        for &platform in &[Macos, Windows, Linux, Other] {
            let list = ExecutionProviderPreference::available_for(platform);
            assert!(list.contains(&ExecutionProviderPreference::Cpu));
            assert!(list.contains(&ExecutionProviderPreference::Xnnpack));
        }
    }

    #[test]
    fn library_sort_mode_none_is_omitted_from_json() {
        let config = AppConfig {
            library_sort_mode: None,
            theme_preference: None,
            ..AppConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("library_sort_mode"));
    }

    #[test]
    fn library_sort_mode_round_trips_through_json() {
        for mode in [
            LibrarySortMode::RecentlyImported,
            LibrarySortMode::TitleAsc,
            LibrarySortMode::ArtistAsc,
        ] {
            let config = AppConfig {
                library_sort_mode: Some(mode),
                theme_preference: None,
                ..AppConfig::default()
            };
            let json = serde_json::to_string(&config).unwrap();
            let loaded: AppConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(loaded.library_sort_mode, Some(mode));
        }
    }

    #[test]
    fn every_target_default_is_a_member_of_its_list() {
        use ExecutionProviderPlatform::*;
        for &platform in &[Macos, Windows, Linux, Other] {
            let default = ExecutionProviderPreference::default_for(platform);
            assert!(default.is_available_for(platform));
        }
    }

    #[test]
    fn effective_execution_provider_for_preserves_valid_saved_value() {
        use ExecutionProviderPlatform::*;
        let config = AppConfig {
            execution_provider: Some(ExecutionProviderPreference::Cpu),
            ..AppConfig::default()
        };
        assert_eq!(
            config.effective_execution_provider_for(Macos),
            ExecutionProviderPreference::Cpu
        );
        assert_eq!(
            config.effective_execution_provider_for(Windows),
            ExecutionProviderPreference::Cpu
        );
    }

    #[test]
    fn effective_eq_defaults_to_disabled_flat() {
        let config = AppConfig::default();
        assert!(!config.effective_eq_enabled());
        assert_eq!(config.effective_eq_gains_db(), [0.0; 5]);
    }

    #[test]
    fn effective_eq_enabled_hydrates_from_persisted_value() {
        let config = AppConfig {
            eq_enabled: Some(true),
            ..Default::default()
        };
        assert!(config.effective_eq_enabled());
    }

    #[test]
    fn effective_eq_gains_hydrates_from_persisted_values() {
        let config = AppConfig {
            eq_gains_db: Some([3.0, -6.0, 0.0, 12.0, -12.0]),
            ..Default::default()
        };
        assert_eq!(
            config.effective_eq_gains_db(),
            [3.0, -6.0, 0.0, 12.0, -12.0]
        );
    }

    #[test]
    fn effective_execution_provider_for_falls_back_for_stale_cross_platform_value() {
        use ExecutionProviderPlatform::*;
        let config = AppConfig {
            execution_provider: Some(ExecutionProviderPreference::DirectMl),
            ..AppConfig::default()
        };
        assert_eq!(
            config.effective_execution_provider_for(Macos),
            ExecutionProviderPreference::Xnnpack
        );
        assert_eq!(
            config.effective_execution_provider_for(Linux),
            ExecutionProviderPreference::Xnnpack
        );
        assert_eq!(
            config.effective_execution_provider_for(Other),
            ExecutionProviderPreference::Xnnpack
        );
        assert_eq!(
            config.effective_execution_provider_for(Windows),
            ExecutionProviderPreference::DirectMl
        );
    }

    #[test]
    fn effective_eq_gains_clamps_out_of_range_persisted_values() {
        // A manually-edited config file could contain values outside the
        // valid range. The effective accessor clamps them rather than
        // panicking, so the app stays usable.
        let config = AppConfig {
            eq_gains_db: Some([20.0, -20.0, 0.0, 100.0, -100.0]),
            ..Default::default()
        };
        assert_eq!(
            config.effective_eq_gains_db(),
            [12.0, -12.0, 0.0, 12.0, -12.0]
        );
    }

    #[test]
    fn effective_execution_provider_for_falls_back_when_unset() {
        use ExecutionProviderPlatform::*;
        let config = AppConfig {
            execution_provider: None,
            ..AppConfig::default()
        };
        assert_eq!(
            config.effective_execution_provider_for(Macos),
            ExecutionProviderPreference::Xnnpack
        );
        assert_eq!(
            config.effective_execution_provider_for(Windows),
            ExecutionProviderPreference::DirectMl
        );
        assert_eq!(
            config.effective_execution_provider_for(Linux),
            ExecutionProviderPreference::Xnnpack
        );
    }

    #[test]
    fn current_target_wrappers_agree_with_compile_target() {
        use ExecutionProviderPlatform::*;
        let current = ExecutionProviderPlatform::current();
        #[cfg(target_os = "macos")]
        assert_eq!(current, Macos);
        #[cfg(target_os = "windows")]
        assert_eq!(current, Windows);
        #[cfg(target_os = "linux")]
        assert_eq!(current, Linux);
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        assert_eq!(current, Other);
    }

    #[test]
    fn effective_eq_gains_replaces_non_finite_persisted_values_with_zero() {
        let config = AppConfig {
            eq_gains_db: Some([f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, 6.0]),
            ..Default::default()
        };
        let gains = config.effective_eq_gains_db();
        assert_eq!(gains, [0.0, 0.0, 0.0, 0.0, 6.0]);
    }

    #[test]
    fn library_sort_mode_rejects_unknown_snake_case_string() {
        let raw = r#"{ "library_sort_mode": "descending_title" }"#;
        let result: Result<AppConfig, _> = serde_json::from_str(raw);
        assert!(result.is_err());
    }

    #[test]
    fn library_sort_mode_persists_through_save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let config = AppConfig {
            library_sort_mode: Some(LibrarySortMode::ArtistAsc),
            theme_preference: None,
            ..AppConfig::default()
        };
        save_config(tmp.path(), &config).unwrap();
        let loaded = load_config(tmp.path()).unwrap().unwrap();
        assert_eq!(
            loaded.effective_library_sort_mode(),
            LibrarySortMode::ArtistAsc
        );
    }

    #[test]
    fn effective_theme_preference_defaults_to_dark() {
        let config = AppConfig::default();
        assert_eq!(config.effective_theme_preference(), ThemePreference::Dark);
    }

    #[test]
    fn theme_preference_none_is_omitted_from_json() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("theme_preference"));
    }

    #[test]
    fn theme_preference_round_trips_through_json() {
        for preference in [
            ThemePreference::System,
            ThemePreference::Light,
            ThemePreference::Dark,
        ] {
            let config = AppConfig {
                theme_preference: Some(preference),
                ..AppConfig::default()
            };
            let json = serde_json::to_string(&config).unwrap();
            let loaded: AppConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(loaded.theme_preference, Some(preference));
        }
    }

    #[test]
    fn invalid_theme_preference_string_is_rejected() {
        let json = r#"{"theme_preference": "high_contrast"}"#;
        let result = serde_json::from_str::<AppConfig>(json);
        assert!(result.is_err());
    }

    #[test]
    fn missing_theme_preference_field_defaults_to_dark() {
        let loaded: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(loaded.effective_theme_preference(), ThemePreference::Dark);
    }

    // ── Crossfade config hydration ───────────────────────────────────────

    #[test]
    fn effective_crossfade_defaults_to_disabled_3000ms() {
        let config = AppConfig::default();
        assert!(!config.effective_crossfade_enabled());
        assert_eq!(config.effective_crossfade_duration_ms(), 3_000);
    }

    #[test]
    fn effective_crossfade_enabled_hydrates_from_persisted_value() {
        let config = AppConfig {
            crossfade_enabled: Some(true),
            ..Default::default()
        };
        assert!(config.effective_crossfade_enabled());
    }

    #[test]
    fn effective_crossfade_duration_hydrates_from_persisted_value() {
        let config = AppConfig {
            crossfade_duration_ms: Some(5_000),
            ..Default::default()
        };
        assert_eq!(config.effective_crossfade_duration_ms(), 5_000);
    }

    #[test]
    fn effective_crossfade_duration_clamps_below_minimum() {
        let config = AppConfig {
            crossfade_duration_ms: Some(100),
            ..Default::default()
        };
        assert_eq!(config.effective_crossfade_duration_ms(), 500);
    }

    #[test]
    fn effective_crossfade_duration_clamps_above_maximum() {
        let config = AppConfig {
            crossfade_duration_ms: Some(20_000),
            ..Default::default()
        };
        assert_eq!(config.effective_crossfade_duration_ms(), 10_000);
    }
}
