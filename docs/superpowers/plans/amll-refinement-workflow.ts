export const meta = {
  name: "amll-visual-refinement-tdd",
  description:
    "Implement AMLL-inspired visual refinements with TDD: end_ms, Web Animations karaoke fill, Spring physics, per-character glow, bg slide-in, mask alpha, last word emphasis",
  phases: [
    {
      title: "Types",
      detail: "Add end_ms to WordToken in Rust and TypeScript",
    },
    {
      title: "Karaoke",
      detail:
        "Web Animations API karaoke fill controller with mask alpha smoothing",
    },
    {
      title: "Spring",
      detail: "Spring physics class and LyricsPanel integration",
    },
    {
      title: "Visual",
      detail: "Per-character glow, bg slide-in, last word emphasis",
    },
    { title: "Verify", detail: "Run all tests, typecheck, lint, clippy" },
  ],
};

// ── Phase 1: Types ──────────────────────────────────────────────

phase("Types");

const typesResult = await agent(
  `You are implementing Task 1 of the AMLL visual refinement plan at /Users/david/Development/OpenKara.

The plan is at docs/superpowers/plans/2026-06-08-amll-inspired-visual-refinement.md — read Task 1 for full details.

TASK 1: Add end_ms to WordToken

FOLLOW TDD STRICTLY.

STEPS:
1. Add a failing test parse_ttml_word_end_time to src-tauri/src/lyrics/ttml_parser.rs tests module (see plan for exact test code)
2. Run: cd src-tauri && cargo test ttml_parser::tests::parse_ttml_word_end_time — verify FAIL
3. Add end_ms: u64 field to WordToken struct in src-tauri/src/lyrics/parser.rs
4. Update parse_word_tokens in parser.rs to set end_ms (use next word's start, last word +500ms)
5. Update ALL WordToken construction sites in parser.rs tests (search for "WordToken {")
6. Run: cd src-tauri && cargo test parser::tests — verify PASS
7. Update TTML parser (ttml_parser.rs): extract end attribute from spans, add current_span_end state variable, use it in WordToken construction
8. Run: cd src-tauri && cargo test ttml_parser::tests — verify PASS
9. Update LYS parser (lys_parser.rs): change raw_tokens to Vec<(String, u64, u64)>, compute end_ms from start+duration
10. Add parse_lys_word_end_time test to lys_parser.rs
11. Run: cd src-tauri && cargo test lys_parser::tests — verify PASS
12. Add end_ms: number to WordToken interface in src/types/ipc.ts
13. Update ALL TypeScript test files that construct WordToken (grep for "time_ms:" in *.test.* files). Files: LyricLine.test.tsx, tauri.test.ts, airplay-runtime.test.ts, ipc-contract.test.ts
14. Run: npx tsc --noEmit — verify PASS
15. Run: cd src-tauri && cargo test — verify ALL pass
16. Commit: "feat: add end_ms to WordToken for precise word duration"`,
  { label: "end-ms-types", phase: "Types" },
);

// ── Phase 2: Karaoke (parallel with Spring) ────────────────────

phase("Karaoke");

