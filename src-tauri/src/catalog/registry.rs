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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownOnlineSource {
    pub source_id: String,
}

pub fn list_online_sources(config: &AppConfig) -> Vec<OnlineSourceSnapshot> {
    vec![
        OnlineSourceSnapshot {
            id: "youtube".to_owned(),
            kind: OnlineSourceKind::Video.as_str().to_owned(),
            enabled: config.effective_youtube_source_enabled(),
        },
        OnlineSourceSnapshot {
            id: "netease".to_owned(),
            kind: OnlineSourceKind::Streaming.as_str().to_owned(),
            enabled: config.effective_netease_source_enabled(),
        },
    ]
}

pub fn set_online_source_enabled(
    config: &mut AppConfig,
    source_id: &str,
    enabled: bool,
) -> Result<(), UnknownOnlineSource> {
    match source_id {
        "youtube" => config.youtube_source_enabled = Some(enabled),
        "netease" => config.netease_source_enabled = Some(enabled),
        _ => {
            return Err(UnknownOnlineSource {
                source_id: source_id.to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_both_sources_off() {
        let sources = list_online_sources(&AppConfig::default());
        assert_eq!(
            sources,
            vec![
                OnlineSourceSnapshot {
                    id: "youtube".to_owned(),
                    kind: "video".to_owned(),
                    enabled: false,
                },
                OnlineSourceSnapshot {
                    id: "netease".to_owned(),
                    kind: "streaming".to_owned(),
                    enabled: false,
                },
            ]
        );
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
