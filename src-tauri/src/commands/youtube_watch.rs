use crate::commands::error::{internal_error, video_source_unavailable, CommandResult};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const YOUTUBE_WATCH_WEBVIEW_LABEL: &str = "youtube-watch";

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum YoutubeWatchAction {
    Play,
    Pause,
    Seek { ms: u64 },
    SetVolume { level: f32 },
    Query,
    Navigate { url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct YoutubeWatchMediaState {
    pub ended: bool,
    pub paused: bool,
    pub current_time_ms: u64,
    pub duration_ms: Option<u64>,
}

pub fn validate_youtube_watch_url(url: &str) -> CommandResult<()> {
    if url.contains("/player") {
        return Err(video_source_unavailable(
            "YouTube /player stream URLs are not used",
        ));
    }
    let rest = url
        .strip_prefix("https://www.youtube.com/watch?")
        .or_else(|| url.strip_prefix("https://youtube.com/watch?"))
        .or_else(|| url.strip_prefix("https://m.youtube.com/watch?"))
        .ok_or_else(|| {
            video_source_unavailable("YouTube watch URL must be a public https watch page")
        })?;
    let video_id = rest.split('&').find_map(|pair| {
        pair.strip_prefix("v=").filter(|id| {
            !id.is_empty()
                && id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        })
    });
    if video_id.is_none() {
        return Err(video_source_unavailable(
            "YouTube watch URL is missing a public video id",
        ));
    }
    Ok(())
}

const KASET_WATCH_HELPERS: &str = r#"
  function moviePlayer() { return document.getElementById('movie_player'); }
  function videoEl() {
    return document.querySelector('#movie_player video') || document.querySelector('video');
  }
  function mediaState(video) {
    if (!video) {
      return { ended: false, paused: true, current_time_ms: 0, duration_ms: null };
    }
    const durationMs = Number.isFinite(video.duration) ? Math.round(video.duration * 1000) : null;
    return {
      ended: !!video.ended,
      paused: !!video.paused,
      current_time_ms: Math.round((video.currentTime || 0) * 1000),
      duration_ms: durationMs
    };
  }
"#;

pub fn youtube_watch_script(action: &YoutubeWatchAction) -> CommandResult<String> {
    let body = match action {
        YoutubeWatchAction::Play => {
            "const video = videoEl(); if (video && video.paused) { video.play(); } return JSON.stringify(mediaState(videoEl()));"
                .to_owned()
        }
        YoutubeWatchAction::Pause => {
            "const video = videoEl(); if (video && !video.paused) { video.pause(); } return JSON.stringify(mediaState(videoEl()));"
                .to_owned()
        }
        YoutubeWatchAction::Seek { ms } => {
            format!(
                "const video = videoEl(); if (video) {{ video.currentTime = {seconds}; }} return JSON.stringify(mediaState(videoEl()));",
                seconds = (*ms as f64) / 1000.0
            )
        }
        YoutubeWatchAction::SetVolume { level } => {
            let clamped = level.clamp(0.0, 1.0);
            format!(
                r#"
  const video = videoEl();
  if (video) {{
    if ({clamped} <= 0) {{ video.muted = true; video.volume = 0; }}
    else {{ video.volume = {clamped}; video.muted = false; }}
  }}
  const player = moviePlayer();
  if (player && player.setVolume) {{ player.setVolume({percent}); }}
  return JSON.stringify(mediaState(videoEl()));
"#,
                percent = (clamped * 100.0).round()
            )
        }
        YoutubeWatchAction::Query => {
            "return JSON.stringify(mediaState(videoEl()));".to_owned()
        }
        YoutubeWatchAction::Navigate { url } => {
            validate_youtube_watch_url(url)?;
            let encoded = serde_json::to_string(url)
                .map_err(|error| internal_error(format!("failed to encode watch URL: {error}")))?;
            return Ok(format!(
                r#"(function() {{ location.assign({encoded}); return JSON.stringify({{ ended: false, paused: true, current_time_ms: 0, duration_ms: null }}); }})()"#
            ));
        }
    };

    Ok(format!("(function() {{ {KASET_WATCH_HELPERS} {body} }})()"))
}

#[tauri::command]
pub async fn control_youtube_watch(
    app_handle: AppHandle,
    action: YoutubeWatchAction,
) -> CommandResult<YoutubeWatchMediaState> {
    let script = youtube_watch_script(&action)?;
    let Some(webview) = app_handle.get_webview(YOUTUBE_WATCH_WEBVIEW_LABEL) else {
        return Ok(YoutubeWatchMediaState {
            ended: false,
            paused: true,
            current_time_ms: 0,
            duration_ms: None,
        });
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = std::sync::Mutex::new(Some(tx));
    webview
        .eval_with_callback(script, move |result| {
            if let Ok(mut sender) = tx.lock() {
                if let Some(sender) = sender.take() {
                    let _ = sender.send(result);
                }
            }
        })
        .map_err(|error| {
            internal_error(format!("failed to control YouTube watch page: {error}"))
        })?;

    let payload = rx
        .await
        .map_err(|_| internal_error("YouTube watch page did not answer"))?;
    serde_json::from_str(&payload).or_else(|_| {
        Ok(YoutubeWatchMediaState {
            ended: false,
            paused: true,
            current_time_ms: 0,
            duration_ms: None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{validate_youtube_watch_url, youtube_watch_script, YoutubeWatchAction};

    #[test]
    fn rejects_player_stream_urls() {
        let error = validate_youtube_watch_url(
            "https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
        )
        .expect_err("player URLs must fail");
        assert!(error.message.contains("/player"));
    }

    #[test]
    fn accepts_public_watch_urls() {
        validate_youtube_watch_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
            .expect("watch URL");
    }

    #[test]
    fn navigate_script_does_not_mention_player_api() {
        let script = youtube_watch_script(&YoutubeWatchAction::Navigate {
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned(),
        })
        .expect("script");
        assert!(script.contains("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(!script.contains("/player"));
    }

    #[test]
    fn play_uses_kaset_movie_player_video_selector() {
        let script = youtube_watch_script(&YoutubeWatchAction::Play).expect("script");
        let movie = script
            .find("#movie_player video")
            .expect("Kaset primary selector");
        let fallback = script.find("querySelector('video')").expect("fallback");
        assert!(movie < fallback);
        assert!(!script.contains("/player"));
    }
}
