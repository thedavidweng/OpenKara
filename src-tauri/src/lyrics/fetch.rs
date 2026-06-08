use crate::{
    library::Song,
    lyrics::{
        lrcapi::LrcApiClient,
        lrclib::{LrcLibClient, LyricsLookupQuery},
        lys_parser, parser, ttml_parser,
    },
    metadata,
};
use anyhow::{Context, Result};
use lofty::{file::TaggedFileExt, tag::ItemKey};
use serde::Serialize;
use std::{fs, path::Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricsSource {
    LrcLib,
    LrcApi,
    LrcApiTtml,
    Embedded,
    Sidecar,
    SidecarTtml,
    SidecarLys,
    Manual,
    ManualTtml,
    ManualLys,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricsFetchResult {
    pub source: LyricsSource,
    pub raw_lrc: String,
}

#[derive(Debug, Clone, Copy)]
pub enum TimedLyricsProvider<'a> {
    LrcLib(&'a LrcLibClient),
    LrcApi(&'a LrcApiClient),
}

impl TimedLyricsProvider<'_> {
    fn source(self) -> LyricsSource {
        match self {
            Self::LrcLib(_) => LyricsSource::LrcLib,
            Self::LrcApi(_) => LyricsSource::LrcApi,
        }
    }

    fn fetch_timed_lrc(self, query: &LyricsLookupQuery) -> Result<Option<String>> {
        match self {
            Self::LrcLib(client) => client
                .fetch_by_track(query)
                .map(|result| {
                    result.and_then(|lyrics| {
                        lyrics
                            .synced_lyrics
                            .filter(|lyrics| !lyrics.trim().is_empty())
                    })
                })
                .map_err(Into::into),
            Self::LrcApi(client) => client
                .fetch_by_track(query)
                .map(|result| {
                    result.and_then(|lyrics| {
                        // Prefer LRC, fall back to TTML
                        let lrc = lyrics.lrc.trim();
                        if !lrc.is_empty() {
                            Some(lyrics.lrc)
                        } else {
                            lyrics.lrc_ttml.filter(|ttml| !ttml.trim().is_empty())
                        }
                    })
                })
                .map_err(Into::into),
        }
    }
}

pub fn fetch_lyrics_for_song(
    providers: &[TimedLyricsProvider<'_>],
    song: &Song,
    resolved_audio_path: &Path,
) -> Result<Option<LyricsFetchResult>> {
    if let Some(query) = lookup_query_from_song(song) {
        if let Ok(Some(lyrics)) = fetch_online_timed_lyrics(providers, &query) {
            return Ok(Some(lyrics));
        }
    }

    if song.is_media_g_zip() {
        return Ok(None);
    }

    if let Some(embedded_lyrics) = read_embedded_lyrics(resolved_audio_path)? {
        return Ok(Some(LyricsFetchResult {
            source: LyricsSource::Embedded,
            raw_lrc: embedded_lyrics,
        }));
    }

    if let Some((sidecar_lyrics, sidecar_source)) = read_sidecar_lyrics(resolved_audio_path)? {
        return Ok(Some(LyricsFetchResult {
            source: sidecar_source,
            raw_lrc: sidecar_lyrics,
        }));
    }

    Ok(None)
}

pub fn lookup_query_from_song(song: &Song) -> Option<LyricsLookupQuery> {
    Some(LyricsLookupQuery {
        track_name: song.title.clone()?,
        artist_name: song.artist.clone()?,
        album_name: song.album.clone(),
        duration_seconds: Some((song.duration_ms / 1_000).max(0) as u64),
    })
}

pub fn fetch_online_timed_lyrics(
    providers: &[TimedLyricsProvider<'_>],
    query: &LyricsLookupQuery,
) -> Result<Option<LyricsFetchResult>> {
    let mut last_error: Option<anyhow::Error> = None;

    for provider in providers {
        match (*provider).fetch_timed_lrc(query) {
            Ok(Some(raw)) => {
                let trimmed = raw.trim();
                // Detect TTML content from LrcAPI
                let source = if (*provider).source() == LyricsSource::LrcApi
                    && (trimmed.starts_with("<?xml") || trimmed.starts_with("<tt"))
                {
                    LyricsSource::LrcApiTtml
                } else {
                    (*provider).source()
                };

                // Verify it has timed content
                let has_timed = if source == LyricsSource::LrcApiTtml {
                    ttml_parser::parse_ttml(&raw)
                        .map(|lines| !lines.is_empty())
                        .unwrap_or(false)
                } else {
                    has_timed_lines(&raw)
                };

                if has_timed {
                    return Ok(Some(LyricsFetchResult {
                        source,
                        raw_lrc: raw,
                    }));
                }
            }
            Ok(None) => {}
            Err(error) => {
                last_error = Some(error);
            }
        }
    }

    if let Some(error) = last_error {
        Err(error)
    } else {
        Ok(None)
    }
}

pub fn read_embedded_lyrics(path: &Path) -> Result<Option<String>> {
    let tagged_file = metadata::read_tagged_file_from_path(path).with_context(|| {
        format!(
            "failed to read embedded lyrics tags from {}",
            path.display()
        )
    })?;

    for tag in tagged_file.tags() {
        if let Some(lyrics) = tag.get_string(ItemKey::Lyrics) {
            let lyrics = lyrics.trim();
            if !lyrics.is_empty() {
                return Ok(Some(lyrics.to_owned()));
            }
        }
    }

    Ok(None)
}

fn read_sidecar_lyrics(path: &Path) -> Result<Option<(String, LyricsSource)>> {
    // Priority: .ttml > .lys > .lrc
    for (ext, source) in &[
        ("ttml", LyricsSource::SidecarTtml),
        ("lys", LyricsSource::SidecarLys),
        ("lrc", LyricsSource::Sidecar),
    ] {
        let sidecar_path = path.with_extension(ext);
        if sidecar_path.exists() {
            let contents = fs::read_to_string(&sidecar_path).with_context(|| {
                format!(
                    "failed to read sidecar lyrics from {}",
                    sidecar_path.display()
                )
            })?;
            let contents = contents.trim().to_owned();
            if !contents.is_empty() {
                return Ok(Some((contents, source.clone())));
            }
        }
    }
    Ok(None)
}

/// Detect format and parse lyrics automatically.
/// TTML if starts with "<?xml" or "<tt", LYS if matches "^\[\d\]", otherwise LRC.
pub fn parse_lyrics_auto(raw: &str) -> Result<Vec<crate::lyrics::parser::LyricLine>> {
    let trimmed = raw.trim();

    // TTML detection
    if trimmed.starts_with("<?xml") || trimmed.starts_with("<tt") {
        return ttml_parser::parse_ttml(raw).map_err(|e| anyhow::anyhow!("TTML parse error: {e}"));
    }

    // LYS detection: first non-empty line starts with [digit]
    if let Some(first_line) = trimmed.lines().find(|l| !l.trim().is_empty()) {
        if first_line.trim().starts_with('[')
            && first_line.trim().len() >= 2
            && first_line.trim().as_bytes()[1].is_ascii_digit()
        {
            if let Ok(lines) = lys_parser::parse_lys(raw) {
                if !lines.is_empty() {
                    return Ok(lines);
                }
            }
        }
    }

    // Default: LRC
    crate::lyrics::parser::parse_lrc(raw)
}

fn has_timed_lines(raw_lrc: &str) -> bool {
    parser::parse_lrc(raw_lrc)
        .map(|lines| !lines.is_empty())
        .unwrap_or(false)
}
