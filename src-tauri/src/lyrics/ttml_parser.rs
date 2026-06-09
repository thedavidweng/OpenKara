use anyhow::{bail, Result};
use quick_xml::events::Event;
use quick_xml::Reader;

use super::parser::{LyricLine, WordToken};

/// Parse a TTML XML string into lyric lines.
pub fn parse_ttml(ttml: &str) -> Result<Vec<LyricLine>> {
    let trimmed = ttml.trim();
    if !trimmed.contains('<') {
        bail!("not valid TTML XML: no XML tags found");
    }

    let mut reader = Reader::from_str(ttml);
    reader.config_mut().trim_text(true);

    let mut lines: Vec<LyricLine> = Vec::new();
    let mut current_section: Option<String> = None;
    let mut _in_body = false;
    let mut _in_div = false;
    let mut in_p = false;
    let mut in_bg_span = false;
    let mut in_translation_span = false;
    let mut in_roman_span = false;
    let mut line_timing_mode = false;

    // Current line state
    let mut p_begin: Option<u64> = None;
    let mut p_end: Option<u64> = None;
    let mut words: Vec<WordToken> = Vec::new();
    let mut bg_words: Vec<WordToken> = Vec::new();
    let mut text_buf = String::new();
    let mut current_span_begin: Option<u64> = None;
    let mut current_span_end: Option<u64> = None;

    // Stack to track nesting (for closing tags)
    let mut span_role_stack: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag_name = e.name();
                let tag_str = std::str::from_utf8(tag_name.as_ref()).unwrap_or("");

                match tag_str {
                    "body" => {
                        _in_body = true;
                    }
                    "div" => {
                        _in_div = true;
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            if key == "song-part" || key.ends_with(":song-part") {
                                current_section =
                                    Some(String::from_utf8_lossy(&attr.value).into_owned());
                            }
                            if (key == "timing" || key.ends_with(":timing"))
                                && String::from_utf8_lossy(&attr.value) == "Line"
                            {
                                line_timing_mode = true;
                            }
                        }
                    }
                    "p" => {
                        in_p = true;
                        p_begin = None;
                        p_end = None;
                        words.clear();
                        bg_words.clear();
                        text_buf.clear();

                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            let val = String::from_utf8_lossy(&attr.value);
                            if key == "begin" {
                                p_begin = parse_ttml_timestamp(&val);
                            }
                            if key == "end" {
                                p_end = parse_ttml_timestamp(&val);
                            }
                            if (key == "timing" || key.ends_with(":timing"))
                                && val.as_ref() == "Line"
                            {
                                line_timing_mode = true;
                            }
                        }
                    }
                    "span" => {
                        let mut role = String::new();
                        let mut begin_ms: Option<u64> = None;
                        let mut end_ms: Option<u64> = None;
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            let val = String::from_utf8_lossy(&attr.value);
                            if key == "role" || key.ends_with(":role") {
                                role = val.to_string();
                            }
                            if key == "begin" {
                                begin_ms = parse_ttml_timestamp(&val);
                            }
                            if key == "end" {
                                end_ms = parse_ttml_timestamp(&val);
                            }
                        }
                        span_role_stack.push(role.clone());

                        if role == "x-translation" {
                            in_translation_span = true;
                        } else if role == "x-roman" {
                            in_roman_span = true;
                        } else if role == "x-bg" {
                            in_bg_span = true;
                        } else if in_p && !line_timing_mode && begin_ms.is_some() {
                            current_span_begin = begin_ms;
                            current_span_end = end_ms;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                let text = e.decode().unwrap_or_default();
                if text.is_empty() || !in_p {
                    continue;
                }

                if in_translation_span || in_roman_span {
                    continue;
                }

                if in_bg_span {
                    if let Some(begin) = current_span_begin {
                        bg_words.push(WordToken {
                            time_ms: begin,
                            end_ms: current_span_end.unwrap_or(begin + 500),
                            text: text.to_string(),
                        });
                    }
                } else if !line_timing_mode {
                    text_buf.push_str(&text);
                    if let Some(begin) = current_span_begin {
                        words.push(WordToken {
                            time_ms: begin,
                            end_ms: current_span_end.unwrap_or(begin + 500),
                            text: text.to_string(),
                        });
                    }
                } else {
                    text_buf.push_str(&text);
                }
            }
            Ok(Event::End(e)) => {
                let tag_name = e.name();
                let tag_str = std::str::from_utf8(tag_name.as_ref()).unwrap_or("");

                match tag_str {
                    "p" => {
                        if in_p {
                            if let Some(begin) = p_begin {
                                let text = text_buf.trim().to_string();
                                if !text.is_empty() {
                                    // Fix up last word's end_ms from <p end="...">
                                    if let Some(p_end_val) = p_end {
                                        if let Some(last) = words.last_mut() {
                                            if last.end_ms == last.time_ms + 500 {
                                                last.end_ms = p_end_val;
                                            }
                                        }
                                    }
                                    lines.push(LyricLine {
                                        time_ms: begin,
                                        text,
                                        words: if words.is_empty() || line_timing_mode {
                                            None
                                        } else {
                                            Some(words.clone())
                                        },
                                        bg_words: if bg_words.is_empty() {
                                            None
                                        } else {
                                            Some(bg_words.clone())
                                        },
                                        section: current_section.clone(),
                                    });
                                }
                            }
                            in_p = false;
                            line_timing_mode = false;
                        }
                    }
                    "div" => {
                        _in_div = false;
                        current_section = None;
                        line_timing_mode = false;
                    }
                    "body" => {
                        _in_body = false;
                    }
                    "span" => {
                        if let Some(role) = span_role_stack.pop() {
                            if role == "x-translation" {
                                in_translation_span = false;
                            } else if role == "x-roman" {
                                in_roman_span = false;
                            } else if role == "x-bg" {
                                in_bg_span = false;
                            }
                        }
                        current_span_begin = None;
                        current_span_end = None;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("TTML parse error: {e}"),
            _ => {}
        }
    }

    lines.sort_by_key(|line| line.time_ms);
    Ok(lines)
}

