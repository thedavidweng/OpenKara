use std::collections::HashMap;

use anyhow::{bail, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::parser::{LyricLine, WordToken};

fn local_tag_name(tag: &str) -> &str {
    tag.rsplit_once(':').map(|(_, local)| local).unwrap_or(tag)
}

fn is_pretty_print_space(text: &str) -> bool {
    text.trim().is_empty() && text.contains(['\n', '\r'])
}

fn lang_is_latn(lang: &str) -> bool {
    lang.split(['-', '_'])
        .any(|part| part.eq_ignore_ascii_case("Latn"))
}

fn nonempty_trimmed(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn append_word_roman(word: Option<&mut WordToken>, text: &str) {
    let Some(word) = word else {
        return;
    };
    let piece = text.trim();
    if piece.is_empty() {
        return;
    }
    match &mut word.roman {
        Some(existing) => {
            existing.push(' ');
            existing.push_str(piece);
        }
        None => word.roman = Some(piece.to_owned()),
    }
}

fn joined_word_romans(words: &[WordToken]) -> Option<String> {
    if words.is_empty() {
        return None;
    }
    let mut parts = Vec::with_capacity(words.len());
    for word in words {
        let roman = word.roman.as_deref()?.trim();
        if roman.is_empty() {
            return None;
        }
        parts.push(roman);
    }
    Some(parts.join(" "))
}

fn attr_key_matches(key: &str, local: &str) -> bool {
    key == local
        || key
            .strip_suffix(local)
            .is_some_and(|prefix| prefix.ends_with(':'))
}

#[derive(Default)]
struct TransliterationSidecar {
    in_translations: bool,
    in_transliterations: bool,
    in_track: bool,
    track_is_latn: bool,
    in_text: bool,
    text_for: String,
    text_buf: String,
    text_parts: Vec<String>,
    chosen: HashMap<String, SidecarRoman>,
}

struct SidecarRoman {
    is_latn: bool,
    line: String,
    parts: Vec<String>,
}

impl TransliterationSidecar {
    fn on_start(&mut self, local: &str, start: &BytesStart<'_>) {
        match local {
            "translations" => self.in_translations = true,
            "transliterations" | "transcriptions" => {
                if !self.in_translations {
                    self.in_transliterations = true;
                }
            }
            "transliteration" | "transcription" => {
                if self.in_translations {
                    return;
                }
                self.in_transliterations = true;
                self.in_track = true;
                self.track_is_latn = false;
                for attr in start.attributes().flatten() {
                    let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                    if attr_key_matches(key, "lang") {
                        self.track_is_latn = lang_is_latn(&String::from_utf8_lossy(&attr.value));
                    }
                }
            }
            "text" => {
                if self.in_translations || !self.in_transliterations || !self.in_track {
                    return;
                }
                self.in_text = true;
                self.text_for.clear();
                self.text_buf.clear();
                self.text_parts.clear();
                for attr in start.attributes().flatten() {
                    let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                    if attr_key_matches(key, "for") {
                        self.text_for = String::from_utf8_lossy(&attr.value).into_owned();
                    }
                }
            }
            _ => {}
        }
    }

    fn on_end(&mut self, local: &str) {
        match local {
            "translations" => self.in_translations = false,
            "transliterations" | "transcriptions" => {
                self.in_transliterations = false;
                self.in_track = false;
                self.in_text = false;
            }
            "transliteration" | "transcription" => {
                self.in_track = false;
                self.in_text = false;
            }
            "text" if self.in_text => {
                self.commit_text();
                self.in_text = false;
            }
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str, in_bg: bool) {
        if !self.in_text || in_bg || is_pretty_print_space(text) {
            return;
        }
        self.text_buf.push_str(text);
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            self.text_parts.push(trimmed.to_owned());
        }
    }

    fn commit_text(&mut self) {
        let key = self.text_for.trim();
        let text = self.text_buf.trim();
        if key.is_empty() || text.is_empty() {
            return;
        }
        match self.chosen.get(key) {
            Some(existing) if existing.is_latn => {}
            Some(existing) if !existing.is_latn && !self.track_is_latn => {}
            _ => {
                self.chosen.insert(
                    key.to_owned(),
                    SidecarRoman {
                        is_latn: self.track_is_latn,
                        line: text.to_owned(),
                        parts: self.text_parts.clone(),
                    },
                );
            }
        }
    }

    fn apply(self, lines: &mut [LyricLine], keys: &[Option<String>]) {
        for (line, key) in lines.iter_mut().zip(keys) {
            let Some(key) = key.as_deref() else {
                continue;
            };
            let Some(sidecar) = self.chosen.get(key) else {
                continue;
            };
            if let Some(words) = line.words.as_mut() {
                if sidecar.parts.len() == words.len() {
                    for (word, part) in words.iter_mut().zip(sidecar.parts.iter()) {
                        if word
                            .roman
                            .as_deref()
                            .map_or(true, |roman| roman.trim().is_empty())
                        {
                            word.roman = Some(part.clone());
                        }
                    }
                }
            }
            if line
                .roman
                .as_deref()
                .is_some_and(|roman| !roman.trim().is_empty())
            {
                continue;
            }
            line.roman = Some(sidecar.line.clone());
        }
    }
}

pub fn parse_ttml(ttml: &str) -> Result<Vec<LyricLine>> {
    let trimmed = ttml.trim();
    if !trimmed.contains('<') {
        bail!("not valid TTML XML: no XML tags found");
    }

    let mut reader = Reader::from_str(ttml);

    let mut lines: Vec<LyricLine> = Vec::new();
    let mut line_keys: Vec<Option<String>> = Vec::new();
    let mut current_section: Option<String> = None;
    let mut _in_body = false;
    let mut _in_div = false;
    let mut in_p = false;
    let mut in_bg_span = false;
    let mut in_translation_span = false;
    let mut in_roman_span = false;
    let mut ruby_text_depth: usize = 0;
    let mut div_line_timing_mode = false;
    let mut line_timing_mode = false;
    let mut div_context_stack: Vec<(Option<String>, bool)> = Vec::new();
    let mut sidecar = TransliterationSidecar::default();

    let mut p_begin: Option<u64> = None;
    let mut p_end: Option<u64> = None;
    let mut p_key: Option<String> = None;
    let mut words: Vec<WordToken> = Vec::new();
    let mut word_has_explicit_end: Vec<bool> = Vec::new();
    let mut bg_words: Vec<WordToken> = Vec::new();
    let mut text_buf = String::new();
    let mut roman_buf = String::new();
    let mut current_span_begin: Option<u64> = None;
    let mut current_span_end: Option<u64> = None;

    let mut span_role_stack: Vec<String> = Vec::new();
    let mut span_timing_stack: Vec<(Option<u64>, Option<u64>)> = Vec::new();
    let mut span_ruby_text_stack: Vec<bool> = Vec::new();

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
                        div_context_stack.push((current_section.clone(), div_line_timing_mode));
                        let mut next_section = current_section.clone();
                        let mut next_div_line_timing_mode = div_line_timing_mode;
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            if key == "song-part" || key.ends_with(":song-part") {
                                next_section =
                                    Some(String::from_utf8_lossy(&attr.value).into_owned());
                            }
                            if attr_key_matches(key, "timing") {
                                let val = String::from_utf8_lossy(&attr.value);
                                if val.as_ref() == "Line" {
                                    next_div_line_timing_mode = true;
                                } else if val.as_ref() == "Word" {
                                    next_div_line_timing_mode = false;
                                }
                            }
                        }
                        current_section = next_section;
                        div_line_timing_mode = next_div_line_timing_mode;
                        line_timing_mode = div_line_timing_mode;
                    }
                    "p" => {
                        in_p = true;
                        p_begin = None;
                        p_end = None;
                        p_key = None;
                        words.clear();
                        word_has_explicit_end.clear();
                        bg_words.clear();
                        text_buf.clear();
                        roman_buf.clear();
                        line_timing_mode = div_line_timing_mode;

                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            let val = String::from_utf8_lossy(&attr.value);
                            if key == "begin" {
                                p_begin = parse_ttml_timestamp(&val);
                            }
                            if key == "end" {
                                p_end = parse_ttml_timestamp(&val);
                            }
                            if attr_key_matches(key, "key") {
                                p_key = nonempty_trimmed(&val);
                            }
                            if attr_key_matches(key, "timing") {
                                if val.as_ref() == "Line" {
                                    line_timing_mode = true;
                                } else if val.as_ref() == "Word" {
                                    line_timing_mode = false;
                                }
                            }
                        }
                    }
                    "span" => {
                        let mut role = String::new();
                        let mut begin_ms: Option<u64> = None;
                        let mut end_ms: Option<u64> = None;
                        let mut is_ruby_text = false;
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
                            if attr_key_matches(key, "ruby")
                                && (val.as_ref() == "text" || val.as_ref() == "textContainer")
                            {
                                is_ruby_text = true;
                            }
                        }
                        span_role_stack.push(role.clone());
                        span_timing_stack.push((current_span_begin, current_span_end));
                        span_ruby_text_stack.push(is_ruby_text);
                        if is_ruby_text {
                            ruby_text_depth = ruby_text_depth.saturating_add(1);
                        }

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
                    _ => {
                        sidecar.on_start(local_tag_name(tag_str), &e);
                    }
                }
            }
            Ok(Event::Text(e)) => {
                let text = e.decode().unwrap_or_default();
                if text.is_empty() {
                    continue;
                }

                sidecar.push_text(&text, in_bg_span);

                if !in_p {
                    continue;
                }

                if in_translation_span || ruby_text_depth > 0 {
                    continue;
                }

                if in_roman_span {
                    if is_pretty_print_space(&text) {
                        continue;
                    }
                    let word_level = current_span_begin.is_some();
                    if in_bg_span {
                        append_word_roman(bg_words.last_mut(), &text);
                        continue;
                    }
                    if word_level {
                        append_word_roman(words.last_mut(), &text);
                    } else {
                        roman_buf.push_str(&text);
                    }
                    continue;
                }

                // TTML word spans carry significant spaces in their text nodes. Drop only
                // pretty-print indentation between tags; trimming all text corrupts line.text.
                if is_pretty_print_space(&text) {
                    continue;
                }

                if in_bg_span {
                    if let Some(begin) = current_span_begin {
                        bg_words.push(WordToken::new(
                            begin,
                            current_span_end.unwrap_or(begin + 500),
                            text.trim(),
                        ));
                    }
                } else if !line_timing_mode {
                    text_buf.push_str(&text);
                    if let Some(begin) = current_span_begin {
                        words.push(WordToken::new(
                            begin,
                            current_span_end.unwrap_or(begin + 500),
                            text.trim(),
                        ));
                        word_has_explicit_end.push(current_span_end.is_some());
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
                                    // Only words without an explicit span end inherit <p end>.
                                    if let Some(p_end_val) = p_end {
                                        if let Some(last_index) = words.len().checked_sub(1) {
                                            if !word_has_explicit_end[last_index] {
                                                let last = &mut words[last_index];
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
                                        roman: nonempty_trimmed(&roman_buf)
                                            .or_else(|| joined_word_romans(&words)),
                                    });
                                    line_keys.push(p_key.take());
                                }
                            }
                            in_p = false;
                            line_timing_mode = div_line_timing_mode;
                        }
                    }
                    "div" => {
                        if let Some((previous_section, previous_div_line_timing_mode)) =
                            div_context_stack.pop()
                        {
                            current_section = previous_section;
                            div_line_timing_mode = previous_div_line_timing_mode;
                        } else {
                            current_section = None;
                            div_line_timing_mode = false;
                        }
                        line_timing_mode = div_line_timing_mode;
                        _in_div = !div_context_stack.is_empty();
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
                        if let Some((previous_begin, previous_end)) = span_timing_stack.pop() {
                            current_span_begin = previous_begin;
                            current_span_end = previous_end;
                        } else {
                            current_span_begin = None;
                            current_span_end = None;
                        }
                        if span_ruby_text_stack.pop() == Some(true) {
                            ruby_text_depth = ruby_text_depth.saturating_sub(1);
                        }
                    }
                    _ => {
                        sidecar.on_end(local_tag_name(tag_str));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("TTML parse error: {e}"),
            _ => {}
        }
    }

    sidecar.apply(&mut lines, &line_keys);
    lines.sort_by_key(|line| line.time_ms);
    Ok(lines)
}

