use anyhow::Result;
use regex::Regex;
use std::sync::LazyLock;

use super::parser::{LyricLine, WordToken};

static PROP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[(\d)\]").unwrap());
static WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(.*?)\((\d+),(\d+)\)").unwrap());

/// Parse a LYS (Lyricify Syllable) string into lyric lines.
pub fn parse_lys(lys: &str) -> Result<Vec<LyricLine>> {
    let mut lines = Vec::new();

    for raw_line in lys.lines() {
        let raw_line = raw_line.trim();
        if raw_line.is_empty() {
            continue;
        }

        let Some(caps) = PROP_RE.captures(raw_line) else {
            continue;
        };

        let prop: u8 = caps[1].parse().unwrap_or(0);
        let content = &raw_line[caps[0].len()..];

        let mut raw_tokens: Vec<(String, u64, u64)> = Vec::new();
        let mut search_start = 0;
        let content_bytes = content.as_bytes();
        while let Some(caps) = WORD_RE.captures_at(content, search_start) {
            let m = caps.get(0).unwrap();
            let mut text = caps[1].to_string();
            let start_ms: u64 = caps[2].parse().unwrap_or(0);
            let duration_ms: u64 = caps[3].parse().unwrap_or(0);
            search_start = m.end();

            // Check for trailing ) right after the match (outer bg parentheses)
            if search_start < content_bytes.len() && content_bytes[search_start] == b')' {
                text.push(')');
                search_start += 1;
            }

            raw_tokens.push((text, start_ms, duration_ms));
        }

        if raw_tokens.is_empty() {
            continue;
        }

        let time_ms = raw_tokens.iter().map(|(_, t, _)| *t).min().unwrap_or(0);

        // Determine if background vocal (using raw text to detect parens)
        let is_bg = prop >= 6
            || (prop == 0
                && raw_tokens
                    .first()
                    .is_some_and(|(t, _, _)| t.starts_with('('))
                && raw_tokens.last().is_some_and(|(t, _, _)| t.ends_with(')')));

        let (words, bg_words, display_text) = if is_bg {
            // Strip parentheses from bg tokens
            let cleaned: Vec<WordToken> = raw_tokens
                .iter()
                .map(|(txt, start_ms, duration_ms)| {
                    let mut t = txt.trim().to_string();
                    if t.starts_with('(') {
                        t.remove(0);
                    }
                    if t.ends_with(')') {
                        t.pop();
                    }
                    WordToken {
                        time_ms: *start_ms,
                        end_ms: start_ms + duration_ms,
                        text: t.trim().to_string(),
                    }
                })
                .collect();
            let bg_text: String = cleaned.iter().map(|t| t.text.as_str()).collect();
            (None, Some(cleaned), bg_text)
        } else {
            let tokens: Vec<WordToken> = raw_tokens
                .iter()
                .map(|(txt, start_ms, duration_ms)| WordToken {
                    time_ms: *start_ms,
                    end_ms: start_ms + duration_ms,
                    text: txt.trim().to_string(),
                })
                .collect();
            // Reconstruct display text from trimmed token texts
            let display = tokens
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            (Some(tokens), None, display)
        };

        lines.push(LyricLine {
            time_ms,
            text: display_text,
            words,
            bg_words,
            section: None,
        });
    }

    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_lys() {
        let lys = "[0]Hello(1000,500) World(1500,500)\n";
        let lines = parse_lys(lys).expect("should parse");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].time_ms, 1000);
        assert_eq!(lines[0].text, "Hello World");
        let words = lines[0].words.as_ref().expect("should have words");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "Hello");
        assert_eq!(words[0].time_ms, 1000);
        assert_eq!(words[1].text, "World");
        assert_eq!(words[1].time_ms, 1500);
        assert!(lines[0].bg_words.is_none());
    }

    #[test]
    fn parse_lys_multiple_lines() {
        let lys = "[0]First(1000,500)\n[0]Second(2000,500)\n";
        let lines = parse_lys(lys).expect("should parse");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "First");
        assert_eq!(lines[1].text, "Second");
    }

    #[test]
    fn parse_lys_background_vocals_prop_6() {
        let lys = "[6]Background(3000,500)\n";
        let lines = parse_lys(lys).expect("should parse");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].words.is_none());
        let bg = lines[0].bg_words.as_ref().expect("should have bg_words");
        assert_eq!(bg.len(), 1);
        assert_eq!(bg[0].text, "Background");
    }

    #[test]
    fn parse_lys_background_vocals_prop_7() {
        let lys = "[7]Backup(3000,500)\n";
        let lines = parse_lys(lys).expect("should parse");
        assert!(lines[0].bg_words.is_some());
        assert!(lines[0].words.is_none());
    }

    #[test]
    fn parse_lys_background_vocals_prop_8() {
        let lys = "[8]Backup(3000,500)\n";
        let lines = parse_lys(lys).expect("should parse");
        assert!(lines[0].bg_words.is_some());
    }

    #[test]
    fn parse_lys_background_detection_via_parens() {
        // prop=0, but first word starts with ( and last ends with )
        let lys = "[0](Again(3000,500))\n";
        let lines = parse_lys(lys).expect("should parse");
        assert_eq!(lines.len(), 1);
        let bg = lines[0].bg_words.as_ref().expect("should have bg_words");
        assert_eq!(bg[0].text, "Again");
    }

    #[test]
    fn parse_lys_empty_input() {
        let lines = parse_lys("").expect("should parse");
        assert!(lines.is_empty());
    }

    #[test]
    fn parse_lys_skip_invalid_lines() {
        let lys = "no prefix here\n[0]Valid(1000,500)\n";
        let lines = parse_lys(lys).expect("should parse");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Valid");
    }

    #[test]
    fn parse_lys_duration_calculation() {
        // Word with start=1000, duration=500 should have end=1500
        let lys = "[0]Word(1000,500)\n";
        let lines = parse_lys(lys).expect("should parse");
        let words = lines[0].words.as_ref().unwrap();
        assert_eq!(words[0].time_ms, 1000);
        assert_eq!(words[0].end_ms, 1500);
        // text should be "Word"
        assert_eq!(words[0].text, "Word");
    }

    #[test]
    fn parse_lys_word_end_time() {
        let lys = "[0]Hello(1000,500) World(1500,750)\n";
        let lines = parse_lys(lys).expect("should parse");
        let words = lines[0].words.as_ref().unwrap();
        assert_eq!(words[0].end_ms, 1500);
        assert_eq!(words[1].end_ms, 2250);
    }
}
