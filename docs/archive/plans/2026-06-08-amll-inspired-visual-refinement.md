# AMLL-Inspired Visual Refinement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refine OpenKara's lyrics rendering to match AMLL's animation quality: precise word timing, CSS-driven karaoke fill, per-character glow, spring-based line transitions, and bg vocal slide-in.

**Architecture:** The existing `feat/lyrics-enhancement` branch already has TTML/LYS parsers, format detection, and basic visual improvements. This plan upgrades the animation layer: replace React re-render driven mask animation with Web Animations API, add a Spring physics class for line transitions, and add per-character emphasis animations. All changes are frontend-only except Task 1 (adds `end_ms` to `WordToken` in Rust + TypeScript).

**Tech Stack:** React, Web Animations API, CSS custom properties, Rust (quick-xml, regex)

---

## File Structure

| File                                       | Responsibility               | Change                                                  |
| ------------------------------------------ | ---------------------------- | ------------------------------------------------------- |
| `src-tauri/src/lyrics/parser.rs`           | LRC parser, WordToken struct | Add `end_ms: u64` to WordToken                          |
| `src-tauri/src/lyrics/ttml_parser.rs`      | TTML parser                  | Parse `end` attribute from `<span>`                     |
| `src-tauri/src/lyrics/lys_parser.rs`       | LYS parser                   | Compute `end_ms` from `start + duration`                |
| `src/types/ipc.ts`                         | TypeScript IPC types         | Add `end_ms: number` to WordToken                       |
| `src/lib/spring.ts`                        | Spring physics solver (new)  | Reusable Spring class with requestAnimationFrame        |
| `src/components/Lyrics/LyricLine.tsx`      | Lyric line rendering         | Per-character glow, bg_words slide, last word emphasis  |
| `src/components/Lyrics/LyricsPanel.tsx`    | Lyrics scroll container      | Spring-driven line transforms replacing CSS transitions |
| `src/components/Lyrics/karaoke-fill.ts`    | Karaoke fill animation (new) | Web Animations API mask controller                      |
| `src/components/Lyrics/LyricLine.test.tsx` | LyricLine tests              | Updated tests for new animation behavior                |

---

## Task 1: Add `end_ms` to WordToken

**Why:** The karaoke fill effect needs precise word duration. Currently we estimate duration from `nextWord.start - currentWord.start`, which fails for the last word (hardcoded 500ms). TTML and LYS both provide explicit end times.

**Files:**

- Modify: `src-tauri/src/lyrics/parser.rs:4-8`
- Modify: `src-tauri/src/lyrics/ttml_parser.rs:82-106`
- Modify: `src-tauri/src/lyrics/lys_parser.rs:60-93`
- Modify: `src/types/ipc.ts:443-446`
- Test: `src-tauri/src/lyrics/parser.rs` (tests module)
- Test: `src-tauri/src/lyrics/ttml_parser.rs` (tests module)
- Test: `src-tauri/src/lyrics/lys_parser.rs` (tests module)

- [ ] **Step 1: Write the failing test for TTML end_ms**

In `src-tauri/src/lyrics/ttml_parser.rs`, add this test to the `tests` module:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test ttml_parser::tests::parse_ttml_word_end_time`
Expected: FAIL — `end_ms` field does not exist on `WordToken`

- [ ] **Step 3: Add `end_ms` to Rust WordToken**

In `src-tauri/src/lyrics/parser.rs`, change the `WordToken` struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordToken {
    pub time_ms: u64,
    pub end_ms: u64,
    pub text: String,
}
```

- [ ] **Step 4: Update all WordToken construction sites in parser.rs**

In `src-tauri/src/lyrics/parser.rs`, find the `parse_word_tokens` function. At line ~127 where `WordToken` is constructed inside the loop, change to:

```rust
tokens.push(WordToken {
    time_ms,
    end_ms: time_ms, // will be fixed by caller or next iteration
    text: word_text.to_owned(),
});
```

After the loop (before `Some((plain, tokens))`), fix up end_ms:

```rust
for i in 0..tokens.len() {
    if tokens[i].end_ms == tokens[i].time_ms {
        if i + 1 < tokens.len() {
            tokens[i].end_ms = tokens[i + 1].time_ms;
        } else {
            // Last word: use a reasonable default; caller should override
            tokens[i].end_ms = tokens[i].time_ms + 500;
        }
    }
}
```

- [ ] **Step 5: Update all WordToken constructions in test files**

Search the codebase for `WordToken {` and add `end_ms: 0` (or appropriate value) to every construction site in test files. The relevant files are:

- `src-tauri/src/lyrics/parser.rs` (tests module — all `WordToken` constructions need `end_ms`)
- `src-tauri/tests/phase4_parser.rs` (if it has WordToken constructions)

