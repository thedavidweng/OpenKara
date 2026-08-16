use crate::hash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use super::AppConfig;

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

impl AppConfig {
    pub fn normalize_for_save(mut self) -> Self {
        if !self.libraries.is_empty() {
            self.library_path = None;
        }

        let active_is_registered = self.active_library_id.as_deref().is_some_and(|active_id| {
            self.libraries
                .iter()
                .any(|library| library.id() == active_id)
        });
        if !active_is_registered {
            self.active_library_id = self
                .libraries
                .first()
                .map(|library| library.id().to_owned());
        }

        self
    }

    pub fn active_library(&self) -> Option<&RegisteredLibrary> {
        self.active_library_id
            .as_deref()
            .and_then(|active_id| {
                self.libraries
                    .iter()
                    .find(|library| library.id() == active_id)
            })
            .or_else(|| self.libraries.first())
    }
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

/// Registers a pre-registry `library_path` as the first library entry.
/// `normalize_for_save` then makes it active. The fallback name for a path
/// with no final component is deliberately "OpenKara Library", not the raw
/// path that `library_display_name` would produce: this is what the shipped
/// migration has always written.
pub(super) fn migrate_legacy_library_path(config: &mut AppConfig) {
    if !config.libraries.is_empty() {
        return;
    }
    let Some(path) = config.library_path.clone() else {
        return;
    };
    let display_name = Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("OpenKara Library")
        .to_owned();
    config
        .libraries
        .push(RegisteredLibrary::local(path, display_name));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::persistence::CONFIG_FILENAME;
    use crate::config::{load_config, save_config, ModelVariant, StemMode};
    use std::fs;

    #[test]
    fn legacy_library_path_is_migrated_to_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = AppConfig {
            library_path: Some("/Users/test/Music/Legacy".to_owned()),
            stem_mode: Some(StemMode::TwoStem),
            language: Some("zh-CN".to_owned()),
            hide_batch_separate: Some(true),
            cover_art_backdrop: None,
            lyrics_blur_inactive: None,
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
            directml_disabled_by_runtime_timeout: None,
        };

        save_config(tmp.path(), &legacy).unwrap();
        let loaded = load_config(tmp.path()).unwrap().unwrap();

        assert!(loaded.library_path.is_none());
        assert_eq!(loaded.libraries.len(), 1);
        assert_eq!(loaded.active_library(), loaded.libraries.first());
    }

    #[test]
    fn legacy_library_path_without_final_component_gets_fallback_name() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join(CONFIG_FILENAME),
            r#"{ "library_path": "/" }"#,
        )
        .unwrap();

        let loaded = load_config(tmp.path()).unwrap().unwrap();

        assert_eq!(loaded.libraries.len(), 1);
        assert_eq!(loaded.libraries[0].display_name(), "OpenKara Library");
        assert_eq!(loaded.active_library(), loaded.libraries.first());
    }

    #[test]
    fn dangling_active_library_id_is_repointed_on_save() {
        let library = RegisteredLibrary::local("/Users/test/Music/A".to_owned(), "A".to_owned());
        let normalized = AppConfig {
            libraries: vec![library.clone()],
            active_library_id: Some("library-missing".to_owned()),
            ..AppConfig::default()
        }
        .normalize_for_save();

        assert_eq!(normalized.active_library_id.as_deref(), Some(library.id()));
    }

    #[test]
    fn dangling_active_library_id_is_dropped_when_registry_is_empty() {
        let normalized = AppConfig {
            active_library_id: Some("library-missing".to_owned()),
            ..AppConfig::default()
        }
        .normalize_for_save();

        assert!(normalized.active_library_id.is_none());
    }

    #[test]
    fn registered_active_library_id_is_kept_on_save() {
        let first = RegisteredLibrary::local("/Users/test/Music/A".to_owned(), "A".to_owned());
        let second = RegisteredLibrary::local("/Users/test/Music/B".to_owned(), "B".to_owned());
        let normalized = AppConfig {
            libraries: vec![first, second.clone()],
            active_library_id: Some(second.id().to_owned()),
            ..AppConfig::default()
        }
        .normalize_for_save();

        assert_eq!(normalized.active_library_id.as_deref(), Some(second.id()));
    }

    #[test]
    fn active_library_falls_back_to_first_when_id_is_dangling() {
        let library = RegisteredLibrary::local("/Users/test/Music/A".to_owned(), "A".to_owned());
        let config = AppConfig {
            libraries: vec![library.clone()],
            active_library_id: Some("library-missing".to_owned()),
            ..AppConfig::default()
        };

        assert_eq!(config.active_library(), Some(&library));
    }

    #[test]
    fn dangling_active_library_id_is_repaired_on_load() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = r#"{
          "libraries": [
            { "kind": "local", "id": "library-1", "display_name": "A", "root_path": "/tmp/a" }
          ],
          "active_library_id": "library-gone"
        }"#;
        fs::write(tmp.path().join(CONFIG_FILENAME), raw).unwrap();

        let loaded = load_config(tmp.path()).unwrap().unwrap();

        assert_eq!(loaded.active_library_id.as_deref(), Some("library-1"));
        assert_eq!(
            loaded.active_library().map(RegisteredLibrary::id),
            Some("library-1")
        );
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
}
