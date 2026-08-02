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
use std::{
    fs,
    path::{Path, PathBuf},
};

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
    /// Internal negative-cache marker — not exposed via IPC.
    Absent,
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

#[derive(Debug)]
pub enum OnlineLyricsResult {
    Found(LyricsFetchResult),
    DefiniteMissing,
    NotApplicable,
    Unavailable(anyhow::Error),
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

pub fn fetch_lyrics_for_song_local(
    song: &Song,
    resolved_audio_path: &Path,
) -> Result<Option<LyricsFetchResult>> {
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
        // A duration of 0 means "unknown" — sending duration=0 to LRCLIB
        // skews matching, so omit it and let artist/title/album drive the
        // lookup instead.
        duration_seconds: if song.duration_ms > 0 {
            Some((song.duration_ms / 1_000) as u64)
        } else {
            None
        },
    })
}

pub fn fetch_online_timed_lyrics(
    providers: &[TimedLyricsProvider<'_>],
    query: &LyricsLookupQuery,
) -> OnlineLyricsResult {
    let mut last_error: Option<anyhow::Error> = None;

    for provider in providers {
        match (*provider).fetch_timed_lrc(query) {
            Ok(Some(raw)) => {
                let trimmed = raw.trim();
                let source = if (*provider).source() == LyricsSource::LrcApi
                    && (trimmed.starts_with("<?xml") || trimmed.starts_with("<tt"))
                {
                    LyricsSource::LrcApiTtml
                } else {
                    (*provider).source()
                };

                let has_timed = if source == LyricsSource::LrcApiTtml {
                    ttml_parser::parse_ttml(&raw)
                        .map(|lines| !lines.is_empty())
                        .unwrap_or(false)
                } else {
                    has_timed_lines(&raw)
                };

                if has_timed {
                    return OnlineLyricsResult::Found(LyricsFetchResult {
                        source,
                        raw_lrc: raw,
                    });
                }
            }
            Ok(None) => {}
            Err(error) => {
                last_error = Some(error);
            }
        }
    }

    if let Some(error) = last_error {
        OnlineLyricsResult::Unavailable(error)
    } else {
        OnlineLyricsResult::DefiniteMissing
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
    // Validate each sidecar by attempting to parse; skip malformed files and
    // fall through to the next format so a valid lower-priority sidecar is used.
    //
    // Extension matching is case-insensitive to support Windows-originated
    // libraries on case-sensitive filesystems (Linux), where `song.LRC` and
    // `song.lrc` are distinct files.
    let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return Ok(None);
    };
    let Some(parent) = path.parent() else {
        return Ok(None);
    };

    // Collect matching sidecar paths by scanning the directory once.
    // Use PathBuf (not String) to preserve byte-exact paths on Linux where
    // paths may contain non-UTF-8 bytes.
    let mut candidates: Vec<(PathBuf, LyricsSource)> = Vec::new();
    let entries = fs::read_dir(parent)
        .with_context(|| format!("failed to read sidecar directory {}", parent.display()))?;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("failed to inspect sidecar directory {}", parent.display()))?;
        let entry_path = entry.path();
        let Some(stem) = entry_path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem != file_stem {
            continue;
        }
        let Some(ext) = entry_path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        match ext.to_ascii_lowercase().as_str() {
            "ttml" => candidates.push((entry_path, LyricsSource::SidecarTtml)),
            "lys" => candidates.push((entry_path, LyricsSource::SidecarLys)),
            "lrc" => candidates.push((entry_path, LyricsSource::Sidecar)),
            _ => {}
        }
    }

    fn priority(source: &LyricsSource) -> u8 {
        match source {
            LyricsSource::SidecarTtml => 0,
            LyricsSource::SidecarLys => 1,
            LyricsSource::Sidecar => 2,
            _ => 3,
        }
    }
    candidates.sort_by_key(|(_, source)| priority(source));

    for (sidecar_path, source) in &candidates {
        let contents = fs::read_to_string(sidecar_path).with_context(|| {
            format!(
                "failed to read sidecar lyrics from {}",
                sidecar_path.display()
            )
        })?;
        let contents = contents.trim().to_owned();
        if !contents.is_empty() && parse_lyrics_auto(&contents).is_ok_and(|l| !l.is_empty()) {
            return Ok(Some((contents, source.clone())));
        }
    }
    Ok(None)
}

pub fn parse_lyrics_auto(raw: &str) -> Result<Vec<crate::lyrics::parser::LyricLine>> {
    let trimmed = raw.trim();

    if trimmed.starts_with("<?xml") || trimmed.starts_with("<tt") {
        return ttml_parser::parse_ttml(raw).map_err(|e| anyhow::anyhow!("TTML parse error: {e}"));
    }

    if let Some(first_line) = trimmed.lines().find(|l| !l.trim().is_empty()) {
        let bytes = first_line.trim().as_bytes();
        if bytes.starts_with(b"[")
            && bytes.len() >= 3
            && bytes[1].is_ascii_digit()
            && bytes[2] == b']'
        {
            if let Ok(lines) = lys_parser::parse_lys(raw) {
                if !lines.is_empty() {
                    return Ok(lines);
                }
            }
        }
    }

    crate::lyrics::parser::parse_lrc(raw)
}