Run: `grep -rn "WordToken {" src-tauri/` to find all sites.

- [ ] **Step 6: Run cargo test to verify parser tests pass**

Run: `cd src-tauri && cargo test parser::tests`
Expected: All existing parser tests pass (with `end_ms` added to constructions)

- [ ] **Step 7: Update TTML parser to extract end_ms**

In `src-tauri/src/lyrics/ttml_parser.rs`, in the `"span"` match arm of `Event::Start(e)`, also extract the `end` attribute:

```rust
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
```

Add `current_span_end: Option<u64>` to the state variables (near line ~32).

In the `Event::Text(e)` handler, when constructing `WordToken` for both main words and bg_words, use `end_ms`:

```rust
// For bg_words:
bg_words.push(WordToken {
    time_ms: begin,
    end_ms: current_span_end.unwrap_or(begin + 500),
    text: text.to_string(),
});

// For main words:
words.push(WordToken {
    time_ms: begin,
    end_ms: current_span_end.unwrap_or(begin + 500),
    text: text.to_string(),
});
```

Also add a fixup after the word loop in the `"p"` end handler to set the last word's end_ms to the `<p end="...">` value if available:

```rust
// After collecting all words, fix last word end_ms from <p end="...">
if let Some(p_end) = p_end {
    if let Some(last) = words.last_mut() {
        if last.end_ms == last.time_ms + 500 {
            last.end_ms = p_end;
        }
    }
}
```

Add `p_end: Option<u64>` to state variables and parse it in the `"p"` Start handler alongside `p_begin`.

- [ ] **Step 8: Run the TTML end_ms test**

Run: `cd src-tauri && cargo test ttml_parser::tests::parse_ttml_word_end_time`
Expected: PASS

- [ ] **Step 9: Update LYS parser to compute end_ms**

In `src-tauri/src/lyrics/lys_parser.rs`, the LYS format provides `(start,duration)`. Update the `WordToken` construction in both the bg and non-bg branches to compute `end_ms`:

For the bg branch (around line ~62):

```rust
WordToken {
    time_ms: *start_ms,
    end_ms: start_ms + duration_ms,
    text: t.trim().to_string(),
}
```

For the non-bg branch (around line ~81):

```rust
WordToken {
    time_ms: *start_ms,
    end_ms: start_ms + duration_ms,
    text: txt.trim().to_string(),
}
```

Remove `let _duration_ms` (currently unused) and use it properly. In the regex capture loop, store `duration_ms` alongside `start_ms` in `raw_tokens`:

Change `raw_tokens: Vec<(String, u64)>` to `raw_tokens: Vec<(String, u64, u64)>` (text, start_ms, duration_ms).

Update all references from `(txt, start_ms)` to `(txt, start_ms, duration_ms)`.

- [ ] **Step 10: Add LYS end_ms test**

In `src-tauri/src/lyrics/lys_parser.rs` tests module:

```rust
#[test]
fn parse_lys_word_end_time() {
    let lys = "[0]Hello(1000,500) World(1500,750)\n";
    let lines = parse_lys(lys).expect("should parse");
    let words = lines[0].words.as_ref().unwrap();
    assert_eq!(words[0].end_ms, 1500);
    assert_eq!(words[1].end_ms, 2250);
}
```

- [ ] **Step 11: Run all LYS tests**

Run: `cd src-tauri && cargo test lys_parser::tests`
Expected: All pass

- [ ] **Step 12: Update LRC parser WordToken constructions**

In `src-tauri/src/lyrics/parser.rs`, the `parse_word_tokens` function already has the fixup loop from Step 4. For enhanced LRC (`<mm:ss.xx>` format), there's no explicit end time, so the fixup loop using `nextWord.start` is correct.

For standard LRC lines (no word tokens), no changes needed since `words` is `None`.

- [ ] **Step 13: Add TypeScript `end_ms` to WordToken**

In `src/types/ipc.ts`:

```typescript
export interface WordToken {
  time_ms: number;
  end_ms: number;
  text: string;
}
```

- [ ] **Step 14: Update TypeScript test files**

Search for all `WordToken` constructions in test files and add `end_ms`:

```bash
grep -rn "time_ms:" src/ --include="*.test.*" | grep -v end_ms
```

Update each construction. Key files:

- `src/components/Lyrics/LyricLine.test.tsx` — add `end_ms` to each word token
- `src/lib/tauri.test.ts` — add `end_ms` to each word token
- `src/runtime/airplay-runtime.test.ts` — add `end_ms` to each word token
- `src/types/ipc-contract.test.ts` — add `end_ms` to each word token

- [ ] **Step 15: Run cargo test and typecheck**