const [karaokeResult, springResult] = await parallel([
  () =>
    agent(
      `You are implementing Tasks 2 and 6 (Karaoke Fill + Mask Alpha Smoothing) at /Users/david/Development/OpenKara.

The plan is at docs/superpowers/plans/2026-06-08-amll-inspired-visual-refinement.md — read Tasks 2 and 6 for full details.

PREREQUISITE: Task 1 is complete. WordToken has end_ms field.

TASK 2: Karaoke Fill with Web Animations API
1. Create src/components/Lyrics/karaoke-fill.ts with KaraokeFillController class (see plan for exact code)
2. Create src/components/Lyrics/karaoke-fill.test.ts with tests
3. Run: npx vitest run src/components/Lyrics/karaoke-fill.test.ts — verify PASS
4. Integrate into LyricLine.tsx: add useRef for controller, useEffect for activation/deactivation, useEffect for update. Remove inline mask styles, use ref callbacks on word spans.
5. Run: npx tsc --noEmit — verify PASS

TASK 6: Mask Alpha Smoothing
6. Add setTargetAlpha method and attack/release smoothing to KaraokeFillController in karaoke-fill.ts
7. Call setTargetAlpha from LyricLine.tsx based on line state (active=1.0/1.0, future=0.2/1.0)
8. Run: npx tsc --noEmit — verify PASS

IMPORTANT: Keep the existing textShadow style for active words (glow). Only the mask-image animation changes.
Commit: "feat: Web Animations API karaoke fill with mask alpha smoothing"`,
      { label: "karaoke-fill", phase: "Karaoke" },
    ),
  () =>
    agent(
      `You are implementing Task 3 (Spring Physics for Line Transitions) at /Users/david/Development/OpenKara.

The plan is at docs/superpowers/plans/2026-06-08-amll-inspired-visual-refinement.md — read Task 3 for full details.

FOLLOW TDD STRICTLY.

STEPS:
1. Create src/lib/spring.ts with Spring class (see plan for exact code)
2. Create src/lib/spring.test.ts with tests
3. Run: npx vitest run src/lib/spring.test.ts — verify PASS
4. Integrate into LyricsPanel.tsx:
   - Import Spring class
   - Add useRef for per-line spring instances (Map of scale/opacity/blur springs)
   - Add requestAnimationFrame loop to update springs each frame
   - Replace inline CSS transition styles with spring-driven values
   - Remove CSS transition property from line wrapper divs
5. Run: npx tsc --noEmit — verify PASS
6. Run: npm run lint — verify PASS
7. Commit: "feat: spring physics for line transitions replacing CSS transitions"`,
      { label: "spring-physics", phase: "Spring" },
    ),
]);

// ── Phase 3: Visual ────────────────────────────────────────────

phase("Visual");

const visualResult = await agent(
  `You are implementing Tasks 4, 5, and 7 (Per-Character Glow, BG Slide-In, Last Word Emphasis) at /Users/david/Development/OpenKara.

The plan is at docs/superpowers/plans/2026-06-08-amll-inspired-visual-refinement.md — read Tasks 4, 5, and 7 for full details.

IMPORTANT: These all modify src/components/Lyrics/LyricLine.tsx. Apply ALL changes carefully.

TASK 4: Per-Character Glow
1. Add shouldEmphasize() and isLastWord() helpers above the component
2. For active words that qualify for emphasis, render each character as a separate <span> with lyric-char-glow CSS animation
3. Add @keyframes lyric-char-glow and lyric-char-glow-last in a <style> tag
4. Last word in emphasis uses lyric-char-glow-last with 1.2x duration

TASK 5: BG Words Slide-In
5. Add CSS transition to bg_words span: "opacity 0.3s ease, transform 0.3s ease"
6. Set opacity 0.4 and translateY(0) when active, opacity 0 and translateY(8px) when not active

TASK 7: Last Word Emphasis
7. For non-emphasis active words, apply stronger text-shadow on the last word: "0 0 20px rgba(255,255,255,0.7), 0 0 8px rgba(255,255,255,0.5)"

After all changes:
1. Run: npx tsc --noEmit — verify PASS
2. Commit: "feat: per-character glow, bg slide-in, last word emphasis"`,
  { label: "visual-effects", phase: "Visual" },
);

// ── Phase 4: Verify ─────────────────────────────────────────────

phase("Verify");

const verifyResult = await agent(
  `You are running final verification at /Users/david/Development/OpenKara.

Run these checks in order:
1. cd /Users/david/Development/OpenKara/src-tauri && cargo test — all Rust tests must pass
2. cd /Users/david/Development/OpenKara && npx tsc --noEmit — TypeScript must compile
3. cd /Users/david/Development/OpenKara && npm run lint — no lint errors
4. cd /Users/david/Development/OpenKara/src-tauri && cargo clippy — no clippy warnings
5. cd /Users/david/Development/OpenKara && npx vitest run — all frontend tests pass

Report any failures with exact error messages. If all pass, report "All checks passed."`,
  { label: "verify", phase: "Verify" },
);

return { typesResult, karaokeResult, springResult, visualResult, verifyResult };