pub fn parse_ttml_declared_offset_ms(raw: &str) -> Option<i64> {
    let mut reader = Reader::from_str(raw);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                let tag_name = e.name();
                let tag_str = std::str::from_utf8(tag_name.as_ref()).unwrap_or("");
                let mut meta_key = None;
                let mut meta_value = None;
                for attr in e.attributes().flatten() {
                    let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                    let val = String::from_utf8_lossy(&attr.value);
                    if attr_key_matches(key, "timingOffset") {
                        if let Ok(offset) = val.trim().parse::<i64>() {
                            return Some(offset);
                        }
                    }
                    if attr_key_matches(key, "key") {
                        meta_key = Some(val.as_ref().to_owned());
                    }
                    if attr_key_matches(key, "value") {
                        meta_value = Some(val.as_ref().to_owned());
                    }
                }
                if local_tag_name(tag_str) == "meta" {
                    if let (Some(key), Some(value)) = (meta_key, meta_value) {
                        if key == "offset" || key == "offsetMs" {
                            if let Ok(offset) = value.trim().parse::<i64>() {
                                return Some(offset);
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => return None,
            Err(_) => return None,
            _ => {}
        }
    }
}

fn parse_ttml_timestamp(ts: &str) -> Option<u64> {
    let ts = ts.trim();

    // Handle "Ns" suffix (e.g., "1.5s" = 1500ms)
    if let Some(s) = ts.strip_suffix('s') {
        let secs: f64 = s.parse().ok()?;
        return Some((secs * 1000.0) as u64);
    }

    let parts: Vec<&str> = ts.split(':').collect();
    match parts.len() {
        3 => {
            let hours: u64 = parts[0].parse().ok()?;
            let minutes: u64 = parts[1].parse().ok()?;
            let (secs, ms) = parse_seconds_and_ms(parts[2])?;
            Some(hours * 3_600_000 + minutes * 60_000 + secs * 1_000 + ms)
        }
        2 => {
            let minutes: u64 = parts[0].parse().ok()?;
            let (secs, ms) = parse_seconds_and_ms(parts[1])?;
            Some(minutes * 60_000 + secs * 1_000 + ms)
        }
        1 => {
            let (secs, ms) = parse_seconds_and_ms(parts[0])?;
            Some(secs * 1_000 + ms)
        }
        _ => None,
    }
}

fn parse_seconds_and_ms(s: &str) -> Option<(u64, u64)> {
    if let Some((sec_str, frac_str)) = s.split_once('.') {
        let secs: u64 = sec_str.parse().ok()?;
        let frac: u64 = frac_str.parse().ok()?;
        let ms = match frac_str.len() {
            1 => frac * 100,
            2 => frac * 10,
            3 => frac,
            len => frac / 10_u64.pow((len - 3) as u32),
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
        assert!(lines[0].roman.is_none());
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
        <span begin="00:15.700" end="00:15.960">I </span>
        <span begin="00:15.960" end="00:16.324">want </span>
        <span begin="00:16.324" end="00:16.688">you</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.time_ms, 15_700);
        assert_eq!(line.text, "I want you");
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
        assert!(lines[0].roman.is_none());
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
        <span begin="00:10.000" end="00:11.000">Hello </span>
        <span begin="00:11.000" end="00:12.000">world</span>
        <span ttm:role="x-translation" xml:lang="zh-CN">你好世界</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Hello world");
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
    fn parse_ttml_div_line_timing_applies_to_every_p_in_div() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div itunes:timing="Line">
      <p begin="00:10.000" end="00:12.000">
        <span begin="00:10.000" end="00:11.000">First</span>
        <span begin="00:11.000" end="00:12.000"> line</span>
      </p>
      <p begin="00:13.000" end="00:15.000">
        <span begin="00:13.000" end="00:14.000">Second</span>
        <span begin="00:14.000" end="00:15.000"> line</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "First line");
        assert_eq!(lines[1].text, "Second line");
        assert!(lines[0].words.is_none());
        assert!(lines[1].words.is_none());
    }

    #[test]
    fn parse_ttml_nested_div_preserves_outer_line_timing_mode() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div itunes:timing="Line">
      <div>
        <p begin="00:10.000" end="00:12.000">
          <span begin="00:10.000" end="00:11.000">Nested</span>
          <span begin="00:11.000" end="00:12.000"> line</span>
        </p>
      </div>
      <p begin="00:13.000" end="00:15.000">
        <span begin="00:13.000" end="00:14.000">Outer</span>
        <span begin="00:14.000" end="00:15.000"> line</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Nested line");
        assert_eq!(lines[1].text, "Outer line");
        assert!(lines[0].words.is_none());
        assert!(lines[1].words.is_none());
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

    #[test]
    fn parse_ttml_preserves_explicit_last_word_end() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000">
        <span begin="00:10.000" end="00:10.500">Hello</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        let words = lines[0].words.as_ref().unwrap();
        assert_eq!(words[0].end_ms, 10_500);
    }

    #[test]
    fn parse_ttml_uses_p_end_for_last_word_without_explicit_end() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000">
        <span begin="00:10.000">Hello</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        let words = lines[0].words.as_ref().unwrap();
        assert_eq!(words[0].end_ms, 12_000);
    }

    #[test]
    fn parse_ttml_nested_span_preserves_outer_word_timing_after_child_closes() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:itunes="http://music.apple.com/lyrics-ttml">
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000">
        <span begin="00:10.000" end="00:11.000"><span>Hello</span> world</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        let words = lines[0].words.as_ref().expect("should keep word timing");
        assert_eq!(
            words,
            &vec![
                WordToken::new(10_000, 11_000, "Hello"),
                WordToken::new(10_000, 11_000, "world"),
            ]
        );
    }

    #[test]
    fn parse_seconds_and_ms_truncates_sub_millisecond_fraction() {
        assert_eq!(parse_seconds_and_ms("5.1234"), Some((5, 123)));
        assert_eq!(parse_seconds_and_ms("5.1239"), Some((5, 123)));
    }

    #[test]
    fn parse_ttml_extracts_inline_x_roman() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:ttm="http://www.w3.org/ns/ttml#metadata">
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000">
        <span begin="00:10.000" end="00:11.000">君の</span>
        <span begin="00:11.000" end="00:12.000">物語</span>
        <span ttm:role="x-roman" xml:lang="ja-Latn">kimi no monogatari</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "君の物語");
        assert_eq!(lines[0].roman.as_deref(), Some("kimi no monogatari"));
        let words = lines[0].words.as_ref().expect("should keep word timing");
        assert_eq!(words.len(), 2);
    }

    #[test]
    fn parse_ttml_sidecar_prefers_latn_and_loses_to_inline() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:ttm="http://www.w3.org/ns/ttml#metadata"
    xmlns:itunes="http://music.apple.com/lyric-ttml-internal">
  <head>
    <metadata>
      <iTunesMetadata>
        <translations>
          <translation xml:lang="en-Latn" type="subtitle">
            <text for="L1">The story you don't know</text>
            <text for="L2">A translated second line</text>
          </translation>
        </translations>
        <transliterations>
          <transliteration xml:lang="ja">
            <text for="L1">キミノ</text>
          </transliteration>
          <transliteration xml:lang="ja-Latn">
            <text for="L1">kimi no</text>
            <text for="L2"><span>shira</span><span>nai</span></text>
          </transliteration>
        </transliterations>
      </iTunesMetadata>
    </metadata>
  </head>
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000" itunes:key="L1">君の</p>
      <p begin="00:13.000" end="00:15.000" itunes:key="L2">知らない</p>
      <p begin="00:16.000" end="00:18.000" itunes:key="L3">
        物語
        <span ttm:role="x-roman">monogatari</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].roman.as_deref(), Some("kimi no"));
        assert_eq!(lines[1].roman.as_deref(), Some("shiranai"));
        assert_eq!(lines[2].roman.as_deref(), Some("monogatari"));
        assert_eq!(lines[0].text, "君の");
        assert_eq!(lines[1].text, "知らない");
        assert_eq!(lines[2].text, "物語");
    }

    #[test]
    fn parse_ttml_inline_roman_wins_over_sidecar() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:ttm="http://www.w3.org/ns/ttml#metadata"
    xmlns:itunes="http://music.apple.com/lyric-ttml-internal">
  <head>
    <metadata>
      <iTunesMetadata>
        <transliterations>
          <transliteration xml:lang="ja-Latn">
            <text for="L1">sidecar reading</text>
          </transliteration>
        </transliterations>
      </iTunesMetadata>
    </metadata>
  </head>
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000" itunes:key="L1">
        歌詞
        <span ttm:role="x-roman">inline reading</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines[0].roman.as_deref(), Some("inline reading"));
    }

    #[test]
    fn parse_ttml_ignores_translation_ruby_and_background_roman() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:ttm="http://www.w3.org/ns/ttml#metadata"
    xmlns:tts="http://www.w3.org/ns/ttml#styling">
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000">
        <span begin="00:10.000" end="00:11.000">
          <span tts:ruby="container">
            <span tts:ruby="base">漢</span>
            <span tts:ruby="text">かん</span>
          </span>
        </span>
        <span begin="00:11.000" end="00:12.000">字</span>
        <span ttm:role="x-translation" xml:lang="zh-CN">汉字</span>
        <span ttm:role="x-bg">
          <span begin="00:10.500" end="00:11.500">和</span>
          <span ttm:role="x-roman">wa</span>
        </span>
        <span ttm:role="x-roman">kanji</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "漢字");
        assert_eq!(lines[0].roman.as_deref(), Some("kanji"));
        let bg = lines[0].bg_words.as_ref().expect("should keep bg words");
        assert_eq!(bg[0].text, "和");
        assert_eq!(bg[0].roman.as_deref(), Some("wa"));
        assert!(lines[0]
            .words
            .as_ref()
            .is_some_and(|words| { words.iter().all(|word| word.roman.is_none()) }));
    }

    #[test]
    fn parse_ttml_attaches_inline_x_roman_to_the_timed_word() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:ttm="http://www.w3.org/ns/ttml#metadata">
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000">
        <span begin="00:10.000" end="00:11.000">君<span ttm:role="x-roman">kimi</span></span>
        <span begin="00:11.000" end="00:12.000">の<span ttm:role="x-roman">no</span></span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines[0].text, "君の");
        assert_eq!(lines[0].roman.as_deref(), Some("kimi no"));
        let words = lines[0].words.as_ref().expect("should keep word timing");
        assert_eq!(words[0].roman.as_deref(), Some("kimi"));
        assert_eq!(words[1].roman.as_deref(), Some("no"));
    }

    #[test]
    fn parse_ttml_sidecar_spans_fill_word_romans() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:ttm="http://www.w3.org/ns/ttml#metadata"
    xmlns:itunes="http://music.apple.com/lyric-ttml-internal">
  <head>
    <metadata>
      <iTunesMetadata>
        <transliterations>
          <transliteration xml:lang="ja-Latn">
            <text for="L1"><span>kimi</span><span>no</span></text>
          </transliteration>
        </transliterations>
      </iTunesMetadata>
    </metadata>
  </head>
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000" itunes:key="L1">
        <span begin="00:10.000" end="00:11.000">君</span>
        <span begin="00:11.000" end="00:12.000">の</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        let words = lines[0].words.as_ref().expect("should keep word timing");
        assert_eq!(words[0].roman.as_deref(), Some("kimi"));
        assert_eq!(words[1].roman.as_deref(), Some("no"));
        assert_eq!(lines[0].roman.as_deref(), Some("kimino"));
    }

    #[test]
    fn parse_ttml_nested_div_word_timing_keeps_child_word_spans() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:itunes="http://music.apple.com/lyric-ttml-internal">
  <body>
    <div itunes:timing="Line">
      <div itunes:timing="Word">
        <p begin="00:10.000" end="00:12.000">
          <span begin="00:10.000" end="00:11.000">Nested</span>
          <span begin="00:11.000" end="00:12.000"> word</span>
        </p>
      </div>
      <p begin="00:13.000" end="00:15.000">
        <span begin="00:13.000" end="00:14.000">Outer</span>
        <span begin="00:14.000" end="00:15.000"> line</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Nested word");
        let words = lines[0]
            .words
            .as_ref()
            .expect("Word child should keep spans");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "Nested");
        assert_eq!(words[1].text, "word");
        assert_eq!(lines[1].text, "Outer line");
        assert!(lines[1].words.is_none());
    }

    #[test]
    fn parse_ttml_word_timing_on_p_clears_only_that_line() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:itunes="http://music.apple.com/lyric-ttml-internal">
  <body>
    <div itunes:timing="Line">
      <p begin="00:10.000" end="00:12.000" itunes:timing="Word">
        <span begin="00:10.000" end="00:11.000">Kept</span>
        <span begin="00:11.000" end="00:12.000"> words</span>
      </p>
      <p begin="00:13.000" end="00:15.000">
        <span begin="00:13.000" end="00:14.000">Still</span>
        <span begin="00:14.000" end="00:15.000"> line</span>
      </p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines.len(), 2);
        assert!(lines[0]
            .words
            .as_ref()
            .is_some_and(|words| words.len() == 2));
        assert!(lines[1].words.is_none());
    }

    #[test]
    fn parse_ttml_sidecar_skips_background_descendants() {
        let ttml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml"
    xmlns:ttm="http://www.w3.org/ns/ttml#metadata"
    xmlns:itunes="http://music.apple.com/lyric-ttml-internal">
  <head>
    <metadata>
      <iTunesMetadata>
        <transliterations>
          <transcription xml:lang="ko-Latn">
            <text for="L1">
              <span begin="10.0s" end="10.8s">duryeopjineun ana</span>
              <span ttm:role="x-bg">
                <span begin="11.0s" end="11.8s">heungmiroul ppun</span>
              </span>
            </text>
          </transcription>
        </transliterations>
      </iTunesMetadata>
    </metadata>
  </head>
  <body>
    <div>
      <p begin="00:10.000" end="00:12.000" itunes:key="L1">두렵지는 않아</p>
    </div>
  </body>
</tt>"#;
        let lines = parse_ttml(ttml).expect("should parse");
        assert_eq!(lines[0].roman.as_deref(), Some("duryeopjineun ana"));
    }

    #[test]
    fn parse_ttml_declared_offset_ms_reads_timing_offset() {
        let ttml =
            r#"<tt itunes:timingOffset="150"><body><div><p begin="1s">Hi</p></div></body></tt>"#;
        assert_eq!(parse_ttml_declared_offset_ms(ttml), Some(150));
    }

    #[test]
    fn parse_ttml_declared_offset_ms_reads_amll_meta() {
        let ttml = r#"<tt><head><metadata><amll:meta key="offsetMs" value="-80"/></metadata></head><body><div><p begin="1s">Hi</p></div></body></tt>"#;
        assert_eq!(parse_ttml_declared_offset_ms(ttml), Some(-80));
    }

    #[test]
    fn parse_ttml_declared_offset_ms_absent() {
        let ttml = r#"<tt><body><div><p begin="1s">Hi</p></div></body></tt>"#;
        assert_eq!(parse_ttml_declared_offset_ms(ttml), None);
    }
}