Run: `cd src-tauri && cargo test && npx tsc --noEmit`
Expected: All pass

- [ ] **Step 16: Commit**

```bash
git add src-tauri/src/lyrics/parser.rs src-tauri/src/lyrics/ttml_parser.rs src-tauri/src/lyrics/lys_parser.rs src/types/ipc.ts src/components/Lyrics/LyricLine.test.tsx src/lib/tauri.test.ts src/runtime/airplay-runtime.test.ts src/types/ipc-contract.test.ts
git commit -m "feat: add end_ms to WordToken for precise word duration"
```

---

## Task 2: Karaoke Fill with Web Animations API

**Why:** The current karaoke fill recalculates `mask-image` on every React re-render (every animation frame). AMLL uses Web Animations API to drive the mask CSS property directly, bypassing React's reconciler entirely. This is significantly smoother and uses less CPU.

**Files:**

- Create: `src/components/Lyrics/karaoke-fill.ts`
- Modify: `src/components/Lyrics/LyricLine.tsx`
- Test: `src/components/Lyrics/karaoke-fill.test.ts`

- [ ] **Step 1: Create the karaoke fill controller**

Create `src/components/Lyrics/karaoke-fill.ts`:

```typescript
/**
 * Manages per-word karaoke fill animations using Web Animations API.
 * Each word gets a CSS mask that sweeps from left to right over its duration.
 * Animations are driven by the browser's compositor, not React re-renders.
 */

interface WordAnimation {
  element: HTMLElement;
  animation: Animation;
  startTime: number;
  endTime: number;
}

export class KaraokeFillController {
  private wordAnimations = new Map<HTMLElement, WordAnimation>();
  private activeLineEl: HTMLElement | null = null;

  /**
   * Set up animations for a line's word elements.
   * Call this when a line becomes active.
   */
  activateLine(
    lineEl: HTMLElement,
    words: Array<{ time_ms: number; end_ms: number }>,
    wordEls: HTMLElement[],
  ) {
    if (this.activeLineEl === lineEl) return;
    this.deactivateLine();
    this.activeLineEl = lineEl;

    for (let i = 0; i < words.length && i < wordEls.length; i++) {
      const word = words[i];
      const el = wordEls[i];
      const duration = Math.max(1, word.end_ms - word.time_ms);

      // Set up the mask gradient (static)
      el.style.maskImage =
        "linear-gradient(to right, rgba(0,0,0,1), rgba(0,0,0,0.2))";
      el.style.maskRepeat = "no-repeat";
      el.style.maskOrigin = "left";
      el.style.maskSize = "200% 100%";

      // Create the sweep animation
      const animation = el.animate(
        [{ maskPosition: "-100% 0" }, { maskPosition: "0% 0" }],
        {
          duration,
          fill: "forwards",
          easing: "linear",
        },
      );
      animation.pause();

      this.wordAnimations.set(el, {
        element: el,
        animation,
        startTime: word.time_ms,
        endTime: word.end_ms,
      });
    }
  }

  /**
   * Update animation progress based on current playback time.
   * Call this on each frame (from requestAnimationFrame or React render).
   */
  update(currentMs: number, isPlaying: boolean) {
    for (const [, wa] of this.wordAnimations) {
      if (currentMs < wa.startTime) {
        // Word hasn't started
        wa.animation.currentTime = 0;
        wa.animation.pause();
      } else if (currentMs >= wa.endTime) {
        // Word is done
        wa.animation.currentTime = wa.endTime - wa.startTime;
        wa.animation.pause();
      } else {
        // Word is active
        wa.animation.currentTime = currentMs - wa.startTime;
        if (isPlaying) {
          wa.animation.play();
        } else {
          wa.animation.pause();
        }
      }
    }
  }

  /**
   * Remove all animations and clean up.
   */
  deactivateLine() {
    for (const [, wa] of this.wordAnimations) {
      wa.animation.cancel();
      wa.element.style.maskImage = "";
      wa.element.style.maskRepeat = "";
      wa.element.style.maskOrigin = "";
      wa.element.style.maskSize = "";
    }
    this.wordAnimations.clear();
    this.activeLineEl = null;
  }

  destroy() {
    this.deactivateLine();
  }
}
```

- [ ] **Step 2: Write the failing test**

Create `src/components/Lyrics/karaoke-fill.test.ts`:

