# Lyrics Enhancement Design Spec

Date: 2026-06-08
Status: Approved

## Overview

Enhance OpenKara's lyrics system with:

1. TTML and .lys format parsers (backend)
2. Extended lyric types for richer metadata
3. LrcAPI TTML consumption from existing provider
4. Sidecar .ttml/.lys file support
5. Visual improvements inspired by AMLL
6. README acknowledgments

## 1. Extended Types

### Rust (`src-tauri/src/lyrics/parser.rs`)

```rust
pub struct WordToken {
    pub time_ms: u64,
    pub text: String,
}

pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
    pub words: Option<Vec<WordToken>>,
    pub bg_words: Option<Vec<WordToken>>,  // NEW: background vocal words
    pub section: Option<String>,            // NEW: "Verse", "Chorus", etc.
}
```

### TypeScript (`src/types/ipc.ts`)

```typescript
export interface WordToken {
  time_ms: number;
  text: string;
}

export interface LyricLine {
  time_ms: number;
  text: string;
  words: WordToken[] | null;
  bg_words: WordToken[] | null; // NEW
  section: string | null; // NEW
}
```

### Migration

All existing LRC parsing produces `bg_words: None` and `section: None`. No data migration needed. The SQLite cache stores `raw_lrc` (the original format string), so cached entries will be re-parsed with the new fields on next load.

Note: The `raw_lrc` column name is a misnomer when storing TTML/lyrics content, but renaming it would require a schema migration for no functional benefit. The `source` column already indicates the format.

### UI Rendering of New Fields

- `bg_words`: Rendered below the main lyric line at a smaller font size and lower opacity (0.4), matching the romanized text display pattern. Background words use the same karaoke fill animation as main words.
- `section`: Not rendered in the initial implementation. Stored for future use (e.g., section labels like "Verse 1", "Chorus" shown above lyric lines). No UI changes needed now.

## 2. TTML Parser

### File

`src-tauri/src/lyrics/ttml_parser.rs` (new)

### Dependencies

- `quick-xml` crate for XML parsing

### Input/Output

- Input: TTML XML string (W3C TTML1 with Apple Music extensions)
- Output: `Vec<LyricLine>` with `words`, `bg_words`, and `section` populated

### Parsing Rules

1. **Root**: `<tt>` element, ignore namespace declarations
2. **Body**: `<body>` → `<div>` blocks (each `<div>` may have `itunes:song-part` attribute)
3. **Lines**: `<p begin="..." end="...">` → `LyricLine`
   - `begin` attribute → `time_ms` (parse timestamp formats: `MM:SS.fff`, `HH:MM:SS.fff`, `SS.fff`, `Ns`)
   - Text content of `<p>` → `text` (concatenation of all text spans)
4. **Words**: `<span begin="..." end="...">word</span>` → `WordToken`
   - Skip `<span>` elements with `ttm:role="x-translation"` (translations not needed)
   - Skip `<span>` elements with `ttm:role="x-roman"` (OpenKara has its own romanizer)
5. **Background vocals**: `<span ttm:role="x-bg">` containing timed spans → `bg_words`
6. **Sections**: `<div itunes:song-part="...">` → `section` field on all lines within that div
7. **Line-level mode**: If `itunes:timing="Line"` on the `<p>` parent, no word tokens (line-level only)
8. **Timestamp parsing**: Support `MM:SS.fff`, `HH:MM:SS.fff`, bare seconds (`SS.fff`), and `Ns` suffix

### Error Handling

- Malformed XML → return error (bail with context)
- Missing timestamps → skip that `<p>` element
- Empty text → skip

### Tests

