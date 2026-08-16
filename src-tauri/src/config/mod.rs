mod execution_provider;
mod library_registry;
mod persistence;
mod preferences;

pub use execution_provider::{
    effective_execution_provider_from_dir, record_directml_unavailable_on_timeout,
    restore_directml_timeout_state, ExecutionProviderPreference,
};
pub use library_registry::{
    library_display_name, library_id_for_path, LibraryKind, RegisteredLibrary,
    RemoteLibraryConnectionConfig, RemoteLibraryProvider,
};
pub use persistence::{load_config, save_config};
pub use preferences::{LibrarySortMode, ModelVariant, StemMode, ThemePreference, UpdatePolicy};

use serde::{Deserialize, Serialize};

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
    pub lyrics_blur_inactive: Option<bool>,
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
    /// Records that this host failed to load a DirectML-linked ONNX Runtime.
    /// Presence is the gate: when set, the platform default avoids DirectML so
    /// the next bootstrap selects a CPU-only runtime. An explicit user choice in
    /// Settings still wins and overrides this default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directml_disabled_by_runtime_timeout: Option<String>,
}