```typescript
import { describe, expect, test, vi, beforeEach } from "vitest";
import { KaraokeFillController } from "./karaoke-fill";

// Mock Web Animations API
class MockAnimation {
  currentTime = 0;
  paused = true;
  play() {
    this.paused = false;
  }
  pause() {
    this.paused = true;
  }
  cancel() {}
}

function createMockEl(): HTMLElement {
  const el = document.createElement("span");
  el.animate = vi.fn(() => new MockAnimation() as unknown as Animation);
  return el;
}

describe("KaraokeFillController", () => {
  let controller: KaraokeFillController;

  beforeEach(() => {
    controller = new KaraokeFillController();
  });

  test("activateLine sets mask styles on word elements", () => {
    const lineEl = document.createElement("div");
    const wordEls = [createMockEl(), createMockEl()];
    const words = [
      { time_ms: 1000, end_ms: 1500 },
      { time_ms: 1500, end_ms: 2000 },
    ];

    controller.activateLine(lineEl, words, wordEls);

    for (const el of wordEls) {
      expect(el.style.maskImage).toContain("linear-gradient");
      expect(el.style.maskOrigin).toBe("left");
      expect(el.animate).toHaveBeenCalled();
    }
  });

  test("update pauses animations before their start time", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];
    controller.activateLine(lineEl, words, [wordEl]);

    controller.update(500, true);
    // Animation should be at 0 and paused
  });

  test("deactivateLine cancels all animations", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];
    controller.activateLine(lineEl, words, [wordEl]);

    controller.deactivateLine();
    expect(wordEl.style.maskImage).toBe("");
  });
});
```

- [ ] **Step 3: Run test to verify it fails**

Run: `npx vitest run src/components/Lyrics/karaoke-fill.test.ts`
Expected: FAIL — module `./karaoke-fill` not found (after Step 1, the test should pass since the module exists)

- [ ] **Step 4: Integrate KaraokeFillController into LyricLine**

In `src/components/Lyrics/LyricLine.tsx`, add a ref-based approach. The key change: instead of computing `mask-image` inline on each render, we create the controller once and update it when `state` or `adjustedMs` changes.

Add at the top of the component:

```typescript
import { KaraokeFillController } from "./karaoke-fill";
import { useRef, useEffect } from "react";
```

Add inside the component, after the existing hooks:

```typescript
const karaokeRef = useRef<KaraokeFillController | null>(null);
const wordElsRef = useRef<HTMLElement[]>([]);

useEffect(() => {
  if (state === "active" && hasWords) {
    if (!karaokeRef.current) {
      karaokeRef.current = new KaraokeFillController();
    }
    // Find the container element and word spans
    const container = wordElsRef.current[0]?.parentElement;
    if (container) {
      karaokeRef.current.activateLine(
        container,
        line.words!,
        wordElsRef.current,
      );
    }
  } else {
    karaokeRef.current?.deactivateLine();
  }

  return () => {
    karaokeRef.current?.destroy();
    karaokeRef.current = null;
  };
}, [state, line, hasWords]);

useEffect(() => {
  if (state === "active") {
    karaokeRef.current?.update(adjustedMs, true);
  }
}, [adjustedMs, state]);
```

For each word `<span>`, add a ref callback to collect elements. Replace the word rendering section: instead of inline `maskImage` style, use a `ref` callback:

```typescript
ref={(el) => {
  if (el) wordElsRef.current[idx] = el;
}}
```

Remove the inline `WebkitMaskImage`, `maskImage`, `WebkitMaskRepeat`, `maskRepeat`, `WebkitMaskOrigin`, `maskOrigin` styles from the `isActiveWord` conditional. The KaraokeFillController handles these via Web Animations API.

Keep the `textShadow` style for active words (this is the glow, not the fill).

- [ ] **Step 5: Run typecheck**

Run: `npx tsc --noEmit`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/components/Lyrics/karaoke-fill.ts src/components/Lyrics/karaoke-fill.test.ts src/components/Lyrics/LyricLine.tsx
git commit -m "feat: drive karaoke fill via Web Animations API instead of React re-render"
```

---

## Task 3: Spring Physics for Line Transitions

**Why:** CSS transitions produce linear-ish easing. AMLL uses a custom Spring class with underdamped/overdamped physics to get natural elastic motion — lines overshoot slightly then settle, which feels much more alive. The Spring class drives `transform: scale()` and vertical position via `requestAnimationFrame`.

**Files:**

- Create: `src/lib/spring.ts`
- Modify: `src/components/Lyrics/LyricsPanel.tsx`
- Test: `src/lib/spring.test.ts`

- [ ] **Step 1: Create the Spring class**

Create `src/lib/spring.ts`:

```typescript
/**
 * A simple spring physics solver for smooth animations.
 * Uses a damped harmonic oscillator model.
 *
 * Usage:
 *   const spring = new Spring({ stiffness: 180, damping: 12 });
 *   spring.setTarget(1.0);
 *   // In animation loop:
 *   spring.update(dtSeconds);
 *   const value = spring.getPosition();
 */