- Parse minimal TTML with one line, no words
- Parse TTML with word-level timing
- Parse TTML with background vocals
- Parse TTML with section labels
- Parse TTML with translations (verify they're discarded)
- Parse TTML with `itunes:timing="Line"` mode
- Parse various timestamp formats
- Handle malformed XML gracefully

## 3. LYS Parser

### File

`src-tauri/src/lyrics/lys_parser.rs` (new)

### Input/Output

- Input: LYS string
- Output: `Vec<LyricLine>` with `words` populated, `bg_words` populated when background flag set

### Parsing Rules

1. Split input on newlines
2. Each line must start with `[digit]` (property prefix, 0-8)
3. After the prefix, extract all `text(start,duration)` tokens via regex
4. Property encoding (0-8):
   - `prop` values 0-2: no background vocal → words go to `words`
   - `prop` values 3-5: explicitly not background → words go to `words`
   - `prop` values 6-8: background vocal → words go to `bg_words`
   - (The digit encodes duet alignment in the upper bits, but OpenKara ignores duet for now)
5. When `prop == 0`: detect background by checking if first token text starts with `(` and last ends with `)` — strip parens, put in `bg_words`
6. `time_ms` for the line = `min(start)` of all tokens in the line
7. `text` = concatenation of all token texts

### Regex

```rust
// Property prefix
let prop_re = Regex::new(r"^\[(\d)\]")?;
// Word tokens: text(start,duration)
let word_re = Regex::new(r"(.*?)\((\d+),(\d+)\)")?;
```

### Tests

- Parse minimal LYS with one line
- Parse LYS with multiple words per line
- Parse LYS with background vocals (prop 6/7/8)
- Parse LYS with background detection via parentheses (prop 0)
- Parse empty input
- Parse line with no valid tokens (skip)

## 4. Fetch Chain Updates

### File

`src-tauri/src/lyrics/fetch.rs`

### Changes

1. **LrcAPI TTML**: In `TimedLyricsProvider::LrcApi`, after checking `lrc` field, also check `lrc_ttml` field. If `lrc` is empty/unsynced but `lrc_ttml` has content, return it with a new source variant `LrcApiTtml`.

2. **Sidecar expansion**: `read_sidecar_lrc` becomes `read_sidecar_lyrics`. Check order:
   - `song.ttml` (highest priority — richest format)
   - `song.lys`
   - `song.lrc` (existing behavior)

3. **New source variants**:

   ```rust
   pub enum LyricsSource {
       LrcLib,
       LrcApi,
       LrcApiTtml,    // NEW
       Embedded,
       Sidecar,
       SidecarTtml,   // NEW
       SidecarLys,    // NEW
       Manual,
       ManualTtml,    // NEW
       ManualLys,     // NEW
   }
   ```

4. **Format detection in `parse_lyrics`**: A new function `parse_lyrics_auto` that detects format:
   - Starts with `<?xml` or `<tt` → TTML parser
   - Matches `^\[\d\]` regex → LYS parser
   - Otherwise → LRC parser (existing)

5. **Cache**: The `lyrics` table stores `raw_lrc` (TEXT). This field now stores the original format string regardless of format. The `source` column indicates which parser to use on re-load.

### TypeScript Source Type

```typescript
export type LyricsSource =
  | "lrc_lib"
  | "lrc_api"
  | "lrc_api_ttml" // NEW
  | "embedded"
  | "sidecar"
  | "sidecar_ttml" // NEW
  | "sidecar_lys" // NEW
  | "manual"
  | "manual_ttml" // NEW
  | "manual_lys"; // NEW
```

## 5. Manual Entry Format Detection

### File

`src/components/Lyrics/LyricsEditDialog.tsx`

### Changes

When user pastes text into the edit dialog, detect format:

- Starts with `<?xml` or `<tt` → parse as TTML
- Matches `^\[\d\]` on first non-empty line → parse as LYS
- Otherwise → parse as LRC (existing behavior)

Pass the detected format to the backend `save_manual_lyrics` command so it stores the correct source variant.

## 6. Visual Improvements

### 6a. Karaoke Fill Effect

**File**: `src/components/Lyrics/LyricLine.tsx`

**Technique**: `mask-image` with animated `mask-position` (same approach as AMLL).

Each word `<span>` gets a CSS mask that sweeps from left to right over the word's duration:

```css
.lyric-word {
  display: inline;
  -webkit-mask-image: linear-gradient(
    to right,
    rgba(0, 0, 0, 1) 0%,
    rgba(0, 0, 0, 0.2) 100%
  );
  -webkit-mask-repeat: no-repeat;
  -webkit-mask-origin: left;
  -webkit-mask-size: 200% 100%;
  -webkit-mask-position: -100% 0; /* fully dim */
}
```

Animation: Use Web Animations API to animate `mask-position` from `-100%` to `0%` over the word's duration. The animation is created when the word becomes active and paused/resumed based on playback state.

**Fade width**: The gradient has a soft edge (controlled by the 0.2 end value) rather than a hard cutoff, matching Apple Music's karaoke style.

**Implementation notes**:

- Use `useRef` to store animation references per word
- Create animations lazily (only when the word's line becomes active)
- Use `animation.pause()` / `animation.play()` for seek/pause handling
- Fallback: if `mask-image` is unsupported, fall back to the current discrete color switching

### 6b. Line Transition Animations

**File**: `src/components/Lyrics/LyricsPanel.tsx`, `src/components/Lyrics/LyricLine.tsx`

**Technique**: CSS transitions on `transform`, `opacity`, `filter`.

```css
.lyric-line-wrapper {
  transition:
    transform 0.4s cubic-bezier(0.25, 0.1, 0.25, 1),
    opacity 0.3s ease,
    filter 0.3s ease;
}

/* Active line */
.lyric-line-wrapper[data-state="active"] {
  opacity: 1;
  filter: blur(0);
  transform: scale(1);
}

/* Adjacent lines (±1 from active) */
.lyric-line-wrapper[data-state="near"] {
  opacity: 0.5;
  filter: blur(1px);
  transform: scale(0.97);
}

/* Distant lines (±2+ from active) */
.lyric-line-wrapper[data-state="distant"] {
  opacity: 0.3;
  filter: blur(3px);
  transform: scale(0.95);
}
```

**State computation**: In `LyricsPanel`, compute `near`/`distant` based on `Math.abs(index - activeLineIndex)`.

**Performance**: Use `will-change: transform, opacity, filter` on line wrappers. Use `contain: layout style paint` on individual lines.

### 6c. Typography Upgrade

**Approach**: Use Apple's system font stack for native feel on macOS/iOS, with good fallbacks.

```css
.lyrics-container {
  font-family:
    -apple-system, BlinkMacSystemFont, "Helvetica Neue", "Noto Sans SC",
    "Noto Sans JP", "Noto Sans KR", system-ui, sans-serif;
  font-weight: 400;
  line-height: 1.3;
}

/* Active line gets slightly heavier weight */
.lyric-line[data-state="active"] {
  font-weight: 500;
}
```

The `Noto Sans CJK` variants ensure good Chinese/Japanese/Korean character rendering when the system font doesn't cover them.

### 6d. Glow Effects

**File**: `src/components/Lyrics/LyricsPanel.tsx`

**Technique**: `mix-blend-mode: plus-lighter` on the lyrics container for additive blending.

```css
.lyrics-container {
  mix-blend-mode: plus-lighter;
}

/* Subtle text-shadow on active words */
.lyric-word[data-state="active"] {
  text-shadow:
    0 0 12px rgba(255, 255, 255, 0.4),
    0 0 4px rgba(255, 255, 255, 0.3);
}
```

The `plus-lighter` blend mode makes overlapping bright elements glow naturally without needing heavy shadow effects. This works best against dark backgrounds (which OpenKara already uses).

### Visual Dependencies

No new npm dependencies needed. All techniques use CSS features available in Tauri's WebView:

- `mask-image` / `-webkit-mask-image`: Safari 15.4+ (macOS WebView), Chromium 120+ (Windows WebView2)
- `mix-blend-mode: plus-lighter`: Safari 15.4+, Chromium 111+
- `filter: blur()`: All modern browsers
- Web Animations API: All modern browsers

## 7. README Acknowledgments

Add a section to README.md:

```markdown
## Acknowledgments

- [amll-ttml-db](https://github.com/amll-dev/amll-ttml-db) — Community-maintained word-by-word lyrics database (CC0)
- [AMLL (Apple Music-like Lyrics)](https://github.com/amll-dev/applemusic-like-lyrics) — Lyrics rendering techniques (karaoke fill, spring physics, glow effects)
```

## Implementation Order

1. **Extended types** — Rust and TypeScript type changes (foundational, everything depends on this)
2. **TTML parser** — New Rust module with tests
3. **LYS parser** — New Rust module with tests
4. **Fetch chain updates** — LrcAPI TTML, sidecar expansion, format detection
5. **Manual entry format detection** — Frontend changes to edit dialog
6. **Typography upgrade** — CSS changes (quick win, no logic)
7. **Line transitions** — CSS + component changes
8. **Glow effects** — CSS changes
9. **Karaoke fill effect** — Most complex visual change, depends on word timing
10. **README acknowledgments** — Docs update

Steps 1-5 are backend/logic changes. Steps 6-9 are visual changes. Step 10 is docs. Steps 6-9 can be done in any order.