/// Parse TTML timestamp formats into milliseconds.
/// Supports: MM:SS.fff, HH:MM:SS.fff, SS.fff, Ns (seconds with 's' suffix)
fn parse_ttml_timestamp(ts: &str) -> Option<u64> {
    let ts = ts.trim();

    // Handle "Ns" suffix (e.g., "1.5s" = 1500ms)
    if let Some(s) = ts.strip_suffix('s') {
        let secs: f64 = s.parse().ok()?;
        return Some((secs * 1000.0) as u64);
    }

    // Handle HH:MM:SS.fff or MM:SS.fff
    let parts: Vec<&str> = ts.split(':').collect();
    match parts.len() {
        3 => {
            // HH:MM:SS.fff
            let hours: u64 = parts[0].parse().ok()?;
            let minutes: u64 = parts[1].parse().ok()?;
            let (secs, ms) = parse_seconds_and_ms(parts[2])?;
            Some(hours * 3_600_000 + minutes * 60_000 + secs * 1_000 + ms)
        }
        2 => {
            // MM:SS.fff
            let minutes: u64 = parts[0].parse().ok()?;
            let (secs, ms) = parse_seconds_and_ms(parts[1])?;
            Some(minutes * 60_000 + secs * 1_000 + ms)
        }
        1 => {
            // SS.fff (bare seconds)
            let (secs, ms) = parse_seconds_and_ms(parts[0])?;
            Some(secs * 1_000 + ms)
        }
        _ => None,
    }
}

