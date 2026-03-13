use anyhow::{Context, Result};
use lofty::{
    file::{AudioFile, TaggedFileExt},
    prelude::Accessor,
    probe::Probe,
};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: i64,
    pub cover_art: Option<Vec<u8>>,
}

pub fn read_from_path(path: &Path) -> Result<SongMetadata> {
    let tagged_file = Probe::open(path)
        .with_context(|| format!("failed to open audio file at {}", path.display()))?
        .read()
        .with_context(|| format!("failed to read audio metadata from {}", path.display()))?;

    let properties = tagged_file.properties();
    let duration_ms = properties.duration().as_millis() as i64;
    let primary_tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());
    let cover_art = primary_tag
        .and_then(|tag| tag.pictures().first())
        .map(|picture| picture.data().to_vec());

    Ok(SongMetadata {
        title: primary_tag.and_then(|tag| tag.title().map(|value| value.into_owned())),
        artist: primary_tag.and_then(|tag| tag.artist().map(|value| value.into_owned())),
        album: primary_tag.and_then(|tag| tag.album().map(|value| value.into_owned())),
        duration_ms,
        cover_art,
    })
}
