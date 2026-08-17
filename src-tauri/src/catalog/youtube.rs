use super::types::{youtube_queue_id, CatalogError, VideoQueueItem, VideoUnavailableReason};
use super::video::VideoSource;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YoutubeTarget {
    Watch { video_id: String },
    Playlist { playlist_id: String },
}

pub fn parse_youtube_url(url: &str) -> Result<YoutubeTarget, CatalogError> {
    let url = url.trim();
    let parsed = reqwest::Url::parse(url).map_err(|_| CatalogError::VideoUnavailable {
        reason: VideoUnavailableReason::InvalidUrl,
    })?;
    let host = parsed.host_str().unwrap_or_default();
    let is_youtube = host == "youtu.be"
        || host == "www.youtube.com"
        || host == "youtube.com"
        || host == "m.youtube.com"
        || host == "music.youtube.com";
    if !is_youtube {
        return Err(CatalogError::VideoUnavailable {
            reason: VideoUnavailableReason::InvalidUrl,
        });
    }

    if host == "youtu.be" {
        let video_id = parsed
            .path_segments()
            .and_then(|mut parts| parts.next())
            .filter(|id| !id.is_empty())
            .ok_or(CatalogError::VideoUnavailable {
                reason: VideoUnavailableReason::InvalidUrl,
            })?;
        return Ok(YoutubeTarget::Watch {
            video_id: video_id.to_owned(),
        });
    }

    if parsed.path() == "/playlist" {
        let playlist_id = query_param(&parsed, "list").ok_or(CatalogError::VideoUnavailable {
            reason: VideoUnavailableReason::InvalidUrl,
        })?;
        return Ok(YoutubeTarget::Playlist { playlist_id });
    }

    if parsed.path() == "/watch" {
        let video_id = query_param(&parsed, "v").ok_or(CatalogError::VideoUnavailable {
            reason: VideoUnavailableReason::InvalidUrl,
        })?;
        return Ok(YoutubeTarget::Watch { video_id });
    }

    Err(CatalogError::VideoUnavailable {
        reason: VideoUnavailableReason::InvalidUrl,
    })
}

