import { test, expect } from "./fixtures/base-test";

/**
 * Lyrics auto-follow regression tests (real browser, mocked IPC).
 *
 * The playback clock extrapolates client-side from `playingSinceMs`, so the
 * active line advances in real time. These tests use a dense lyric fixture
 * (one line per second) so auto-scroll must move within a short wait.
 */

const DENSE_LYRICS_SCRIPT = `
(() => {
  const lines = [];
  for (let i = 0; i < 60; i++) {
    lines.push({
      time_ms: 1000 + i * 1000,
      // Long text so each rendered line is tall and the centered scroll
      // target leaves the clamped-to-zero zone after just a few lines.
      text:
        "Lyric line " + i + " — the quick brown fox jumps over the lazy dog",
      words: null,
      bg_words: null,
      section: null,
    });
  }
  const payload = { raw_lrc: "dense", lines, offset_ms: 0, source: "manual" };
  const originalInternals = () => window.__TAURI_INTERNALS__;
  // Wrap invoke after the base mock is installed (init scripts run in order).
  const internals = originalInternals();
  const baseInvoke = internals.invoke;
  internals.invoke = (cmd, args) => {
    if (cmd === "fetch_lyrics") {
      return Promise.resolve(JSON.parse(JSON.stringify(payload)));
    }
    return baseInvoke(cmd, args);
  };
})();
`;

const VIEWPORT = "[data-testid='lyrics-scroll-viewport']";
/** Must match `USER_SCROLL_PAUSE_MS` in lyrics-engine. */
const USER_SCROLL_PAUSE_MS = 4000;

async function readScrollTop(page: import("@playwright/test").Page) {
  return page.locator(VIEWPORT).evaluate((el) => el.scrollTop);
}

async function emitLayoutDrivenScroll(page: import("@playwright/test").Page) {
  await page.locator(VIEWPORT).evaluate((el) => {
    // Model the bare scroll event WKWebView can emit when active-line layout
    // changes after a seek. It has no wheel/touch/pointer user intent.
    el.scrollTop += 2;
    el.dispatchEvent(new Event("scroll"));
  });
}

