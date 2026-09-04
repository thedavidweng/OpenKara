use super::types::{OnlineSourceCapabilities, NETEASE_SOURCE_ID, YOUTUBE_SOURCE_ID};
use crate::config::AppConfig;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnlineSourceKind {
    Video,
    Streaming,
}

impl OnlineSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Streaming => "streaming",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OnlineSourceSnapshot {
    pub id: String,
    pub kind: String,
    pub enabled: bool,
    pub capabilities: OnlineSourceCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownOnlineSource {
    pub source_id: String,
}

pub fn list_online_sources(config: &AppConfig) -> Vec<OnlineSourceSnapshot> {
    vec![
        snapshot(
            YOUTUBE_SOURCE_ID,
            OnlineSourceKind::Video,
            config.effective_youtube_source_enabled(),
        ),
        snapshot(
            NETEASE_SOURCE_ID,
            OnlineSourceKind::Streaming,
            config.effective_netease_source_enabled(),
        ),
    ]
}

pub fn set_online_source_enabled(
    config: &mut AppConfig,
    source_id: &str,
    enabled: bool,
) -> Result<(), UnknownOnlineSource> {
    match source_id {
        YOUTUBE_SOURCE_ID => config.youtube_source_enabled = Some(enabled),
        NETEASE_SOURCE_ID => config.netease_source_enabled = Some(enabled),
        _ => {
            return Err(UnknownOnlineSource {
                source_id: source_id.to_owned(),
            });
        }
    }
    Ok(())
}

fn snapshot(id: &str, kind: OnlineSourceKind, enabled: bool) -> OnlineSourceSnapshot {
    OnlineSourceSnapshot {
        id: id.to_owned(),
        kind: kind.as_str().to_owned(),
        enabled,
        capabilities: OnlineSourceCapabilities::for_source(id, enabled),
    }
}

pub fn require_enabled(
    config: &AppConfig,
    source_id: &str,
) -> Result<OnlineSourceSnapshot, super::types::CatalogError> {
    let sources = list_online_sources(config);
    let Some(source) = sources.into_iter().find(|source| source.id == source_id) else {
        return Err(super::types::CatalogError::UnknownSource {
            source_id: source_id.to_owned(),
        });
    };
    if !source.enabled {
        return Err(super::types::CatalogError::SourceDisabled {
            source_id: source_id.to_owned(),
        });
    }
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_both_sources_off() {
        let sources = list_online_sources(&AppConfig::default());
        assert_eq!(sources[0].id, "youtube");
        assert_eq!(sources[0].kind, "video");
        assert!(!sources[0].enabled);
        assert!(!sources[0].capabilities.resolve_video);
        assert_eq!(sources[1].id, "netease");
        assert_eq!(sources[1].kind, "streaming");
        assert!(!sources[1].enabled);
        assert!(!sources[1].capabilities.browse);
    }

    #[test]
    fn enabled_source_reports_capabilities() {
        let mut config = AppConfig::default();
        set_online_source_enabled(&mut config, "youtube", true).expect("known");
        set_online_source_enabled(&mut config, "netease", true).expect("known");
        let sources = list_online_sources(&config);
        assert!(sources[0].capabilities.resolve_video);
        assert!(sources[1].capabilities.sign_in);
        assert!(sources[1].capabilities.browse);
        assert!(sources[1].capabilities.import);
        assert!(!sources[1].capabilities.resolve_video);
    }

    #[test]
    fn disabled_source_is_rejected() {
        let config = AppConfig::default();
        assert!(matches!(
            require_enabled(&config, "netease"),
            Err(crate::catalog::CatalogError::SourceDisabled { .. })
        ));
    }

    #[test]
    fn flags_persist_across_settings_reload() {
        let tmp = tempfile::tempdir().expect("tmp");
        let mut config = AppConfig::default();
        set_online_source_enabled(&mut config, "youtube", true).expect("known");
        crate::config::save_config(tmp.path(), &config).expect("save");
        let loaded = crate::config::load_config(tmp.path())
            .expect("load")
            .expect("present");
        let sources = list_online_sources(&loaded);
        assert!(sources[0].enabled);
        assert!(!sources[1].enabled);
    }

    #[test]
    fn enabling_one_source_does_not_enable_the_other() {
        let mut config = AppConfig::default();
        set_online_source_enabled(&mut config, "netease", true).expect("known source");
        let sources = list_online_sources(&config);
        assert!(!sources[0].enabled);
        assert!(sources[1].enabled);
    }

    #[test]
    fn unknown_source_is_rejected() {
        let mut config = AppConfig::default();
        let error = set_online_source_enabled(&mut config, "kugou", true).unwrap_err();
        assert_eq!(error.source_id, "kugou");
        assert!(!config.effective_netease_source_enabled());
    }
}
