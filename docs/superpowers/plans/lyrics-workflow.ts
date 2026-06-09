export const meta = {
  name: "lyrics-enhancement-tdd",
  description:
    "Implement lyrics enhancement plan with TDD: TTML/LYS parsers, extended types, fetch chain, visual improvements",
  phases: [
    { title: "Types", detail: "Extend LyricLine types in Rust and TypeScript" },
    { title: "Parsers", detail: "TTML and LYS parsers with TDD" },
    {
      title: "Integration",
      detail: "Format auto-detection, fetch chain, sidecar expansion",
    },
    {
      title: "Frontend",
      detail: "Edit dialog format detection, visual improvements",
    },
    { title: "Verify", detail: "Run all tests, typecheck, lint" },
  ],
};

// ── Phase 1: Types ──────────────────────────────────────────────

phase("Types");

const typesResult = await agent(
  `You are implementing Task 1 and Task 2 of the lyrics enhancement plan at /Users/david/Development/OpenKara.

TASK 1: Extend Rust LyricLine types
- Modify src-tauri/src/lyrics/parser.rs: Add bg_words: Option<Vec<WordToken>> and section: Option<String> to the LyricLine struct
- Modify src-tauri/src/lyrics/fetch.rs: Add new LyricsSource variants: LrcApiTtml, SidecarTtml, SidecarLys, ManualTtml, ManualLys
- Modify src-tauri/src/lyrics/parser.rs and src-tauri/src/commands/lyrics.rs: Add bg_words: None, section: None to ALL existing LyricLine construction sites (search for "LyricLine {" in the codebase)
- Run: cd src-tauri && cargo test — all existing tests must pass
- Commit: "feat: extend LyricLine with bg_words and section fields"

TASK 2: Extend TypeScript types
- Modify src/types/ipc.ts: Add bg_words: WordToken[] | null and section: string | null to LyricLine interface. Add new LyricsSource variants: "lrc_api_ttml", "sidecar_ttml", "sidecar_lys", "manual_ttml", "manual_lys"
- Run: npm run typecheck — must pass
- Commit: "feat: extend TypeScript LyricLine and LyricsSource types"

TDD NOTE: These are type extensions, not new behavior. The existing tests verify correctness. Follow the plan exactly at docs/superpowers/plans/2026-06-08-lyrics-enhancement.md Tasks 1-2.`,
  { label: "types", phase: "Types" },
);

// ── Phase 2: Parsers (parallel) ─────────────────────────────────

phase("Parsers");

const [ttmlResult, lysResult] = await parallel([
  () =>
    agent(
      `You are implementing Task 3 (TTML Parser) of the lyrics enhancement plan at /Users/david/Development/OpenKara. FOLLOW TDD STRICTLY.

The plan is at docs/superpowers/plans/2026-06-08-lyrics-enhancement.md — read Task 3 for full details.

STEPS:
1. Add quick-xml = "0.37" to src-tauri/Cargo.toml [dependencies]
2. Create src-tauri/src/lyrics/ttml_parser.rs with the todo!() stub and ALL tests from the plan
3. Add pub mod ttml_parser; to src-tauri/src/lyrics/mod.rs
4. Run: cd src-tauri && cargo test ttml_parser — verify tests FAIL with todo!() panic (RED)
5. Replace todo!() with the full implementation from the plan
6. Run: cd src-tauri && cargo test ttml_parser — verify ALL tests PASS (GREEN)
7. Run: cd src-tauri && cargo test — verify no regressions
8. Commit: "feat: add TTML parser with word-level timing support"

IMPORTANT: The LyricLine struct has bg_words and section fields (added in Phase 1). Make sure your parser populates them.`,
      { label: "ttml-parser", phase: "Parsers", isolation: "worktree" },
    ),
  () =>
    agent(
      `You are implementing Task 4 (LYS Parser) of the lyrics enhancement plan at /Users/david/Development/OpenKara. FOLLOW TDD STRICTLY.

The plan is at docs/superpowers/plans/2026-06-08-lyrics-enhancement.md — read Task 4 for full details.

STEPS:
1. Add regex = "1" to src-tauri/Cargo.toml [dependencies]
2. Create src-tauri/src/lyrics/lys_parser.rs with the todo!() stub and ALL tests from the plan
3. Add pub mod lys_parser; to src-tauri/src/lyrics/mod.rs
4. Run: cd src-tauri && cargo test lys_parser — verify tests FAIL with todo!() panic (RED)
5. Replace todo!() with the full implementation from the plan (use LazyLock for regex statics)
6. Run: cd src-tauri && cargo test lys_parser — verify ALL tests PASS (GREEN)
7. Run: cd src-tauri && cargo test — verify no regressions
8. Commit: "feat: add LYS (Lyricify Syllable) parser"

IMPORTANT: The LyricLine struct has bg_words and section fields (added in Phase 1). Make sure your parser populates them.`,
      { label: "lys-parser", phase: "Parsers", isolation: "worktree" },
    ),
]);