fn query_param(url: &reqwest::Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

pub trait YoutubePageFetcher {
    fn fetch_text(&self, url: &str) -> Result<String, CatalogError>;
}

pub struct YoutubeVideoSource<F> {
    fetcher: F,
}

impl<F: YoutubePageFetcher + Send + Sync> YoutubeVideoSource<F> {
    pub fn new(fetcher: F) -> Self {
        Self { fetcher }
    }
}

impl<F: YoutubePageFetcher + Send + Sync> VideoSource for YoutubeVideoSource<F> {
    fn source_id(&self) -> &str {
        "youtube"
    }

    fn resolve(&self, url: &str) -> Result<Vec<VideoQueueItem>, CatalogError> {
        if url.contains("/youtubei/v1/player") || url.contains("/player?") {
            return Err(CatalogError::Internal(
                "YouTube /player stream URLs are not used".to_owned(),
            ));
        }
        match parse_youtube_url(url)? {
            YoutubeTarget::Watch { video_id } => {
                let watch_url = format!("https://www.youtube.com/watch?v={video_id}");
                let html = self.fetcher.fetch_text(&watch_url)?;
                reject_unplayable(&html)?;
                Ok(vec![item_from_watch_html(&html, &video_id, &watch_url)])
            }
            YoutubeTarget::Playlist { playlist_id } => {
                let playlist_url = format!("https://www.youtube.com/playlist?list={playlist_id}");
                let html = self.fetcher.fetch_text(&playlist_url)?;
                reject_unplayable(&html)?;
                let items = items_from_playlist_html(&html);
                if items.is_empty() {
                    return Err(CatalogError::VideoUnavailable {
                        reason: VideoUnavailableReason::Unavailable,
                    });
                }
                Ok(items)
            }
        }
    }
}

fn reject_unplayable(html: &str) -> Result<(), CatalogError> {
    if html.contains("\"status\":\"LOGIN_REQUIRED\"")
        || html.contains("age-restricted")
        || html.contains("confirm your age")
    {
        return Err(CatalogError::VideoUnavailable {
            reason: VideoUnavailableReason::AgeRestricted,
        });
    }
    if html.contains("\"status\":\"ERROR\"") && html.contains("private") {
        return Err(CatalogError::VideoUnavailable {
            reason: VideoUnavailableReason::Private,
        });
    }
    if html.contains("unlisted") && html.contains("\"status\":\"UNPLAYABLE\"") {
        return Err(CatalogError::VideoUnavailable {
            reason: VideoUnavailableReason::Unlisted,
        });
    }
    if html.contains("\"status\":\"UNPLAYABLE\"") {
        return Err(CatalogError::VideoUnavailable {
            reason: VideoUnavailableReason::Unavailable,
        });
    }
    Ok(())
}

fn item_from_watch_html(html: &str, video_id: &str, watch_url: &str) -> VideoQueueItem {
    let title = extract_between(html, "<title>", " - YouTube</title>")
        .or_else(|| extract_between(html, "\"title\":\"", "\""))
        .unwrap_or_else(|| video_id.to_owned());
    let channel = extract_between(html, "\"ownerChannelName\":\"", "\"").unwrap_or_default();
    VideoQueueItem {
        id: youtube_queue_id(video_id),
        title,
        channel,
        duration_ms: None,
        thumbnail_url: Some(format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")),
        watch_url: watch_url.to_owned(),
    }
}

fn items_from_playlist_html(html: &str) -> Vec<VideoQueueItem> {
    let mut items = Vec::new();
    let mut seen = HashMap::new();
    let marker = "\"videoId\":\"";
    let mut rest = html;
    while let Some(index) = rest.find(marker) {
        rest = &rest[index + marker.len()..];
        let Some(end) = rest.find('"') else {
            break;
        };
        let video_id = &rest[..end];
        if video_id.len() < 6 || seen.contains_key(video_id) {
            continue;
        }
        seen.insert(video_id.to_owned(), ());
        items.push(VideoQueueItem {
            id: youtube_queue_id(video_id),
            title: video_id.to_owned(),
            channel: String::new(),
            duration_ms: None,
            thumbnail_url: Some(format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")),
            watch_url: format!("https://www.youtube.com/watch?v={video_id}"),
        });
    }
    items
}

fn extract_between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<String> {
    let from = haystack.find(start)? + start.len();
    let tail = haystack.get(from..)?;
    let to = tail.find(end)?;
    Some(tail[..to].to_owned())
}

pub struct LiveYoutubeFetcher {
    client: reqwest::blocking::Client,
}

impl LiveYoutubeFetcher {
    pub fn new() -> Result<Self, CatalogError> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("Mozilla/5.0 OpenKara")
            .build()
            .map_err(|error| CatalogError::Network(error.to_string()))?;
        Ok(Self { client })
    }
}

impl YoutubePageFetcher for LiveYoutubeFetcher {
    fn fetch_text(&self, url: &str) -> Result<String, CatalogError> {
        if url.contains("/youtubei/v1/player") || url.contains("/player?") {
            return Err(CatalogError::Internal(
                "YouTube /player stream URLs are not used".to_owned(),
            ));
        }
        self.client
            .get(url)
            .send()
            .and_then(|response| response.text())
            .map_err(|error| CatalogError::Network(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MapFetcher(HashMap<String, String>);

    impl YoutubePageFetcher for MapFetcher {
        fn fetch_text(&self, url: &str) -> Result<String, CatalogError> {
            assert!(
                !url.contains("/player"),
                "resolver must not request /player"
            );
            self.0
                .get(url)
                .cloned()
                .ok_or(CatalogError::VideoUnavailable {
                    reason: VideoUnavailableReason::Unavailable,
                })
        }
    }

    #[test]
    fn parses_watch_and_playlist_urls() {
        assert_eq!(
            parse_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap(),
            YoutubeTarget::Watch {
                video_id: "dQw4w9WgXcQ".to_owned()
            }
        );
        assert_eq!(
            parse_youtube_url("https://youtu.be/dQw4w9WgXcQ").unwrap(),
            YoutubeTarget::Watch {
                video_id: "dQw4w9WgXcQ".to_owned()
            }
        );
        assert_eq!(
            parse_youtube_url("https://www.youtube.com/playlist?list=PLabc").unwrap(),
            YoutubeTarget::Playlist {
                playlist_id: "PLabc".to_owned()
            }
        );
    }

    #[test]
    fn fake_pages_resolve_without_player() {
        let mut pages = HashMap::new();
        pages.insert(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned(),
            "<title>Never - YouTube</title>\"ownerChannelName\":\"Rick\"".to_owned(),
        );
        pages.insert(
            "https://www.youtube.com/playlist?list=PLabc".to_owned(),
            "\"videoId\":\"aaaaaaaaaaa\" \"videoId\":\"bbbbbbbbbbb\"".to_owned(),
        );
        let source = YoutubeVideoSource::new(MapFetcher(pages));
        let watch = source
            .resolve("https://youtu.be/dQw4w9WgXcQ")
            .expect("watch");
        assert_eq!(watch.len(), 1);
        assert_eq!(watch[0].id, "yt:dQw4w9WgXcQ");
        let playlist = source
            .resolve("https://www.youtube.com/playlist?list=PLabc")
            .expect("playlist");
        assert_eq!(playlist.len(), 2);
    }

    #[test]
    fn age_restricted_is_typed() {
        let mut pages = HashMap::new();
        pages.insert(
            "https://www.youtube.com/watch?v=agegatevid1".to_owned(),
            "\"status\":\"LOGIN_REQUIRED\" confirm your age".to_owned(),
        );
        let source = YoutubeVideoSource::new(MapFetcher(pages));
        assert!(matches!(
            source.resolve("https://www.youtube.com/watch?v=agegatevid1"),
            Err(CatalogError::VideoUnavailable {
                reason: VideoUnavailableReason::AgeRestricted
            })
        ));
    }
}
