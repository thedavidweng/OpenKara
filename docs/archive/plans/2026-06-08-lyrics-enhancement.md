# Lyrics Enhancement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add TTML and .lys format parsers, extend lyric types, expand the fetch chain, and improve lyrics visual rendering.

**Architecture:** New Rust parser modules (`ttml_parser.rs`, `lys_parser.rs`) produce the same `Vec<LyricLine>` output as the existing LRC parser. A new `parse_lyrics_auto` dispatcher detects format and routes to the correct parser. The fetch chain is expanded to try TTML from LrcAPI and sidecar `.ttml`/`.lys` files. Visual improvements use CSS `mask-image`, `filter: blur()`, and `mix-blend-mode: plus-lighter`.

**Tech Stack:** Rust (quick-xml, regex), React/TypeScript, CSS (mask-image, filter, mix-blend-mode), Web Animations API

---

## File Map

### Rust Backend (new files)

- `src-tauri/src/lyrics/ttml_parser.rs` — TTML XML parser
- `src-tauri/src/lyrics/lys_parser.rs` — LYS format parser

### Rust Backend (modified files)

- `src-tauri/Cargo.toml` — add `quick-xml` and `regex` dependencies
- `src-tauri/src/lyrics/mod.rs` — register new modules
- `src-tauri/src/lyrics/parser.rs` — extend `LyricLine` with `bg_words`, `section`
- `src-tauri/src/lyrics/fetch.rs` — add new `LyricsSource` variants, sidecar expansion, format dispatch
- `src-tauri/src/lyrics/lrcapi.rs` — no changes needed (struct already has `lrc_ttml`)
- `src-tauri/src/commands/lyrics.rs` — use `parse_lyrics_auto` instead of `parse_lrc`, update `save_manual_lyrics` format detection

### TypeScript Frontend (modified files)

- `src/types/ipc.ts` — extend `LyricLine` and `LyricsSource` types
- `src/components/Lyrics/LyricLine.tsx` — karaoke fill effect, render `bg_words`
- `src/components/Lyrics/LyricsPanel.tsx` — line transitions, glow effects, pass `bgWords` to LyricLine
- `src/components/Lyrics/LyricsEditDialog.tsx` — multi-format detection indicator
- `src/stores/lyrics-store.ts` — no changes needed (types flow through)
- `src/locales/en.json` — add new format detection strings
- `src/locales/zh-CN.json` — add new format detection strings

### CSS

- `src/index.css` or Tailwind config — typography, glow, transition utilities

### Docs

- `README.md` — acknowledgments

---

## Task 1: Extend LyricLine Types (Rust)

**Files:**

- Modify: `src-tauri/src/lyrics/parser.rs:4-15`
- Modify: `src-tauri/src/lyrics/fetch.rs:15-23`

- [ ] **Step 1: Add `bg_words` and `section` to Rust `LyricLine`**