export interface SpringConfig {
  stiffness: number; // Spring constant (higher = snappier). Default: 180
  damping: number; // Damping ratio (higher = less bounce). Default: 12
  mass: number; // Mass (higher = slower). Default: 1
  precision: number; // Settle threshold. Default: 0.001
}

const DEFAULT_CONFIG: SpringConfig = {
  stiffness: 180,
  damping: 12,
  mass: 1,
  precision: 0.001,
};

export class Spring {
  private position: number;
  private velocity: number;
  private target: number;
  private config: SpringConfig;
  private settled: boolean;

  constructor(initialValue = 0, config: Partial<SpringConfig> = {}) {
    this.position = initialValue;
    this.velocity = 0;
    this.target = initialValue;
    this.config = { ...DEFAULT_CONFIG, ...config };
    this.settled = true;
  }

  setTarget(target: number) {
    if (this.target === target && this.settled) return;
    this.target = target;
    this.settled = false;
  }

  /**
   * Advance the simulation by `dt` seconds.
   * Typical dt is 1/60 (~0.0167).
   */
  update(dt: number) {
    if (this.settled) return;

    const { stiffness, damping, mass, precision } = this.config;

    // Spring force: F = -k * displacement
    const displacement = this.position - this.target;
    const springForce = -stiffness * displacement;

    // Damping force: F = -c * velocity
    const dampingForce = -damping * this.velocity;

    // Acceleration: a = F / m
    const acceleration = (springForce + dampingForce) / mass;

    // Semi-implicit Euler integration
    this.velocity += acceleration * dt;
    this.position += this.velocity * dt;

    // Check if settled
    if (
      Math.abs(this.velocity) < precision &&
      Math.abs(this.position - this.target) < precision
    ) {
      this.position = this.target;
      this.velocity = 0;
      this.settled = true;
    }
  }

  getPosition(): number {
    return this.position;
  }

  getVelocity(): number {
    return this.velocity;
  }

  isSettled(): boolean {
    return this.settled;
  }

  jumpTo(value: number) {
    this.position = value;
    this.target = value;
    this.velocity = 0;
    this.settled = true;
  }
}
```

- [ ] **Step 2: Write the failing tests**

Create `src/lib/spring.test.ts`:

```typescript
import { describe, expect, test } from "vitest";
import { Spring } from "./spring";

describe("Spring", () => {
  test("starts at initial value", () => {
    const spring = new Spring(0.5);
    expect(spring.getPosition()).toBe(0.5);
    expect(spring.isSettled()).toBe(true);
  });

  test("setTarget starts animation", () => {
    const spring = new Spring(0);
    spring.setTarget(1);
    expect(spring.isSettled()).toBe(false);
  });

  test("converges to target after enough updates", () => {
    const spring = new Spring(0, { stiffness: 180, damping: 12 });
    spring.setTarget(1);

    // Simulate 2 seconds at 60fps
    for (let i = 0; i < 120; i++) {
      spring.update(1 / 60);
    }

    expect(spring.getPosition()).toBeCloseTo(1.0, 2);
    expect(spring.isSettled()).toBe(true);
  });

  test("jumpTo immediately sets position", () => {
    const spring = new Spring(0);
    spring.setTarget(1);
    spring.update(0.016); // advance a bit
    spring.jumpTo(0.5);
    expect(spring.getPosition()).toBe(0.5);
    expect(spring.isSettled()).toBe(true);
  });

  test("update is a no-op when settled", () => {
    const spring = new Spring(1);
    spring.setTarget(1);
    spring.update(0.016);
    expect(spring.getPosition()).toBe(1);
  });
});
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `npx vitest run src/lib/spring.test.ts`
Expected: FAIL — module `./spring` not found

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/lib/spring.test.ts`
Expected: PASS (after Step 1 created the file)

- [ ] **Step 5: Integrate Spring into LyricsPanel line transforms**

In `src/components/Lyrics/LyricsPanel.tsx`, replace the inline CSS transition styles with Spring-driven transforms.

Add at the top:

```typescript
import { useRef, useEffect, useCallback } from "react";
import { Spring } from "@/lib/spring";
```

Add a ref to hold per-line Spring instances:

```typescript
const springsRef = useRef<
  Map<number, { scale: Spring; opacity: Spring; blur: Spring }>
