use crate::hash;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const CONFIG_FILENAME: &str = "config.json";

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StemMode {
    TwoStem,
    #[default]
    FourStem,
}

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
pub enum UpdatePolicy {
    Manual,
    #[default]
    Notify,
    AutoDownload,
}

impl UpdatePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Notify => "notify",
            Self::AutoDownload => "auto_download",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "manual" => Some(Self::Manual),
            "notify" => Some(Self::Notify),
            "auto_download" => Some(Self::AutoDownload),
            _ => None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProviderPreference {
    Cpu,
    Xnnpack,
    CoreMl,
    #[serde(alias = "directml")]
    DirectMl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum ExecutionProviderPlatform {
    MacosAppleSilicon,
    MacosIntel,
    Windows,
    Linux,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExecutionProviderCapabilities {
    directml: bool,
    coreml: bool,
    xnnpack: bool,
}

impl ExecutionProviderCapabilities {
    fn current() -> Self {
        Self {
            directml: crate::platform_capabilities::directml_available(),
            coreml: crate::platform_capabilities::coreml_available(),
            xnnpack: crate::platform_capabilities::xnnpack_available(),
        }
    }

    fn for_platform(platform: ExecutionProviderPlatform) -> Self {
        let current_platform = ExecutionProviderPlatform::current();
        Self {
            directml: platform == ExecutionProviderPlatform::Windows
                && current_platform == ExecutionProviderPlatform::Windows
                && crate::platform_capabilities::directml_available(),
            coreml: platform == ExecutionProviderPlatform::MacosAppleSilicon,
            xnnpack: matches!(
                platform,
                ExecutionProviderPlatform::MacosIntel | ExecutionProviderPlatform::Linux
            ),
        }
    }
}

impl ExecutionProviderPlatform {
    fn current() -> Self {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            Self::MacosAppleSilicon
        }
        #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
        {
            Self::MacosIntel
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
    fn available_for(platform: ExecutionProviderPlatform) -> &'static [Self] {
        match platform {
            ExecutionProviderPlatform::Windows => &[Self::Cpu, Self::Xnnpack, Self::DirectMl],
            ExecutionProviderPlatform::MacosAppleSilicon => {
                &[Self::Cpu, Self::CoreMl, Self::Xnnpack]
            }
            ExecutionProviderPlatform::MacosIntel | ExecutionProviderPlatform::Linux => {
                &[Self::Cpu, Self::Xnnpack]
            }
            ExecutionProviderPlatform::Other => &[Self::Cpu],
        }
    }

    /// Platform default EP: CoreML on Apple Silicon when `capabilities.coreml`
    /// is available (else CPU); DirectML on Windows with D3D12 hardware;
    /// otherwise CPU (XNNPACK loses to ORT CPU on Intel/Linux).
    fn default_for(platform: ExecutionProviderPlatform) -> Self {
        Self::default_for_capabilities(
            platform,
            ExecutionProviderCapabilities::for_platform(platform),
        )
    }

    fn default_for_capabilities(
        platform: ExecutionProviderPlatform,
        capabilities: ExecutionProviderCapabilities,
    ) -> Self {
        match platform {
            ExecutionProviderPlatform::Windows if capabilities.directml => Self::DirectMl,
            ExecutionProviderPlatform::Windows => Self::Cpu,
            ExecutionProviderPlatform::MacosAppleSilicon if capabilities.coreml => Self::CoreMl,
            ExecutionProviderPlatform::MacosIntel
            | ExecutionProviderPlatform::Linux
            | ExecutionProviderPlatform::Other => Self::Cpu,
            ExecutionProviderPlatform::MacosAppleSilicon => Self::Cpu,
        }
    }

    fn is_available_for(self, platform: ExecutionProviderPlatform) -> bool {
        Self::available_for(platform).contains(&self)
    }

    fn is_compatible_for(
        self,
        platform: ExecutionProviderPlatform,
        capabilities: ExecutionProviderCapabilities,
    ) -> bool {
        match self {
            Self::Cpu => true,
            Self::Xnnpack => {
                matches!(
                    platform,
                    ExecutionProviderPlatform::MacosIntel | ExecutionProviderPlatform::Linux
                ) && capabilities.xnnpack
            }
            Self::CoreMl => {
                platform == ExecutionProviderPlatform::MacosAppleSilicon && capabilities.coreml
            }
            Self::DirectMl => {
                platform == ExecutionProviderPlatform::Windows && capabilities.directml
            }
        }
    }

    pub fn default_for_current_platform() -> Self {
        Self::default_for(ExecutionProviderPlatform::current())
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Xnnpack => "xnnpack",
            Self::CoreMl => "coreml",
            Self::DirectMl => "directml",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cpu" => Some(Self::Cpu),
            "xnnpack" => Some(Self::Xnnpack),
            "coreml" => Some(Self::CoreMl),
            "directml" => Some(Self::DirectMl),
            _ => None,
        }
    }

    pub fn available_for_current_platform() -> Vec<&'static str> {
        Self::available_for(ExecutionProviderPlatform::current())
            .iter()
            .map(|ep| ep.as_str())
            .collect()
    }

    pub fn compatible_for_current_platform() -> Vec<&'static str> {
        let platform = ExecutionProviderPlatform::current();
        let capabilities = ExecutionProviderCapabilities::current();
        Self::available_for(platform)
            .iter()
            .copied()
            .filter(|provider| provider.is_compatible_for(platform, capabilities))
            .map(Self::as_str)
            .collect()
    }

    pub fn is_compatible_for_current_platform(self) -> bool {
        self.is_compatible_for(
            ExecutionProviderPlatform::current(),
            ExecutionProviderCapabilities::current(),
        )
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_art_backdrop: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_upgrade_all: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_variant: Option<ModelVariant>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics_font_step: Option<i8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_provider: Option<ExecutionProviderPreference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eq_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eq_gains_db: Option<[f32; 5]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crossfade_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crossfade_duration_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_sort_mode: Option<LibrarySortMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_preference: Option<ThemePreference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_policy: Option<UpdatePolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_cache_bytes_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending_mirror_restore: bool,
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

    pub fn effective_crossfade_duration_ms(&self) -> u32 {
        self.crossfade_duration_ms
            .unwrap_or(3_000)
            .clamp(500, 10_000)
    }

    pub fn effective_theme_preference(&self) -> ThemePreference {
        self.theme_preference.unwrap_or_default()
    }

    pub fn effective_update_policy(&self) -> UpdatePolicy {
        self.update_policy.unwrap_or_default()
    }
}

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
        };

        save_config(tmp.path(), &config).unwrap();
        let loaded = load_config(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded.libraries.len(), 1);
        assert_eq!(loaded.active_library_id, config.active_library_id);
        assert_eq!(loaded.stem_mode, Some(StemMode::FourStem));
        assert_eq!(loaded.lyrics_font_step, Some(1));
    }

    #[test]
    fn execution_provider_policy_holds_for_every_platform() {
        use ExecutionProviderPlatform as Platform;
        use ExecutionProviderPreference as Ep;

        assert!(Ep::DirectMl.is_available_for(Platform::Windows));
        for platform in [
            Platform::MacosAppleSilicon,
            Platform::MacosIntel,
            Platform::Linux,
            Platform::Other,
        ] {
            assert!(
                !Ep::DirectMl.is_available_for(platform),
                "DirectML must not be offered on {platform:?}"
            );
        }

        for platform in [
            Platform::Windows,
            Platform::MacosAppleSilicon,
            Platform::MacosIntel,
            Platform::Linux,
        ] {
            assert!(Ep::Cpu.is_available_for(platform));
            assert!(Ep::Xnnpack.is_available_for(platform));
        }
        assert!(Ep::Cpu.is_available_for(Platform::Other));
        assert!(Ep::CoreMl.is_available_for(Platform::MacosAppleSilicon));
        assert!(!Ep::CoreMl.is_available_for(Platform::MacosIntel));

        assert_eq!(Ep::default_for(Platform::MacosAppleSilicon), Ep::CoreMl);
        assert_eq!(
            Ep::default_for(Platform::Windows),
            if crate::platform_capabilities::directml_available() {
                Ep::DirectMl
            } else {
                Ep::Cpu
            }
        );
        for platform in [Platform::MacosIntel, Platform::Linux, Platform::Other] {
            assert_eq!(
                Ep::default_for(platform),
                Ep::Cpu,
                "{platform:?} must default to the ORT CPU EP"
            );
        }

        for platform in [
            Platform::Windows,
            Platform::MacosAppleSilicon,
            Platform::MacosIntel,
            Platform::Linux,
            Platform::Other,
        ] {
            assert!(Ep::default_for(platform).is_available_for(platform));
        }
    }

    #[test]
    fn windows_auto_selection_requires_a_hardware_capability() {
        use ExecutionProviderPlatform::Windows;
        use ExecutionProviderPreference as Ep;

        let hardware_available = ExecutionProviderCapabilities {
            directml: true,
            coreml: false,
            xnnpack: false,
        };
        let hardware_unavailable = ExecutionProviderCapabilities {
            directml: false,
            coreml: false,
            xnnpack: false,
        };
        assert_eq!(
            Ep::default_for_capabilities(Windows, hardware_available),
            Ep::DirectMl
        );
        assert_eq!(
            Ep::default_for_capabilities(Windows, hardware_unavailable),
            Ep::Cpu
        );
        assert_eq!(
            Ep::default_for_capabilities(ExecutionProviderPlatform::Linux, hardware_available),
            Ep::Cpu
        );
    }

    #[test]
    fn effective_stem_mode_defaults_to_four_stem() {
        let config = AppConfig::default();
        assert_eq!(config.effective_stem_mode(), StemMode::FourStem);
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
            hide_upgrade_all: None,
            model_variant: None,
            lyrics_font_step: None,
            execution_provider: None,
            eq_enabled: None,
            eq_gains_db: None,
            crossfade_enabled: None,
            crossfade_duration_ms: None,
            library_sort_mode: None,
            theme_preference: None,
            update_policy: None,
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
            hide_upgrade_all: None,
            model_variant: None,
            lyrics_font_step: None,
            execution_provider: None,
            eq_enabled: None,
            eq_gains_db: None,
            crossfade_enabled: None,
            crossfade_duration_ms: None,
            library_sort_mode: None,
            theme_preference: None,
            update_policy: None,
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
            hide_upgrade_all: None,
            model_variant: None,
            lyrics_font_step: None,
            execution_provider: None,
            eq_enabled: None,
            eq_gains_db: None,
            crossfade_enabled: None,
            crossfade_duration_ms: None,
            library_sort_mode: None,
            theme_preference: None,
            update_policy: None,
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
            update_policy: None,
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
            hide_upgrade_all: None,
            model_variant: Some(ModelVariant::HtdemucsFt),
            lyrics_font_step: Some(1),
            execution_provider: None,
            eq_enabled: None,
            eq_gains_db: None,
            crossfade_enabled: None,
            crossfade_duration_ms: None,
            library_sort_mode: None,
            theme_preference: None,
            update_policy: None,
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

        // Host-conditional expectations mirror the runtime artifact matrix:
        // Windows selects DirectML only with a D3D12 hardware adapter, Apple
        // Silicon selects CoreML, and other hosts use CPU.
        #[cfg(target_os = "windows")]
        assert_eq!(
            config.effective_execution_provider(),
            ExecutionProviderPreference::default_for_current_platform()
        );

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert_eq!(
            config.effective_execution_provider(),
            ExecutionProviderPreference::CoreMl
        );

        #[cfg(not(any(
            target_os = "windows",
            all(target_os = "macos", target_arch = "aarch64")
        )))]
        assert_eq!(
            config.effective_execution_provider(),
            ExecutionProviderPreference::Cpu
        );
    }

    #[test]
    fn execution_provider_available_table_is_exact_and_ordered() {
        use ExecutionProviderPlatform::*;

        assert_eq!(
            ExecutionProviderPreference::available_for(MacosAppleSilicon),
            &[
                ExecutionProviderPreference::Cpu,
                ExecutionProviderPreference::CoreMl,
                ExecutionProviderPreference::Xnnpack,
            ]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(MacosIntel),
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
            &[ExecutionProviderPreference::Cpu]
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
        for &platform in &[MacosAppleSilicon, MacosIntel, Linux, Other] {
            assert!(!ExecutionProviderPreference::available_for(platform)
                .contains(&ExecutionProviderPreference::DirectMl));
        }
        assert!(ExecutionProviderPreference::available_for(Windows)
            .contains(&ExecutionProviderPreference::DirectMl));
    }

    #[test]
    fn cpu_is_present_for_every_target() {
        use ExecutionProviderPlatform::*;
        for &platform in &[MacosAppleSilicon, MacosIntel, Windows, Linux, Other] {
            let list = ExecutionProviderPreference::available_for(platform);
            assert!(list.contains(&ExecutionProviderPreference::Cpu));
        }
    }

    #[test]
    fn provider_capabilities_match_runtime_artifacts() {
        use ExecutionProviderPlatform::*;
        use ExecutionProviderPreference as Ep;

        let available = ExecutionProviderCapabilities {
            directml: true,
            coreml: true,
            xnnpack: true,
        };
        assert_eq!(
            Ep::default_for_capabilities(MacosAppleSilicon, available),
            Ep::CoreMl
        );
        assert!(Ep::CoreMl.is_compatible_for(MacosAppleSilicon, available));
        assert!(!Ep::CoreMl.is_compatible_for(MacosIntel, available));
        assert!(Ep::Xnnpack.is_compatible_for(Linux, available));
        assert!(!Ep::Xnnpack.is_compatible_for(Windows, available));
        assert!(Ep::DirectMl.is_compatible_for(Windows, available));
        assert!(!Ep::DirectMl.is_compatible_for(Linux, available));

        let unavailable = ExecutionProviderCapabilities {
            directml: false,
            coreml: false,
            xnnpack: false,
        };
        assert_eq!(
            Ep::default_for_capabilities(MacosAppleSilicon, unavailable),
            Ep::Cpu
        );
        assert!(!Ep::CoreMl.is_compatible_for(MacosAppleSilicon, unavailable));
        assert!(!Ep::Xnnpack.is_compatible_for(Linux, unavailable));
        assert!(!Ep::DirectMl.is_compatible_for(Windows, unavailable));
    }

    #[test]
    fn library_sort_mode_none_is_omitted_from_json() {
        let config = AppConfig {
            library_sort_mode: None,
            theme_preference: None,
            update_policy: None,
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
                update_policy: None,
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
        for &platform in &[MacosAppleSilicon, MacosIntel, Windows, Linux, Other] {
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
            config.effective_execution_provider_for(MacosAppleSilicon),
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
            config.effective_execution_provider_for(MacosAppleSilicon),
            ExecutionProviderPreference::CoreMl
        );
        assert_eq!(
            config.effective_execution_provider_for(MacosIntel),
            ExecutionProviderPreference::Cpu
        );
        assert_eq!(
            config.effective_execution_provider_for(Linux),
            ExecutionProviderPreference::Cpu
        );
        assert_eq!(
            config.effective_execution_provider_for(Other),
            ExecutionProviderPreference::Cpu
        );
        assert_eq!(
            config.effective_execution_provider_for(Windows),
            ExecutionProviderPreference::DirectMl
        );
    }

    #[test]
    fn effective_eq_gains_clamps_out_of_range_persisted_values() {
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
            config.effective_execution_provider_for(MacosAppleSilicon),
            ExecutionProviderPreference::CoreMl
        );
        assert_eq!(
            config.effective_execution_provider_for(Windows),
            ExecutionProviderPreference::default_for(Windows)
        );
        assert_eq!(
            config.effective_execution_provider_for(MacosIntel),
            ExecutionProviderPreference::Cpu
        );
        assert_eq!(
            config.effective_execution_provider_for(Linux),
            ExecutionProviderPreference::Cpu
        );
    }

    #[test]
    fn current_target_wrappers_agree_with_compile_target() {
        use ExecutionProviderPlatform::*;
        let current = ExecutionProviderPlatform::current();
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert_eq!(current, MacosAppleSilicon);
        #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
        assert_eq!(current, MacosIntel);
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
            update_policy: None,
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