In `src-tauri/src/lyrics/parser.rs`, change the `LyricLine` struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
    pub words: Option<Vec<WordToken>>,
    pub bg_words: Option<Vec<WordToken>>,
    pub section: Option<String>,
}
```

- [ ] **Step 2: Add new `LyricsSource` variants**

In `src-tauri/src/lyrics/fetch.rs`, change the enum:

```rust
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
```

- [ ] **Step 3: Fix all existing `LyricLine` construction sites**

Every place that creates a `LyricLine` must add the two new fields. Search for `LyricLine {` in the codebase and add `bg_words: None, section: None` to each.

In `src-tauri/src/lyrics/parser.rs` — the `parse_lrc` function (line ~85):

```rust
parsed_lines.push(LyricLine {
    time_ms: timestamp_ms,
    text: lyric_text.clone(),
    words: words.clone(),
    bg_words: None,
    section: None,
});
```

In `src-tauri/src/commands/lyrics.rs` — the `plain_text_to_lines` function (line ~493):

```rust
fn plain_text_to_lines(text: &str) -> Vec<LyricLine> {
    text.lines()
        .map(|l| LyricLine {
            time_ms: 0,
            text: l.to_string(),
            words: None,
            bg_words: None,
            section: None,
        })
        .collect()
}
```

- [ ] **Step 4: Run existing tests to verify no regressions**

Run: `cd src-tauri && cargo test`
Expected: All existing parser tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lyrics/parser.rs src-tauri/src/lyrics/fetch.rs src-tauri/src/commands/lyrics.rs
git commit -m "feat: extend LyricLine with bg_words and section fields"
```

---

## Task 2: Extend LyricLine Types (TypeScript)

**Files:**

- Modify: `src/types/ipc.ts:431-455`

- [ ] **Step 1: Update TypeScript types**

In `src/types/ipc.ts`, update the types:

```typescript
export type LyricsSource =
  | "lrc_lib"
  | "lrc_api"
  | "lrc_api_ttml"
  | "embedded"
  | "sidecar"
  | "sidecar_ttml"
  | "sidecar_lys"
  | "manual"
  | "manual_ttml"
  | "manual_lys";

export interface WordToken {
  time_ms: number;
  text: string;
}

export interface LyricLine {
  time_ms: number;
  text: string;
  words: WordToken[] | null;
  bg_words: WordToken[] | null;
  section: string | null;
}
```

- [ ] **Step 2: Verify TypeScript compiles**

Run: `npm run typecheck` (or equivalent)
Expected: No type errors. The new fields are optional-compatible since Rust serializes `None` as `null`.

- [ ] **Step 3: Commit**

```bash
git add src/types/ipc.ts
git commit -m "feat: extend TypeScript LyricLine and LyricsSource types"
```

---

## Task 3: TTML Parser

**Files:**

- Modify: `src-tauri/Cargo.toml` — add `quick-xml` dependency
- Create: `src-tauri/src/lyrics/ttml_parser.rs`
- Modify: `src-tauri/src/lyrics/mod.rs` — add `pub mod ttml_parser;`

- [ ] **Step 1: Add `quick-xml` dependency**

In `src-tauri/Cargo.toml`, add to `[dependencies]`:

```toml
quick-xml = "0.37"
```

- [ ] **Step 2: Write failing tests for TTML parser**

Create `src-tauri/src/lyrics/ttml_parser.rs` with tests first:

```rust
use anyhow::Result;

use super::parser::{LyricLine, WordToken};

/// Parse a TTML XML string into lyric lines.
pub fn parse_ttml(ttml: &str) -> Result<Vec<LyricLine>> {
    todo!("implement TTML parser")
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
}
```

- [ ] **Step 3: Register the module**

In `src-tauri/src/lyrics/mod.rs`, add:

```rust
pub mod ttml_parser;
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cd src-tauri && cargo test ttml_parser`
Expected: Tests fail with `todo!()` panic.

- [ ] **Step 5: Implement the TTML parser**

Replace the `parse_ttml` function in `src-tauri/src/lyrics/ttml_parser.rs`:

```rust
use anyhow::{bail, Context, Result};
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader;

use super::parser::{LyricLine, WordToken};

/// Parse a TTML XML string into lyric lines.
pub fn parse_ttml(ttml: &str) -> Result<Vec<LyricLine>> {
    let mut reader = Reader::from_str(ttml);
    reader.trim_text(true);

    let mut lines: Vec<LyricLine> = Vec::new();
    let mut current_section: Option<String> = None;
    let mut in_body = false;
    let mut in_div = false;
    let mut in_p = false;
    let mut in_bg_span = false;
    let mut in_translation_span = false;
    let mut in_roman_span = false;
    let mut line_timing_mode = false;

    // Current line state
    let mut p_begin: Option<u64> = None;
    let mut words: Vec<WordToken> = Vec::new();
    let mut bg_words: Vec<WordToken> = Vec::new();
    let mut text_buf = String::new();
    let mut current_span_begin: Option<u64> = None;

    // Stack to track nesting (for closing tags)
    let mut span_role_stack: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag_name = e.name();
                let tag_str = std::str::from_utf8(tag_name.as_ref()).unwrap_or("");

                match tag_str {
                    "body" => {
                        in_body = true;
                    }
                    "div" => {
                        in_div = true;
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            if key == "song-part" || key.ends_with(":song-part") {
                                current_section =
                                    Some(String::from_utf8_lossy(&attr.value).into_owned());
                            }
                            if key == "timing" || key.ends_with(":timing") {
                                if String::from_utf8_lossy(&attr.value) == "Line" {
                                    line_timing_mode = true;
                                }
                            }
                        }
                    }
                    "p" => {
                        in_p = true;
                        p_begin = None;
                        words.clear();
                        bg_words.clear();
                        text_buf.clear();

                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            let val = String::from_utf8_lossy(&attr.value);
                            if key == "begin" {
                                p_begin = parse_ttml_timestamp(&val);
                            }
                            if key == "timing" || key.ends_with(":timing") {
                                if val.as_ref() == "Line" {
                                    line_timing_mode = true;
                                }
                            }
                        }
                    }
                    "span" => {
                        let mut role = String::new();
                        let mut begin_ms: Option<u64> = None;
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            let val = String::from_utf8_lossy(&attr.value);
                            if key == "role" || key.ends_with(":role") {
                                role = val.to_string();
                            }
                            if key == "begin" {
                                begin_ms = parse_ttml_timestamp(&val);
                            }
                        }
                        span_role_stack.push(role.clone());

                        if role == "x-translation" || role == "x-roman" {
                            in_translation_span = true;
                        } else if role == "x-bg" {
                            in_bg_span = true;
                        } else if in_p && !line_timing_mode && begin_ms.is_some() {
                            current_span_begin = begin_ms;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                let text = e.unescape().unwrap_or_default();
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
                            text: text.to_string(),
                        });
                    }
                } else if !line_timing_mode {
                    text_buf.push_str(&text);
                    if let Some(begin) = current_span_begin {
                        words.push(WordToken {
                            time_ms: begin,
                            text: text.to_string(),
                        });
                    }
                } else {
                    text_buf.push_str(&text);
                }
            }
            Ok(Event::End(e)) => {
                let tag_str =
                    std::str::from_utf8(e.name().as_ref()).unwrap_or("");

                match tag_str {
                    "p" => {
                        if in_p {
                            if let Some(begin) = p_begin {
                                let text = text_buf.trim().to_string();
                                if !text.is_empty() {
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
                        in_div = false;
                        current_section = None;
                        line_timing_mode = false;
                    }
                    "body" => {
                        in_body = false;
                    }
                    "span" => {
                        if let Some(role) = span_role_stack.pop() {
                            if role == "x-translation" || role == "x-roman" {
                                in_translation_span = false;
                            } else if role == "x-bg" {
                                in_bg_span = false;
                            }
                        }
                        current_span_begin = None;
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
```

- [ ] **Step 6: Run TTML parser tests**

Run: `cd src-tauri && cargo test ttml_parser`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/lyrics/ttml_parser.rs src-tauri/src/lyrics/mod.rs
git commit -m "feat: add TTML parser with word-level timing support"
```

---

## Task 4: LYS Parser

**Files:**

- Create: `src-tauri/src/lyrics/lys_parser.rs`
- Modify: `src-tauri/src/lyrics/mod.rs` — add `pub mod lys_parser;`

- [ ] **Step 1: Add `regex` dependency**

In `src-tauri/Cargo.toml`, add to `[dependencies]`:

```toml
regex = "1"
```

- [ ] **Step 2: Write failing tests for LYS parser**

Create `src-tauri/src/lyrics/lys_parser.rs`:

```rust
use anyhow::Result;
use regex::Regex;

use super::parser::{LyricLine, WordToken};

/// Parse a LYS (Lyricify Syllable) string into lyric lines.
pub fn parse_lys(lys: &str) -> Result<Vec<LyricLine>> {
    todo!("implement LYS parser")
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
        // text should be "Word"
        assert_eq!(words[0].text, "Word");
    }
}
```

- [ ] **Step 3: Register the module**

In `src-tauri/src/lyrics/mod.rs`, add:

```rust
pub mod lys_parser;
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cd src-tauri && cargo test lys_parser`
Expected: Tests fail with `todo!()` panic.

- [ ] **Step 5: Implement the LYS parser**

Replace the `parse_lys` function in `src-tauri/src/lyrics/lys_parser.rs`:

```rust
use anyhow::Result;
use regex::Regex;
use std::sync::LazyLock;