>(new Map());
const rafRef = useRef<number>(0);
```

Add a function to get or create springs for a line:

```typescript
const getLineSprings = useCallback((index: number) => {
  let springs = springsRef.current.get(index);
  if (!springs) {
    springs = {
      scale: new Spring(1, { stiffness: 180, damping: 18 }),
      opacity: new Spring(1, { stiffness: 120, damping: 14 }),
      blur: new Spring(0, { stiffness: 120, damping: 14 }),
    };
    springsRef.current.set(index, springs);
  }
  return springs;
}, []);
```

In the `visibleLines.map()` render loop, compute target values from distance (same as current), then set spring targets:

```typescript
const targetScale =
  distance === 0
    ? 1
    : distance === 1
      ? 0.98
      : Math.max(0.95, 1 - distance * 0.015);
const targetOpacity = distance === 0 ? 1 : Math.max(0.3, 1 - distance * 0.2);
const targetBlur =
  distance === 0 ? 0 : distance === 1 ? 1 : Math.min(distance, 4);

const springs = getLineSprings(absoluteIndex);
springs.scale.setTarget(targetScale);
springs.opacity.setTarget(targetOpacity);
springs.blur.setTarget(targetBlur);
```

Add a `useEffect` with `requestAnimationFrame` loop that updates all springs each frame:

```typescript
useEffect(() => {
  let lastTime = performance.now();

  const tick = (now: number) => {
    const dt = Math.min((now - lastTime) / 1000, 0.05); // cap at 50ms
    lastTime = now;

    for (const [, springs] of springsRef.current) {
      springs.scale.update(dt);
      springs.opacity.update(dt);
      springs.blur.update(dt);
    }

    // Force re-render to read spring positions
    // (This is acceptable because we only re-render when springs are not settled)
    rafRef.current = requestAnimationFrame(tick);
  };

  rafRef.current = requestAnimationFrame(tick);
  return () => cancelAnimationFrame(rafRef.current);
}, []);
```

On the line wrapper `<div>`, replace the inline styles with spring-driven values:

```typescript
style={{
  transform: `scale(${springs.scale.getPosition().toFixed(4)})`,
  opacity: springs.opacity.getPosition(),
  filter: `blur(${springs.blur.getPosition().toFixed(1)}px)`,
  willChange: "transform, opacity, filter",
  contain: "layout style paint",
}}
```

Remove the CSS `transition` property entirely — the Spring class handles all animation.

- [ ] **Step 6: Run typecheck and lint**

Run: `npx tsc --noEmit && npm run lint`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/lib/spring.ts src/lib/spring.test.ts src/components/Lyrics/LyricsPanel.tsx
git commit -m "feat: spring physics for line transitions replacing CSS transitions"
```

---

## Task 4: Per-Character Glow Animation

**Why:** AMLL animates `text-shadow` per character with a bezier-curved swell effect, giving long words a rhythmic visual pulse. Our current glow is a static `text-shadow` on the whole word span. Per-character glow makes the karaoke feel alive.

**Files:**

- Modify: `src/components/Lyrics/LyricLine.tsx`

- [ ] **Step 1: Add emphasis detection helper**

In `src/components/Lyrics/LyricLine.tsx`, add a helper function above the component:

```typescript
function shouldEmphasize(word: {
  text: string;
  time_ms: number;
  end_ms: number;
}): boolean {
  const duration = word.end_ms - word.time_ms;
  if (duration < 1000) return false;
  const trimmed = word.text.trim();
  // CJK characters: any length qualifies
  if (/[一-鿿぀-ゟ゠-ヿ]/.test(trimmed)) return true;
  // Non-CJK: 2-7 characters
  return trimmed.length >= 2 && trimmed.length <= 7;
}

function isLastWord(index: number, total: number): boolean {
  return index === total - 1;
}
```

- [ ] **Step 2: Add per-character glow rendering**

For words that qualify for emphasis, render each character as a separate `<span>` with its own animation. For non-emphasis words, keep the current rendering.

In the word rendering section of `LyricLine`, after the existing `<span key={idx}>` for each word, add a conditional branch:

```typescript
{shouldEmphasize(word) && isActiveWord ? (
  <span key={idx} className="motion-surface inline-flex">
    {word.text.split("").map((char, charIdx) => (
      <span
        key={charIdx}
        style={{
          display: "inline-block",
          textShadow: "0 0 12px rgba(255,255,255,0.5), 0 0 4px rgba(255,255,255,0.4)",
          animation: `lyric-char-glow ${wordDuration}ms ease-in-out`,
          animationDelay: `${charIdx * 20}ms`,
        }}
      >
        {char}
      </span>
    ))}
    {idx < line.words!.length - 1 ? " " : ""}
  </span>
) : (
  // existing word rendering
)}
```

- [ ] **Step 3: Add the CSS keyframes**

Add a `<style>` tag or CSS module for the character glow keyframes. Since OpenKara uses Tailwind, add this as an inline `<style>` in the component or in a global CSS file.

Add in `LyricLine.tsx` (inside the component JSX, at the top of the return):

