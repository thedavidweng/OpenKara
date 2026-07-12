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

async function readScrollTop(page: import("@playwright/test").Page) {
  return page.locator(VIEWPORT).evaluate((el) => el.scrollTop);
}

test.describe("Lyrics auto-follow", () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(DENSE_LYRICS_SCRIPT);
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();
    await page.getByText("Bohemian Rhapsody").dblclick();
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
      const snapshot = {
        song_id: "aaa111",
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

    await page.getByText("Lyric line 20 ").click();

    // Mock seek returns position_ms = 21000; clock keeps extrapolating.
    await page.waitForTimeout(1000);
    const afterSeek = await readScrollTop(page);
    expect(afterSeek).toBeGreaterThan(100);

    // Follow must continue: subsequent line changes keep moving the viewport.
    await page.waitForTimeout(3000);
    const later = await readScrollTop(page);
    expect(later).toBeGreaterThan(afterSeek);
  });

  test("user wheel unlocks follow and Follow button re-locks", async ({
    page,
  }) => {
    await page.waitForTimeout(2500);

    const viewport = page.locator(VIEWPORT);
    const box = await viewport.boundingBox();
    if (!box) throw new Error("viewport not visible");
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    // Scroll far below the playing line so the re-lock target differs clearly.
    await page.mouse.wheel(0, 800);
    await page.waitForTimeout(200);
    await page.mouse.wheel(0, 800);

    // Follow button pins visible (data-visible) once the user unlocks follow.
    const followButton = page.locator("[data-testid='lyrics-follow-playing']");
    await expect(followButton).toHaveAttribute("data-visible", "true");

    // Wait for wheel momentum to finish before sampling the resting position.
    await page.waitForTimeout(800);
    const unlockedTop = await readScrollTop(page);
    expect(unlockedTop).toBeGreaterThan(400);
    await page.waitForTimeout(1500);
    const stillTop = await readScrollTop(page);
    expect(Math.abs(stillTop - unlockedTop)).toBeLessThan(2);

    // Clicking Follow re-locks to the playing line (far above) and unpins it.
    await followButton.click();
    await expect(followButton).toHaveAttribute("data-visible", "false");
    await page.waitForTimeout(300);
    const relockedTop = await readScrollTop(page);
    expect(relockedTop).toBeLessThan(stillTop - 200);
  });
});