fn has_timed_lines(raw_lrc: &str) -> bool {
    parser::parse_lrc(raw_lrc)
        .map(|lines| !lines.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lyrics_auto_detects_ttml_xml_prefix() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml">
  <body><div><p begin="00:10.000" end="00:12.000">Hello</p></div></body>
</tt>"#;
        let lines = parse_lyrics_auto(ttml).expect("should parse TTML");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Hello");
    }

    #[test]
    fn parse_lyrics_auto_detects_ttml_tt_prefix() {
        let ttml = r#"<tt xmlns="http://www.w3.org/ns/ttml">
  <body><div><p begin="00:05.000" end="00:07.000">World</p></div></body>
</tt>"#;
        let lines = parse_lyrics_auto(ttml).expect("should parse TTML without xml decl");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "World");
    }

    #[test]
    fn parse_lyrics_auto_detects_lys_format() {
        let lys = "[0]Hello(1000,500) World(1500,500)\n";
        let lines = parse_lyrics_auto(lys).expect("should parse LYS");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Hello World");
    }

    #[test]
    fn parse_lyrics_auto_falls_back_to_lrc() {
        let lrc = "[00:10.00]Hello world\n";
        let lines = parse_lyrics_auto(lrc).expect("should parse LRC");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Hello world");
    }

    #[test]
    fn parse_lyrics_auto_empty_input_returns_empty() {
        let lines = parse_lyrics_auto("").expect("should not error");
        assert!(lines.is_empty());
    }

    #[test]
    fn parse_lyrics_auto_lys_with_background_vocals() {
        let lys = "[6]Background(3000,500)\n";
        let lines = parse_lyrics_auto(lys).expect("should parse LYS bg");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].bg_words.is_some());
    }

    #[test]
    fn parse_lyrics_auto_lrc_with_l_bracket_digit_not_confused_with_lys() {
        let lrc = "[00:10.00]Hello world\n";
        let lines = parse_lyrics_auto(lrc).expect("should fall back to LRC");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Hello world");
    }

    #[test]
    fn has_timed_lines_returns_true_for_valid_lrc() {
        assert!(has_timed_lines("[00:10.00]Hello\n"));
    }

    #[test]
    fn has_timed_lines_returns_false_for_empty() {
        assert!(!has_timed_lines(""));
    }

    #[test]
    fn has_timed_lines_returns_false_for_metadata_only() {
        assert!(!has_timed_lines("[ar:Artist]\n[ti:Title]\n"));
    }

    fn test_song(title: &str, artist: &str, duration_ms: i64) -> Song {
        Song {
            hash: "test-hash".to_owned(),
            file_path: Some("media/test.mp3".to_owned()),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: Some(title.to_owned()),
            artist: Some(artist.to_owned()),
            album: None,
            duration_ms,
            cover_art: None,
            has_cover_art: false,
            artwork_thumb_path: None,
            imported_at: 0,
            original_ext: None,
        }
    }

    #[test]
    fn lookup_query_omits_duration_when_unknown() {
        let song = test_song("Title", "Artist", 0);
        let query = lookup_query_from_song(&song).expect("query should exist");
        assert_eq!(query.duration_seconds, None);
    }

    #[test]
    fn lookup_query_includes_duration_when_known() {
        let song = test_song("Title", "Artist", 195_000);
        let query = lookup_query_from_song(&song).expect("query should exist");
        assert_eq!(query.duration_seconds, Some(195));
    }

    #[test]
    fn read_sidecar_lyrics_finds_uppercase_extension() {
        let dir = std::env::temp_dir().join(format!(
            "openkara_sidecar_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let audio_path = dir.join("song.mp3");
        std::fs::write(&audio_path, b"fake audio").unwrap();
        let lrc_path = dir.join("song.LRC");
        std::fs::write(&lrc_path, "[00:10.00]Hello world\n").unwrap();

        let result = read_sidecar_lyrics(&audio_path).expect("should not error");
        let (content, source) = result.expect("should find sidecar");
        assert_eq!(source, LyricsSource::Sidecar);
        assert!(content.contains("Hello world"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_sidecar_lyrics_priority_ttml_over_lrc() {
        let dir = std::env::temp_dir().join(format!(
            "openkara_sidecar_prio_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let audio_path = dir.join("song.mp3");
        std::fs::write(&audio_path, b"fake audio").unwrap();
        std::fs::write(dir.join("song.lrc"), "[00:10.00]LRC content\n").unwrap();
        std::fs::write(
            dir.join("song.ttml"),
            r#"<?xml version="1.0"?><tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:05.000" end="00:07.000">TTML content</p></div></body></tt>"#,
        )
        .unwrap();

        let result = read_sidecar_lyrics(&audio_path).expect("should not error");
        let (_, source) = result.expect("should find sidecar");
        assert_eq!(source, LyricsSource::SidecarTtml);

        std::fs::remove_dir_all(&dir).ok();
    }
}