// ── Phase 3: Integration ────────────────────────────────────────

phase("Integration");

const integrationResult = await agent(
  `You are implementing Task 5 (Format Auto-Detection and Fetch Chain Updates) of the lyrics enhancement plan at /Users/david/Development/OpenKara.

The plan is at docs/superpowers/plans/2026-06-08-lyrics-enhancement.md — read Task 5 for full details.

PREREQUISITES: Tasks 1-4 are complete. The LyricLine struct has bg_words/section, LyricsSource has new variants, ttml_parser and lys_parser modules exist.

STEPS:
1. Add parse_lyrics_auto function to src-tauri/src/lyrics/fetch.rs (detects TTML/LYS/LRC format)
2. Replace read_sidecar_lrc with read_sidecar_lyrics that checks .ttml > .lys > .lrc
3. Update fetch_lyrics_for_song to use new sidecar function
4. Update LrcApi fetch_timed_lrc to try lrc_ttml field when lrc is empty
5. Update fetch_online_timed_lyrics to detect TTML content from LrcAPI
6. Update src-tauri/src/commands/lyrics.rs: replace all parse_lrc calls with parse_lyrics_auto (5 call sites, keep map_err pattern)
7. Update save_manual_lyrics to detect format and use correct source variant
8. Run: cd src-tauri && cargo test — ALL tests must pass
9. Commit: "feat: add format auto-detection, LrcAPI TTML, sidecar .ttml/.lys"

IMPORTANT: Use the EXACT code from the plan. The ttml_parser and lys_parser modules are already created and tested.`,
  { label: "integration", phase: "Integration" },
);

// ── Phase 4: Frontend + Visual (parallel) ───────────────────────

phase("Frontend");

const [frontendResult, visualResult] = await parallel([
  () =>
    agent(
      `You are implementing Task 6 (Frontend Format Detection) and Task 12 (README Acknowledgments) at /Users/david/Development/OpenKara.

The plan is at docs/superpowers/plans/2026-06-08-lyrics-enhancement.md — read Tasks 6 and 12.

TASK 6:
1. Update src/components/Lyrics/LyricsEditDialog.tsx: add isTtml and isLys detection (see plan for exact code)
2. Update format indicator text to show TTML/LYS/LRC/plain detection
3. Add locale strings to src/locales/en.json and src/locales/zh-CN.json
4. Run: npm run typecheck — must pass
5. Commit: "feat: add TTML/LYS format detection in lyrics edit dialog"

TASK 12:
1. Add 3 acknowledgment entries to README.md in the existing Acknowledgments section
2. Commit: "docs: add LyricsBlossom, amll-ttml-db, and AMLL to acknowledgments"`,
      { label: "frontend", phase: "Frontend" },
    ),
  () =>
    agent(
      `You are implementing Tasks 7-11 (Visual Improvements) at /Users/david/Development/OpenKara.

The plan is at docs/superpowers/plans/2026-06-08-lyrics-enhancement.md — read Tasks 7, 8, 9, 10, 11 for full details.

IMPORTANT: These tasks modify overlapping files (LyricLine.tsx, LyricsPanel.tsx). Apply ALL changes carefully.

TASK 7 (Typography): Add font-family inline style to LyricLine container div, add fontWeight variation by state.

TASK 8 (Line Transitions): In LyricsPanel.tsx, add distance-based blur/scale/opacity to each line wrapper div.

TASK 9 (Glow): Add mix-blend-mode: plus-lighter to lyrics scroll viewport, add text-shadow glow to active words.

TASK 10 (Karaoke Fill): Refactor word rendering in LyricLine.tsx to use mask-image linear-gradient for progressive fill effect.

TASK 11 (Background Vocals): Add bg_words rendering below main text in LyricLine.tsx, before romanized text.

Apply all changes, then:
1. Run: npm run typecheck — must pass
2. Make ONE commit combining all visual changes: "feat: add lyrics visual improvements (typography, transitions, glow, karaoke fill, bg vocals)"

Read the FULL plan for exact code. Do NOT write placeholder code.`,
      { label: "visual", phase: "Frontend" },
    ),
]);

// ── Phase 5: Verify ─────────────────────────────────────────────

phase("Verify");

const verifyResult = await agent(
  `You are running final verification at /Users/david/Development/OpenKara.

Run these checks in order:
1. cd src-tauri && cargo test — all Rust tests must pass
2. npm run typecheck — TypeScript must compile
3. npm run lint — no lint errors
4. cd src-tauri && cargo clippy — no clippy warnings

Report any failures with exact error messages. If all pass, report "All checks passed."`,
  { label: "verify", phase: "Verify" },
);

return {
  typesResult,
  ttmlResult,
  lysResult,
  integrationResult,
  frontendResult,
  visualResult,
  verifyResult,
};