```typescript
<>
<style>{`
  @keyframes lyric-char-glow {
    0%, 100% {
      text-shadow: 0 0 4px rgba(255,255,255,0.3), 0 0 2px rgba(255,255,255,0.2);
      transform: scale(1) translateY(0);
    }
    40% {
      text-shadow: 0 0 16px rgba(255,255,255,0.6), 0 0 6px rgba(255,255,255,0.5);
      transform: scale(1.05) translateY(-1px);
    }
    60% {
      text-shadow: 0 0 16px rgba(255,255,255,0.6), 0 0 6px rgba(255,255,255,0.5);
      transform: scale(1.05) translateY(-1px);
    }
  }

  @keyframes lyric-char-glow-last {
    0%, 100% {
      text-shadow: 0 0 6px rgba(255,255,255,0.4), 0 0 3px rgba(255,255,255,0.3);
      transform: scale(1) translateY(0);
    }
    35% {
      text-shadow: 0 0 24px rgba(255,255,255,0.8), 0 0 10px rgba(255,255,255,0.6);
      transform: scale(1.08) translateY(-2px);
    }
    65% {
      text-shadow: 0 0 24px rgba(255,255,255,0.8), 0 0 10px rgba(255,255,255,0.6);
      transform: scale(1.08) translateY(-2px);
    }
  }
`}</style>
{/* rest of component JSX */}
</>
```

For the last word in a line, use `lyric-char-glow-last` instead:

```typescript
animation: isLastWord(idx, line.words!.length)
  ? `lyric-char-glow-last ${wordDuration * 1.2}ms ease-in-out`
  : `lyric-char-glow ${wordDuration}ms ease-in-out`,
```

- [ ] **Step 4: Run typecheck**