/// Parse "SS.fff" into (seconds, milliseconds)
fn parse_seconds_and_ms(s: &str) -> Option<(u64, u64)> {
    if let Some((sec_str, frac_str)) = s.split_once('.') {
        let secs: u64 = sec_str.parse().ok()?;
        let frac: u64 = frac_str.parse().ok()?;
        let ms = match frac_str.len() {
            1 => frac * 100,
            2 => frac * 10,
            3 => frac,
            _ => return None,
        };
        Some((secs, ms))
    } else {
        let secs: u64 = s.parse().ok()?;
        Some((secs, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_ttml_one_line() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:ttm="http://www.w3.org/ns/ttml#metadata"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000">Hello world</p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].time_ms, 10_000);
        assert_eq!(lines[0].text, "Hello world");
        assert!(lines[0].words.is_none());
        assert!(lines[0].bg_words.is_none());
        assert!(lines[0].section.is_none());
    }

    #[test]
    fn parse_ttml_with_word_timing() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:ttm="http://www.w3.org/ns/ttml#metadata"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div>
      <p begin="00:15.700" end="00:17.659" itunes:key="L1">
        <span begin="00:15.700" end="00:15.960">I</span>
        <span begin="00:15.960" end="00:16.324">want</span>
        <span begin="00:16.324" end="00:16.688">you</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.time_ms, 15_700);
        assert_eq!(line.text, "Iwantyou");
        let words = line.words.as_ref().expect("should have words");
        assert_eq!(words.len(), 3);
        assert_eq!(words[0].text, "I");
        assert_eq!(words[0].time_ms, 15_700);
        assert_eq!(words[1].text, "want");
        assert_eq!(words[1].time_ms, 15_960);
        assert_eq!(words[2].text, "you");
        assert_eq!(words[2].time_ms, 16_324);
    }

    #[test]
    fn parse_ttml_with_section() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div itunes:song-part="Chorus">
      <p begin="00:10.000" end="00:12.000">Sing along</p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].section.as_deref(), Some("Chorus"));
    }

    #[test]
    fn parse_ttml_discards_translations() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:ttm="http://www.w3.org/ns/ttml#metadata"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000">
        <span begin="00:10.000" end="00:11.000">Hello</span>
        <span begin="00:11.000" end="00:12.000">world</span>
        <span ttm:role="x-translation" xml:lang="zh-CN">你好世界</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 1);
        // Text should only contain the timed words, not the translation
        assert_eq!(lines[0].text, "Helloworld");
        let words = lines[0].words.as_ref().expect("should have words");
        assert_eq!(words.len(), 2);
    }

    #[test]
    fn parse_ttml_with_background_vocals() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:ttm="http://www.w3.org/ns/ttml#metadata"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000">
        <span begin="00:10.000" end="00:11.000">Main</span>
        <span ttm:role="x-bg">
          <span begin="00:10.500" end="00:11.500">Background</span>
        </span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].words.is_some());
        assert!(lines[0].bg_words.is_some());
        let bg = lines[0].bg_words.as_ref().unwrap();
        assert_eq!(bg.len(), 1);
        assert_eq!(bg[0].text, "Background");
    }

    #[test]
    fn parse_ttml_line_timing_mode() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div itunes:timing="Line">
      <p begin="00:10.000" end="00:12.000">No word timing here</p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].words.is_none());
        assert_eq!(lines[0].text, "No word timing here");
    }

    #[test]
    fn parse_ttml_hh_mm_ss_format() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div>
      <p begin="01:30.500" end="01:32.000">Long song</p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines[0].time_ms, 90_500);
    }

    #[test]
    fn parse_ttml_malformed_returns_error() {
        let result = parse_ttml("not xml at all");
        assert!(result.is_err());
    }

    #[test]
    fn parse_ttml_empty_body() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml">
  <body>
    <div></div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert!(lines.is_empty());
    }

    #[test]
    fn parse_ttml_word_end_time() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000">
        <span begin="00:10.000" end="00:10.500">Hello</span>
        <span begin="00:10.500" end="00:12.000">world</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        let words = lines[0].words.as_ref().unwrap();
        assert_eq!(words[0].end_ms, 10_500);
        assert_eq!(words[1].end_ms, 12_000);
    }
}