test.describe("Lyrics auto-follow", () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(DENSE_LYRICS_SCRIPT);
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
    await page.getByText("Earfquake").dblclick();
    await expect(page.getByText("Lyric line 0")).toBeVisible({
      timeout: 10000,
    });
  });

  test("auto-scroll follows the active line during playback", async ({
    page,
  }) => {
    const before = await readScrollTop(page);
    await page.waitForTimeout(7000);
    const after = await readScrollTop(page);
    expect(after).toBeGreaterThan(before);
  });

  test("playback clock stays real-time under the 33ms position-event stream", async ({
    page,
  }) => {
    // REGRESSION: the desktop backend emits playback-position every 33ms.
    // Pairing each event's fresh position with a stale playingSinceMs anchor
    // made the displayed clock run at ~2× speed, racing past the last line and
    // freezing auto-scroll. The plain mock never emitted events, so only this
    // stream reproduces the production failure mode.
    await page.evaluate(() => {
      const startedAt = performance.now();
      // transport_generation must match the post-play snapshot so the stream
      // is accepted; contract requires top-level generation === snapshot gen.
      const transportGeneration = 1;
      const snapshot = {
        song_id: "earfquake",
        transport_generation: transportGeneration,
        state: "playing",
        is_playing: true,
        position_ms: 0,
        duration_ms: 354000,
        buffered_ms: 354000,
        volume: 0.8,
        stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
        has_stems: false,
        stem_mode: null,
      };
      setInterval(() => {
        const positionMs = Math.round(performance.now() - startedAt);
        window.__OPENKARA_E2E__.emitEvent("playback-position", {
          ms: positionMs,
          transport_generation: transportGeneration,
          snapshot: { ...snapshot, position_ms: positionMs },
        });
      }, 33);
    });

    await page.waitForTimeout(8000);

    // Lines are 1s apart starting at 1s, so after ~8s the viewport must be
    // centered near line 7 — a 2× clock would already sit near line 15+.
    const centeredLine = await page.evaluate((sel) => {
      const container = document.querySelector(sel) as HTMLElement;
      const center = container.scrollTop + container.clientHeight / 2;
      let best = -1;
      let bestDistance = Infinity;
      for (const el of container.querySelectorAll<HTMLElement>(
        "[data-lyrics-line-index]",
      )) {
        const mid = el.offsetTop + el.clientHeight / 2;
        const distance = Math.abs(mid - center);
        if (distance < bestDistance) {
          bestDistance = distance;
          best = Number(el.dataset.lyricsLineIndex);
        }
      }
      return best;
    }, VIEWPORT);

    expect(centeredLine).toBeGreaterThanOrEqual(5);
    expect(centeredLine).toBeLessThanOrEqual(10);
  });

  test("clicking a line seeks and auto-scroll keeps following", async ({
    page,
  }) => {
    // Let playback advance a little first.
    await page.waitForTimeout(1500);

    // WebKit keeps the lyric-line transform animation mid-flight during
    // auto-scroll, so Playwright's stability gate can burn the entire 30s
    // timeout. Call the DOM click directly to fire the React onClick handler
    // without waiting for animation rest.
    await page.getByText("Lyric line 20 ").evaluate((el) => el.click());

    // Mock streaming seek publishes buffering at 21000, then playing at
    // 21050; the clock keeps extrapolating after recovery.
    await page.waitForTimeout(1000);
    const afterSeek = await readScrollTop(page);
    expect(afterSeek).toBeGreaterThan(100);

    // Follow must continue: subsequent line changes keep moving the viewport.
    await page.waitForTimeout(3000);
    const later = await readScrollTop(page);
    expect(later).toBeGreaterThan(afterSeek);
  });

  test("delayed Tauri line seek keeps following after WebKit layout scroll", async ({
    page,
  }) => {
    await page.evaluate(() => {
      window.__OPENKARA_E2E__.setCommandDelayMs("seek", 350);
    });
    await page.waitForTimeout(1200);

    await page.getByText("Lyric line 20 ").evaluate((el) => el.click());
    // Rust emits the target position before the delayed invoke response. Let
    // WebKit's layout scroll land inside that real production timing window.
    await page.waitForTimeout(120);
    await emitLayoutDrivenScroll(page);
    await page.waitForTimeout(430);

    const afterSeek = await readScrollTop(page);
    expect(afterSeek).toBeGreaterThan(100);

    await page.waitForTimeout(2500);
    const later = await readScrollTop(page);
    expect(later).toBeGreaterThan(afterSeek + 20);
  });

  test("delayed Tauri seek-bar seek keeps following after WebKit layout scroll", async ({
    page,
  }) => {
    await page.evaluate(() => {
      window.__OPENKARA_E2E__.setCommandDelayMs("seek", 250);
    });

    const seekBar = page.locator("[role='slider']");
    const box = await seekBar.boundingBox();
    if (!box) throw new Error("seek bar not visible");
    await page.mouse.click(box.x + box.width * 0.05, box.y + box.height / 2);

    await page.waitForTimeout(80);
    await emitLayoutDrivenScroll(page);
    await page.waitForTimeout(370);

    const afterSeek = await readScrollTop(page);
    expect(afterSeek).toBeGreaterThan(100);

    await page.waitForTimeout(2500);
    const later = await readScrollTop(page);
    expect(later).toBeGreaterThan(afterSeek + 20);
  });

  test("user wheel unlocks follow and Follow button re-locks", async ({
    page,
  }) => {
    // Plain advance wait so the engine has locked onto the playing line before
    // we take over scrollTop. This is NOT a race against the 4s re-lock — the
    // idle timer only starts once the user scroll below unlocks follow.
    await page.waitForTimeout(1000);

    const followButton = page.locator("[data-testid='lyrics-follow-playing']");

    // Deterministic user scroll: write scrollTop far below the playing line and
    // dispatch a real WheelEvent in the same synchronous frame. This exercises
    // the follow guard's wheel path directly — no page.mouse.wheel (WebKit
    // silently drops synthetic wheel deltas) and no smooth-scroll inertia
    // stream that would keep re-arming USER_SCROLL_PAUSE_MS mid-test. A
    // synthetic (untrusted) WheelEvent performs no native scroll, so scrollTop
    // stays exactly where we wrote it.
    await page.locator(VIEWPORT).evaluate((el) => {
      el.scrollTop = 900;
      el.dispatchEvent(
        new WheelEvent("wheel", {
          deltaY: 240,
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    // State assertion (built-in retry): unlocking pins the Follow button.
    await expect(followButton).toHaveAttribute("data-visible", "true");
    const unlockedTop = await readScrollTop(page);
    expect(unlockedTop).toBeGreaterThan(400);

    // While unlocked the engine tracks the user's viewport and never writes
    // scrollTop. Confirm auto-scroll stays released: sampled well inside the 4s
    // idle window, the view holds its browse position and follow is still
    // unlocked (no fixed sleep that races the re-lock).
    await expect
      .poll(async () => Math.abs((await readScrollTop(page)) - unlockedTop) < 2)
      .toBe(true);
    await expect(followButton).toHaveAttribute("data-visible", "true");
    const stillTop = await readScrollTop(page);

    // Clicking Follow re-locks to the playing line (far above) and unpins it.
    // Requires pointer-events-auto on the control (parent overlay is none).
    await followButton.click();
    await expect(followButton).toHaveAttribute("data-visible", "false");
    // Re-lock snaps scrollTop back to the playing line on the next rAF; poll
    // the resting position instead of a fixed post-click sleep.
    await expect
      .poll(async () => (await readScrollTop(page)) < stillTop - 200)
      .toBe(true);
  });

  test("idle timeout re-locks follow and returns scrollTop to the playing line", async ({
    page,
  }) => {
    // Dense lyrics start at 1s; stay early so the active line does not change
    // across the 4s idle window (would mask a missing resume snap).
    await page.waitForTimeout(1500);

    // One synthetic wheel + scrollTop write — no browser wheel inertia stream
    // that would keep re-arming USER_SCROLL_PAUSE_MS in CI.
    await page.locator(VIEWPORT).evaluate((el) => {
      el.scrollTop = 900;
      el.dispatchEvent(
        new WheelEvent("wheel", {
          deltaY: 240,
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    const followButton = page.locator("[data-testid='lyrics-follow-playing']");
    await expect(followButton).toHaveAttribute("data-visible", "true");
    const unlockedTop = await readScrollTop(page);
    expect(unlockedTop).toBeGreaterThan(400);

    await expect(followButton).toHaveAttribute("data-visible", "false", {
      timeout: USER_SCROLL_PAUSE_MS + 1500,
    });
    const relockedTop = await readScrollTop(page);
    expect(relockedTop).toBeLessThan(unlockedTop - 200);
  });
});