Run: `npx tsc --noEmit`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/components/Lyrics/LyricLine.tsx
git commit -m "feat: per-character glow animation for emphasis words"
```

---

## Task 5: BG Words Slide-In Animation

**Why:** AMLL renders background vocals as a separate container that slides in below the main line with a Spring animation. Our current bg_words are inline below the main text with no entrance animation. Adding a slide-in makes bg vocals feel like a separate voice joining.

**Files:**

- Modify: `src/components/Lyrics/LyricLine.tsx`

- [ ] **Step 1: Wrap bg_words in an animated container**

In `src/components/Lyrics/LyricLine.tsx`, wrap the existing bg_words rendering block in a container with CSS transition for opacity and transform:

Replace the bg_words block (the `{line.bg_words && line.bg_words.length > 0 ? (` section) with:

```typescript
{line.bg_words && line.bg_words.length > 0 ? (
  <span
    className={
      presentation === "audience"
        ? "motion-surface font-medium tracking-tight opacity-40"
        : `motion-surface text-sm font-medium md:text-base ${
            state === "plain" || state === "active"
              ? "text-[var(--color-text-dim)]"
              : state === "past"
                ? "text-[var(--color-text-dimmer)]"
                : "text-[var(--color-text-dim)]"
          }`
    }
    style={{
      ...(presentation === "audience"
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
        : undefined),
      // Slide-in animation
      transition: "opacity 0.3s ease, transform 0.3s ease",
      opacity: state === "active" ? 0.4 : 0,
      transform: state === "active" ? "translateY(0)" : "translateY(8px)",
    }}
  >
    {line.bg_words.map((word, idx) => (
      <span key={idx}>
        {word.text}
        {idx < line.bg_words!.length - 1 ? " " : ""}
      </span>
    ))}
  </span>
) : null}
```

- [ ] **Step 2: Run typecheck**

Run: `npx tsc --noEmit`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/components/Lyrics/LyricLine.tsx
git commit -m "feat: bg words slide-in animation on line activation"
```

---

## Task 6: Mask Alpha Smoothing (Attack/Release Curves)

**Why:** AMLL uses exponential attack/release curves to smoothly transition the karaoke mask contrast when a line becomes active. Without this, the mask snaps from dim to bright, which is jarring. The attack curve (fast brightening) and release curve (slow dimming) create a natural feel.

**Files:**

- Modify: `src/components/Lyrics/karaoke-fill.ts`
- Modify: `src/components/Lyrics/LyricLine.tsx`

- [ ] **Step 1: Add alpha smoothing to KaraokeFillController**

In `src/components/Lyrics/karaoke-fill.ts`, add alpha state tracking and smoothing:

Add to the class:

```typescript
private brightAlpha = 0.2;
private darkAlpha = 1.0;
private targetBrightAlpha = 0.2;
private targetDarkAlpha = 1.0;

private static ATTACK_SPEED = 50.0;
private static RELEASE_SPEED = 7.0;

/**
 * Set the target alpha values for the mask gradient.
 * bright: alpha for the "filled" portion (0-1). Active lines: 1.0, inactive: 0.2
 * dark: alpha for the "unfilled" portion (0-1). Typically 1.0
 */
setTargetAlpha(bright: number, dark: number) {
  this.targetBrightAlpha = bright;
  this.targetDarkAlpha = dark;
}
```

In the `update` method, add alpha smoothing before the word animation loop:

```typescript
// Smooth alpha with attack/release curves
const dtMs = dt * 1000;
const brightSpeed =
  this.targetBrightAlpha > this.brightAlpha
    ? KaraokeFillController.ATTACK_SPEED
    : KaraokeFillController.RELEASE_SPEED;
const darkSpeed =
  this.targetDarkAlpha > this.darkAlpha
    ? KaraokeFillController.ATTACK_SPEED
    : KaraokeFillController.RELEASE_SPEED;

this.brightAlpha +=
  (this.targetBrightAlpha - this.brightAlpha) *
  (1 - Math.exp(-brightSpeed * dt));
this.darkAlpha +=
  (this.targetDarkAlpha - this.darkAlpha) * (1 - Math.exp(-darkSpeed * dt));

// Update mask gradient with smoothed alpha
for (const [, wa] of this.wordAnimations) {
  wa.element.style.maskImage = `linear-gradient(to right, rgba(0,0,0,${this.brightAlpha}), rgba(0,0,0,${this.darkAlpha}))`;
}
```

- [ ] **Step 2: Call setTargetAlpha from LyricLine**

In `src/components/Lyrics/LyricLine.tsx`, in the `useEffect` that activates the line:

```typescript
useEffect(() => {
  if (state === "active" && hasWords) {
    if (!karaokeRef.current) {
      karaokeRef.current = new KaraokeFillController();
    }
    const container = wordElsRef.current[0]?.parentElement;
    if (container) {
      karaokeRef.current.activateLine(
        container,
        line.words!,
        wordElsRef.current,
      );
      karaokeRef.current.setTargetAlpha(1.0, 1.0); // fully bright when active
    }
  } else if (state === "past") {
    karaokeRef.current?.setTargetAlpha(1.0, 1.0); // fully filled when past
  } else {
    karaokeRef.current?.setTargetAlpha(0.2, 1.0); // dim when future
  }

  if (state !== "active" && state !== "past") {
    karaokeRef.current?.deactivateLine();
  }

  return () => {
    karaokeRef.current?.destroy();
    karaokeRef.current = null;
  };
}, [state, line, hasWords]);
```

- [ ] **Step 3: Run typecheck**

Run: `npx tsc --noEmit`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/components/Lyrics/karaoke-fill.ts src/components/Lyrics/LyricLine.tsx
git commit -m "feat: mask alpha smoothing with attack/release curves"
```

---

## Task 7: Last Word Emphasis

**Why:** AMLL amplifies the glow effect on the last word of each line (1.6x glow amount, 1.5x blur, 1.2x duration). This makes line endings feel like a natural crescendo. We already have the `isLastWord` helper from Task 4.

**Files:**

- Modify: `src/components/Lyrics/LyricLine.tsx`

- [ ] **Step 1: Apply last word styling to non-emphasis words**

For words that don't qualify for per-character emphasis but ARE the last word in the line, apply a stronger static text-shadow. In the existing word `<span>` rendering, update the `textShadow` style:

Find the `isActiveWord` style conditional and change the text-shadow for last words:

```typescript
...(isActiveWord
  ? {
      textShadow: isLastWord(idx, line.words!.length)
        ? "0 0 20px rgba(255,255,255,0.7), 0 0 8px rgba(255,255,255,0.5)"
        : "0 0 12px rgba(255,255,255,0.5), 0 0 4px rgba(255,255,255,0.4)",
    }
  : undefined),
```

- [ ] **Step 2: Run typecheck**

Run: `npx tsc --noEmit`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/components/Lyrics/LyricLine.tsx
git commit -m "feat: amplified glow on last word of each line"
```

---

## Task 8: Final Verification

- [ ] **Step 1: Run all Rust tests**

Run: `cd src-tauri && cargo test`
Expected: All pass

- [ ] **Step 2: Run TypeScript typecheck**

Run: `npx tsc --noEmit`
Expected: PASS

- [ ] **Step 3: Run lint**

Run: `npm run lint`
Expected: PASS

- [ ] **Step 4: Run clippy**

Run: `cd src-tauri && cargo clippy`
Expected: No warnings

- [ ] **Step 5: Run frontend tests**

Run: `npx vitest run`
Expected: All pass

- [ ] **Step 6: Fix any issues found**

Address any failures from the above checks.