use super::parser::{LyricLine, WordToken};

static PROP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[(\d)\]").unwrap());
static WORD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(.*?)\((\d+),(\d+)\)").unwrap());

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

        let mut tokens: Vec<WordToken> = Vec::new();
        for word_caps in WORD_RE.captures_iter(content) {
            let text = word_caps[1].to_string();
            let start_ms: u64 = word_caps[2].parse().unwrap_or(0);
            let duration_ms: u64 = word_caps[3].parse().unwrap_or(0);

            tokens.push(WordToken {
                time_ms: start_ms,
                text,
            });
        }

        if tokens.is_empty() {
            continue;
        }

        let time_ms = tokens.iter().map(|t| t.time_ms).min().unwrap_or(0);
        let text: String = tokens.iter().map(|t| t.text.as_str()).collect();

        // Determine if background vocal
        let is_bg = prop >= 6
            || (prop == 0
                && tokens.first().map_or(false, |t| t.text.starts_with('('))
                && tokens.last().map_or(false, |t| t.text.ends_with(')')));

        let (words, bg_words, display_text) = if is_bg {
            // Strip parentheses from bg tokens
            let cleaned: Vec<WordToken> = tokens
                .iter()
                .map(|t| {
                    let mut txt = t.text.clone();
                    if txt.starts_with('(') {
                        txt.remove(0);
                    }
                    if txt.ends_with(')') {
                        txt.pop();
                    }
                    WordToken {
                        time_ms: t.time_ms,
                        text: txt,
                    }
                })
                .collect();
            let bg_text: String = cleaned.iter().map(|t| t.text.as_str()).collect();
            (None, Some(cleaned), bg_text)
        } else {
            (Some(tokens), None, text)
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
```

- [ ] **Step 6: Run LYS parser tests**

Run: `cd src-tauri && cargo test lys_parser`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/lyrics/lys_parser.rs src-tauri/src/lyrics/mod.rs
git commit -m "feat: add LYS (Lyricify Syllable) parser"
```

---

## Task 5: Format Auto-Detection and Fetch Chain Updates

**Files:**

- Modify: `src-tauri/src/lyrics/fetch.rs`
- Modify: `src-tauri/src/commands/lyrics.rs`

- [ ] **Step 1: Add `parse_lyrics_auto` to fetch.rs**

In `src-tauri/src/lyrics/fetch.rs`, add:

```rust
use crate::lyrics::{ttml_parser, lys_parser};

/// Detect format and parse lyrics automatically.
/// TTML if starts with "<?xml" or "<tt", LYS if matches "^\[\d\]", otherwise LRC.
pub fn parse_lyrics_auto(raw: &str) -> Result<Vec<crate::lyrics::parser::LyricLine>> {
    let trimmed = raw.trim();

    // TTML detection
    if trimmed.starts_with("<?xml") || trimmed.starts_with("<tt") {
        return ttml_parser::parse_ttml(raw)
            .map_err(|e| anyhow::anyhow!("TTML parse error: {e}"));
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
```

- [ ] **Step 2: Add sidecar .ttml/.lys support**

In `src-tauri/src/lyrics/fetch.rs`, replace `read_sidecar_lrc` with:

```rust
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
```

- [ ] **Step 3: Update `fetch_lyrics_for_song` to use new sidecar function**

In `src-tauri/src/lyrics/fetch.rs`, update the sidecar section:

```rust
    if let Some((sidecar_lyrics, sidecar_source)) = read_sidecar_lyrics(resolved_audio_path)? {
        return Ok(Some(LyricsFetchResult {
            source: sidecar_source,
            raw_lrc: sidecar_lyrics,
        }));
    }
```

- [ ] **Step 4: Update LrcAPI to try TTML field**

In `src-tauri/src/lyrics/fetch.rs`, update the `LrcApi` variant's `fetch_timed_lrc`:

```rust
            Self::LrcApi(client) => client
                .fetch_by_track(query)
                .map(|result| {
                    result.and_then(|lyrics| {
                        // Prefer LRC, fall back to TTML
                        let lrc = lyrics.lrc.trim();
                        if !lrc.is_empty() {
                            Some(lyrics.lrc)
                        } else if let Some(ttml) = lyrics.lrc_ttml {
                            if !ttml.trim().is_empty() {
                                Some(ttml)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                })
                .map_err(Into::into),
```

Also update `TimedLyricsProvider::source` for LrcApi to detect TTML:

```rust
    fn source(self) -> LyricsSource {
        match self {
            Self::LrcLib(_) => LyricsSource::LrcLib,
            Self::LrcApi(_) => LyricsSource::LrcApi,
        }
    }
```

And update `fetch_online_timed_lyrics` to detect TTML content from LrcAPI:

```rust
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
```

- [ ] **Step 5: Update `commands/lyrics.rs` to use `parse_lyrics_auto`**

In `src-tauri/src/commands/lyrics.rs`, replace all calls to `lyrics::parser::parse_lrc` with `lyrics::fetch::parse_lyrics_auto`. There are 5 call sites — each keeps the existing `.map_err()` pattern:

1. `fetch_lyrics_from_connection` (line ~120):
   `let lines = lyrics::fetch::parse_lyrics_auto(&fetched.raw_lrc).map_err(|e| LyricsError::LyricsNotReady(e.to_string()))?;`

2. `payload_from_cached_entry` (line ~171):
   `let mut lines = lyrics::fetch::parse_lyrics_auto(&cached.lrc).map_err(|e| LyricsError::LyricsNotReady(e.to_string()))?;`

3. `save_manual_lyrics` (line ~200):
   `let lines = match lyrics::fetch::parse_lyrics_auto(&text) { Ok(parsed) if !parsed.is_empty() => parsed, _ => plain_text_to_lines(&text), };`

4. `fetch_lyrics_online` (line ~460):
   `let lines = lyrics::fetch::parse_lyrics_auto(&fetched.raw_lrc).map_err(|e| LyricsError::LyricsNotReady(e.to_string()))?;`

5. `extract_embedded_lyrics` (line ~380):
   `let lines = match lyrics::fetch::parse_lyrics_auto(&embedded) { Ok(parsed) if !parsed.is_empty() => parsed, _ => plain_text_to_lines(&embedded), };`

Also update `save_manual_lyrics` to detect format and use the correct source variant:

```rust
        let source = {
            let trimmed = text.trim();
            if trimmed.starts_with("<?xml") || trimmed.starts_with("<tt") {
                LyricsSource::ManualTtml
            } else if trimmed.lines().find(|l| !l.trim().is_empty()).map_or(false, |l| {
                l.trim().starts_with('[')
                    && l.trim().len() >= 2
                    && l.trim().as_bytes()[1].is_ascii_digit()
            }) {
                LyricsSource::ManualLys
            } else {
                LyricsSource::Manual
            }
        };
```

- [ ] **Step 6: Run all tests**

Run: `cd src-tauri && cargo test`
Expected: All tests pass including new TTML and LYS parser tests.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/lyrics/fetch.rs src-tauri/src/commands/lyrics.rs
git commit -m "feat: add format auto-detection, LrcAPI TTML, sidecar .ttml/.lys"
```

---

## Task 6: Frontend Manual Entry Format Detection

**Files:**

- Modify: `src/components/Lyrics/LyricsEditDialog.tsx`
- Modify: `src/locales/en.json`
- Modify: `src/locales/zh-CN.json`

- [ ] **Step 1: Update format detection in edit dialog**

In `src/components/Lyrics/LyricsEditDialog.tsx`, update the format detection (line ~43):

```typescript
const isTtml = text.trim().startsWith("<?xml") || text.trim().startsWith("<tt");
const isLys = /^\[\d]/.test(
  text
    .trim()
    .split("\n")
    .find((l) => l.trim().length > 0) ?? "",
);
const isLrc = /\[\d{2}:\d{2}/.test(text);
```

And update the format indicator text (line ~94):

```typescript
        <p className="text-[11px] text-[var(--color-text-dim)]">
          {text.trim().length > 0
            ? isTtml
              ? t("lyrics.detectedTtml")
              : isLys
                ? t("lyrics.detectedLys")
                : isLrc
                  ? t("lyrics.detectedLrc")
                  : t("lyrics.detectedPlain")
            : t("lyrics.supportsFormats")}
        </p>
```

- [ ] **Step 2: Add locale strings**

In `src/locales/en.json`, add under the `lyrics` section:

```json
"detectedTtml": "TTML format detected",
"detectedLys": "Lyricify Syllable format detected",
"supportsFormats": "Supports LRC, TTML, and Lyricify Syllable formats"
```

In `src/locales/zh-CN.json`, add:

```json
"detectedTtml": "检测到 TTML 格式",
"detectedLys": "检测到 Lyricify Syllable 格式",
"supportsFormats": "支持 LRC、TTML 和 Lyricify Syllable 格式"
```

- [ ] **Step 3: Verify TypeScript compiles**

Run: `npm run typecheck`
Expected: No errors.

- [ ] **Step 4: Commit**

```bash
git add src/components/Lyrics/LyricsEditDialog.tsx src/locales/en.json src/locales/zh-CN.json
git commit -m "feat: add TTML/LYS format detection in lyrics edit dialog"
```

---

## Task 7: Typography Upgrade

**Files:**

- Modify: `src/components/Lyrics/LyricLine.tsx`
- Modify: `src/components/Lyrics/LyricsPanel.tsx`

- [ ] **Step 1: Update font stack in LyricLine**

In `src/components/Lyrics/LyricLine.tsx`, update the `STANDARD_TEXT_SIZE_CLASSES` to use a better font stack. Since the component uses Tailwind classes, add a custom font-family via inline style on the container div (line ~112):

```tsx
    <div
      onClick={isSeekable ? handleClick : undefined}
      className={`motion-surface flex flex-col items-center gap-1.5 text-center ${
        state === "active" ? "opacity-100" : "opacity-70"
      } ${isSeekable ? "cursor-pointer group/line" : ""}`}
      style={{
        fontFamily:
          '-apple-system, BlinkMacSystemFont, "Helvetica Neue", "Noto Sans SC", "Noto Sans JP", "Noto Sans KR", system-ui, sans-serif',
        ...(presentation === "audience"
          ? {
              transform:
                state === "active"
                  ? `scale(${audiencePresentationSpec.activeScale})`
                  : undefined,
            }
          : undefined),
      }}
    >
```

- [ ] **Step 2: Add font-weight variation by state**

Update the text spans to use `fontWeight` based on state. For the word-level rendering span (line ~131):

```tsx
        <span
          className={(presentation === "audience"
            ? `tracking-tight ${hoverClass}`
            : `${textSizeClass} ${hoverClass}`
          ).trim()}
          style={{
            fontWeight: state === "active" ? 500 : 400,
            ...(presentation === "audience"
              ? {
                  fontSize: audiencePresentationSpec.fontSizePx,
                  lineHeight: audiencePresentationSpec.lineHeightMultiple,
                }
              : undefined),
          }}
        >
```

- [ ] **Step 3: Commit**

```bash
git add src/components/Lyrics/LyricLine.tsx
git commit -m "feat: upgrade lyrics typography with system font stack"
```

---

## Task 8: Line Transition Animations

**Files:**

- Modify: `src/components/Lyrics/LyricsPanel.tsx`
- Modify: `src/components/Lyrics/LyricLine.tsx`

- [ ] **Step 1: Add distance-based state to LyricsPanel**

In `src/components/Lyrics/LyricsPanel.tsx`, compute proximity to active line for each rendered line. Update the line rendering loop (line ~312):

```tsx
          {visibleLines.map((line, idx) => {
            const absoluteIndex = shouldRenderAudiencePlainTextPages
              ? currentPageStart + idx
              : idx;

            const distance = isPlainText
              ? 0
              : Math.abs(absoluteIndex - activeLineIndex);

            return (
              <div
                key={`${absoluteIndex}-${line.time_ms}-${line.text}`}
                data-lyrics-line-index={absoluteIndex}
                data-line-distance={distance}
                className="w-full transition-all duration-400 ease-out"
                style={{
                  filter: distance === 0 ? "blur(0)" : distance === 1 ? "blur(1px)" : `blur(${Math.min(distance, 4)}px)`,
                  transform: distance === 0 ? "scale(1)" : distance === 1 ? "scale(0.98)" : `scale(${Math.max(0.95, 1 - distance * 0.015)})`,
                  opacity: distance === 0 ? 1 : Math.max(0.3, 1 - distance * 0.2),
                  transition: "filter 0.4s ease, transform 0.4s cubic-bezier(0.25, 0.1, 0.25, 1), opacity 0.3s ease",
                  willChange: "transform, opacity, filter",
                  contain: "layout style paint",
                }}
              >
```

- [ ] **Step 2: Test visually**

Run the app and verify:

- Active line has no blur, full opacity, scale 1
- Adjacent lines (±1) have slight blur, reduced opacity
- Distant lines have more blur, lower opacity
- Transitions are smooth

- [ ] **Step 3: Commit**

```bash
git add src/components/Lyrics/LyricsPanel.tsx
git commit -m "feat: add line transition animations with blur and scale"
```

---

## Task 9: Glow Effects

**Files:**

- Modify: `src/components/Lyrics/LyricsPanel.tsx`

- [ ] **Step 1: Add `mix-blend-mode: plus-lighter` to lyrics container**

In `src/components/Lyrics/LyricsPanel.tsx`, add the blend mode to the scroll viewport div (line ~278):

```tsx
      <div
        ref={containerRef}
        key={songId}
        data-testid="lyrics-scroll-viewport"
        className={`flex w-full flex-1 overflow-y-auto animate-[song-fade-in_var(--motion-duration-slow)_var(--motion-ease-emphasized-out)] ${
          isAudience ? "" : spaciousStageLayout ? "px-16 py-10" : "px-12 py-8"
        }`}
        style={{
          mixBlendMode: "plus-lighter" as const,
          ...(isAudience
            ? {
                padding: `${audiencePresentationSpec.verticalPaddingPx}px ${audiencePresentationSpec.horizontalPaddingPx}px`,
              }
            : undefined),
        }}
      >
```

- [ ] **Step 2: Add text-shadow glow to active words in LyricLine**

In `src/components/Lyrics/LyricLine.tsx`, update the active word style (the `wordState === "active"` branch for standard presentation, line ~190):

```tsx
                    : wordState === "active"
                      ? {
                          textShadow:
                            "0 0 12px rgba(255,255,255,0.5), 0 0 4px rgba(255,255,255,0.4)",
                        }
                      : undefined
```

- [ ] **Step 3: Test visually**

Verify the glow effect is visible but subtle. The `plus-lighter` blend mode should make bright text appear to glow against the dark background.

- [ ] **Step 4: Commit**

```bash
git add src/components/Lyrics/LyricsPanel.tsx src/components/Lyrics/LyricLine.tsx
git commit -m "feat: add glow effects with mix-blend-mode plus-lighter"
```

---

## Task 10: Karaoke Fill Effect

**Files:**

- Modify: `src/components/Lyrics/LyricLine.tsx`

- [ ] **Step 1: Add mask-image based karaoke fill to word rendering**

In `src/components/Lyrics/LyricLine.tsx`, refactor the word rendering to use `mask-image` for the fill effect. Each word `<span>` needs:

1. A base color (dim) for the unfilled state
2. A mask that sweeps from left to right over the word's duration
3. A separate "bright" color overlay that shows through the mask

Replace the word rendering block (lines ~144-202) with:

```tsx
{
  line.words!.map((word, idx) => {
    const wordState =
      state === "plain"
        ? "active"
        : state === "active"
          ? idx < activeWordIndex
            ? "past"
            : idx === activeWordIndex
              ? "active"
              : "future"
          : state === "past"
            ? "past"
            : "future";

    const isActiveWord = wordState === "active";
    const isPastWord = wordState === "past";

    // Calculate fill progress for active word
    const wordDuration =
      idx < line.words!.length - 1
        ? line.words![idx + 1].time_ms - word.time_ms
        : 500; // default 500ms for last word
    const elapsed = Math.max(0, adjustedMs - word.time_ms);
    const progress = isActiveWord
      ? Math.min(1, elapsed / Math.max(wordDuration, 1))
      : isPastWord
        ? 1
        : 0;

    return (
      <span
        key={idx}
        className={
          presentation === "audience"
            ? "motion-surface"
            : `motion-surface relative inline-block ${
                wordState === "active"
                  ? "text-white"
                  : wordState === "past"
                    ? "text-[var(--color-text-dimmer)]"
                    : "text-[var(--color-active)]"
              }`
        }
        style={{
          ...(presentation === "audience"
            ? {
                color: colorToCss(
                  wordState === "active"
                    ? audiencePresentationSpec.activeTextColor
                    : wordState === "past"
                      ? audiencePresentationSpec.pastTextColor
                      : audiencePresentationSpec.futureTextColor,
                ),
                textShadow:
                  wordState === "active"
                    ? `0 0 ${audiencePresentationSpec.activeGlowBlurPx}px ${colorToCss(
                        audiencePresentationSpec.activeGlowColor,
                      )}`
                    : undefined,
              }
            : isActiveWord
              ? {
                  textShadow:
                    "0 0 12px rgba(255,255,255,0.5), 0 0 4px rgba(255,255,255,0.4)",
                }
              : undefined),
          ...(isActiveWord && presentation !== "audience"
            ? {
                WebkitMaskImage: `linear-gradient(to right, rgba(0,0,0,1) ${progress * 100}%, rgba(0,0,0,0.2) ${progress * 100}%)`,
                WebkitMaskRepeat: "no-repeat",
                WebkitMaskOrigin: "left",
                maskImage: `linear-gradient(to right, rgba(0,0,0,1) ${progress * 100}%, rgba(0,0,0,0.2) ${progress * 100}%)`,
                maskRepeat: "no-repeat",
                maskOrigin: "left",
              }
            : {}),
        }}
      >
        {word.text}
        {idx < line.words!.length - 1 ? " " : ""}
      </span>
    );
  });
}
```

- [ ] **Step 2: Test with synced lyrics**

Play a song with word-level timing and verify:

- Past words are fully filled (dim color)
- Active word fills progressively from left to right
- Future words are unfilled
- The gradient edge is soft (not a hard cutoff)

- [ ] **Step 3: Commit**

```bash
git add src/components/Lyrics/LyricLine.tsx
git commit -m "feat: add karaoke fill effect with mask-image sweep"
```

---

## Task 11: Render Background Vocals

**Files:**

- Modify: `src/components/Lyrics/LyricLine.tsx`

- [ ] **Step 1: Add bg_words rendering below main text**

In `src/components/Lyrics/LyricLine.tsx`, after the main text rendering and before the romanized text, add background vocal rendering:

```tsx
{
  line.bg_words && line.bg_words.length > 0 ? (
    <span
      className={
        presentation === "audience"
          ? "motion-surface font-medium tracking-tight opacity-40"
          : `motion-surface text-sm font-medium md:text-base opacity-40 ${
              state === "plain" || state === "active"
                ? "text-[var(--color-text-dim)]"
                : state === "past"
                  ? "text-[var(--color-text-dimmer)]"
                  : "text-[var(--color-text-dim)]"
            }`
      }
      style={
        presentation === "audience"
          ? {
              fontSize: audiencePresentationSpec.fontSizePx * 0.55,
              lineHeight: audiencePresentationSpec.lineHeightMultiple,
              color: colorToCss(
                state === "plain" || state === "active"
                  ? audiencePresentationSpec.activeTextColor
                  : state === "past"
                    ? audiencePresentationSpec.pastTextColor
                    : audiencePresentationSpec.futureTextColor,
              ),
              opacity: 0.4,
            }
          : undefined
      }
    >
      {line.bg_words.map((word, idx) => (
        <span key={idx}>
          {word.text}
          {idx < line.bg_words!.length - 1 ? " " : ""}
        </span>
      ))}
    </span>
  ) : null;
}
```

Place this block after the main text `<span>` and before the `romanizedText` block.

- [ ] **Step 2: Commit**

```bash
git add src/components/Lyrics/LyricLine.tsx
git commit -m "feat: render background vocals below main lyric text"
```

---

## Task 12: README Acknowledgments

**Files:**

- Modify: `README.md`

- [ ] **Step 1: Add acknowledgments**

In `README.md`, in the existing Acknowledgments section, add:

```markdown
- [amll-ttml-db](https://github.com/amll-dev/amll-ttml-db) — Community-maintained word-by-word lyrics database (CC0)
- [AMLL (Apple Music-like Lyrics)](https://github.com/amll-dev/applemusic-like-lyrics) — Lyrics rendering techniques (karaoke fill, spring physics, glow effects)
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add amll-ttml-db and AMLL to acknowledgments"
```

---

## Task 13: Final Verification

- [ ] **Step 1: Run full Rust test suite**

Run: `cd src-tauri && cargo test`
Expected: All tests pass.

- [ ] **Step 2: Run TypeScript type check**

Run: `npm run typecheck`
Expected: No type errors.

- [ ] **Step 3: Run linting**

Run: `npm run lint` and `cargo clippy`
Expected: No errors.

- [ ] **Step 4: Build the app**

Run: `npm run build` (or `cargo tauri build --debug`)
Expected: Successful build.

- [ ] **Step 5: Manual smoke test**

1. Play a song with existing LRC lyrics — verify they display correctly
2. Play a song with a sidecar `.lrc` file — verify it loads
3. Test the edit dialog with LRC, TTML, and LYS content — verify format detection
4. Verify visual effects: font, glow, line transitions, karaoke fill
5. Verify background vocals display (if a TTML source with x-bg is available)
