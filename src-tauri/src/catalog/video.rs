use super::types::{CatalogError, VideoQueueItem, VideoUnavailableReason};
use std::collections::HashMap;

pub trait VideoSource: Send + Sync {
    fn source_id(&self) -> &str;
    fn resolve(&self, url: &str) -> Result<Vec<VideoQueueItem>, CatalogError>;
}

pub struct GatedVideoSource<S> {
    enabled: bool,
    inner: S,
}

impl<S: VideoSource> GatedVideoSource<S> {
    pub fn new(enabled: bool, inner: S) -> Self {
        Self { enabled, inner }
    }
}

impl<S: VideoSource> VideoSource for GatedVideoSource<S> {
    fn source_id(&self) -> &str {
        self.inner.source_id()
    }

    fn resolve(&self, url: &str) -> Result<Vec<VideoQueueItem>, CatalogError> {
        if !self.enabled {
            return Err(CatalogError::SourceDisabled {
                source_id: self.inner.source_id().to_owned(),
            });
        }
        self.inner.resolve(url)
    }
}

#[derive(Clone)]
pub struct FakeVideoPage {
    pub items: Vec<VideoQueueItem>,
    pub error: Option<VideoUnavailableReason>,
}

pub struct FakeVideoSource {
    source_id: String,
    pages: HashMap<String, FakeVideoPage>,
}

impl FakeVideoSource {
    pub fn new(source_id: impl Into<String>) -> Self {
        Self {
            source_id: source_id.into(),
            pages: HashMap::new(),
        }
    }

    pub fn insert(&mut self, url: impl Into<String>, page: FakeVideoPage) {
        self.pages.insert(url.into(), page);
    }
}

impl VideoSource for FakeVideoSource {
    fn source_id(&self) -> &str {
        &self.source_id
    }

    fn resolve(&self, url: &str) -> Result<Vec<VideoQueueItem>, CatalogError> {
        let Some(page) = self.pages.get(url) else {
            return Err(CatalogError::VideoUnavailable {
                reason: VideoUnavailableReason::InvalidUrl,
            });
        };
        if let Some(reason) = page.error {
            return Err(CatalogError::VideoUnavailable { reason });
        }
        Ok(page.items.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::types::youtube_queue_id;

    fn item(id: &str, title: &str) -> VideoQueueItem {
        VideoQueueItem {
            id: youtube_queue_id(id),
            title: title.to_owned(),
            channel: "Channel".to_owned(),
            duration_ms: Some(60_000),
            thumbnail_url: None,
            watch_url: format!("https://www.youtube.com/watch?v={id}"),
        }
    }

    #[test]
    fn watch_url_becomes_one_item_playlist_expands() {
        let mut source = FakeVideoSource::new("youtube");
        source.insert(
            "https://www.youtube.com/watch?v=aaaaaaaaaaa",
            FakeVideoPage {
                items: vec![item("aaaaaaaaaaa", "One")],
                error: None,
            },
        );
        source.insert(
            "https://www.youtube.com/playlist?list=PLpublic",
            FakeVideoPage {
                items: vec![item("aaaaaaaaaaa", "One"), item("bbbbbbbbbbb", "Two")],
                error: None,
            },
        );
        let watch = source
            .resolve("https://www.youtube.com/watch?v=aaaaaaaaaaa")
            .expect("watch");
        assert_eq!(watch.len(), 1);
        assert_eq!(watch[0].id, "yt:aaaaaaaaaaa");
        let playlist = source
            .resolve("https://www.youtube.com/playlist?list=PLpublic")
            .expect("playlist");
        assert_eq!(playlist.len(), 2);
        assert_eq!(playlist[1].id, "yt:bbbbbbbbbbb");
    }

    #[test]
    fn private_and_age_gated_fail_with_typed_reason() {
        let mut source = FakeVideoSource::new("youtube");
        source.insert(
            "https://www.youtube.com/watch?v=privatevid1",
            FakeVideoPage {
                items: vec![],
                error: Some(VideoUnavailableReason::Private),
            },
        );
        source.insert(
            "https://www.youtube.com/watch?v=agegatevid1",
            FakeVideoPage {
                items: vec![],
                error: Some(VideoUnavailableReason::AgeRestricted),
            },
        );
        assert!(matches!(
            source.resolve("https://www.youtube.com/watch?v=privatevid1"),
            Err(CatalogError::VideoUnavailable {
                reason: VideoUnavailableReason::Private
            })
        ));
        assert!(matches!(
            source.resolve("https://www.youtube.com/watch?v=agegatevid1"),
            Err(CatalogError::VideoUnavailable {
                reason: VideoUnavailableReason::AgeRestricted
            })
        ));
    }

    #[test]
    fn disabled_source_rejects_resolve() {
        let source = FakeVideoSource::new("youtube");
        let gated = GatedVideoSource::new(false, source);
        assert!(matches!(
            gated.resolve("https://www.youtube.com/watch?v=aaaaaaaaaaa"),
            Err(CatalogError::SourceDisabled { .. })
        ));
    }
}
