import { test, expect } from "./fixtures/base-test";

/**
 * Scrollbar platform contract (#97).
 *
 * macOS keeps its native overlay scrollbar under a dark `color-scheme`
 * (scrollbar-color/width: auto, no author ::-webkit-scrollbar geometry);
 * Windows/Linux descendants get a thin semantic scrollbar from the root
 * tokens. Forced colors return control to the system. These are CSS contract
 * assertions — Playwright WebKit is an engine-level regression test, not
 * evidence for macOS WKWebView system preference behavior, so the spec
 * feature-detects serialization differences instead of asserting one
 * rgb(...) spelling.
 *
 * The standard `scrollbar-width`/`scrollbar-color` properties are supported by
 * Chromium but not by WebKit (which uses the `::-webkit-scrollbar` fallback).
 * The `thin`/non-`auto` assertions are therefore Chromium-only; WebKit asserts
 * the mac `auto` contract and `color-scheme`, and the desktop fallback is
 * covered by the source/contract test plus Chromium.
 *
 * Every required scroll surface — library, lyrics, queue, and settings — is
 * exercised with enough content to produce real overflow (scrollHeight >
 * clientHeight), programmatic scroll, and platform-contract inheritance.
 */

type PlatformMarker = "desktop" | "mac";

async function setPlatformMarker(
  page: import("@playwright/test").Page,
  marker: PlatformMarker,
) {
  await page.evaluate((value) => {
    const el = document.querySelector("[data-window-chrome-platform]");
    if (el instanceof HTMLElement) {
      el.setAttribute("data-window-chrome-platform", value);
    }
  }, marker);
}

async function readScrollbarStyles(
  page: import("@playwright/test").Page,
  selector: string,
) {
  return page.evaluate((sel) => {
    const el = document.querySelector(sel);
    if (!(el instanceof HTMLElement)) {
      return null;
    }
    const cs = getComputedStyle(el);
    return {
      scrollbarWidth: cs.scrollbarWidth,
      scrollbarColor: cs.scrollbarColor,
      colorScheme: cs.colorScheme,
    };
  }, selector);
}

function engineName(page: import("@playwright/test").Page): string {
  return page.context().browser()?.browserType().name() ?? "";
}

/** True when the engine actually exposes a computed scrollbar property value. */
function isExposed(value: unknown): value is string {
  return typeof value === "string" && value !== "";
}

/** Assert the desktop thin scrollbar contract on a scroll element (Chromium). */
async function assertDesktopThinContract(
  page: import("@playwright/test").Page,
  locator: import("@playwright/test").Locator,
) {
  if (engineName(page) !== "chromium") return;
  const styles = await locator.evaluate((el) => {
    const cs = getComputedStyle(el);
    return {
      scrollbarWidth: cs.scrollbarWidth,
      scrollbarColor: cs.scrollbarColor,
    };
  });
  expect(styles.scrollbarWidth).toBe("thin");
  expect(styles.scrollbarColor).not.toBe("auto");
}

/** Assert real overflow and programmatic scroll on a scroll element. */
async function assertRealOverflow(locator: import("@playwright/test").Locator) {
  const overflow = await locator.evaluate((el) => ({
    scrollHeight: el.scrollHeight,
    clientHeight: el.clientHeight,
  }));
  expect(overflow.scrollHeight).toBeGreaterThan(overflow.clientHeight);

  // Set and read scrollTop in the same evaluate call so auto-follow engines
  // (e.g. lyrics) cannot reset the position between two round-trips.
  const after = await locator.evaluate((el) => {
    el.scrollTop = 120;
    return el.scrollTop;
  });
  expect(after).toBeGreaterThan(0);
}

/** Generate N fixture songs with unique hashes for overflow testing. */
function generateSongs(count: number) {
  return Array.from({ length: count }, (_, i) => ({
    hash: `s${String(i).padStart(4, "0")}`,
    file_path: `/music/song_${i}.mp3`,
    audio_source_kind: "original",
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language: "en",
    title: `Test Song ${i}`,
    artist: `Artist ${i}`,
    album: `Album ${i}`,
    duration_ms: 180000,
    cover_art: null,
    imported_at: Date.now(),
    original_ext: ".mp3",
  }));
}

/** Generate a lyrics payload with N synced lines for overflow testing. */
function generateLyrics(count: number, songId = "s0000") {
  const lines = Array.from({ length: count }, (_, i) => ({
    time_ms: i * 5000,
    text: `Lyric line ${i} — la la la la la la la la`,
    words: null,
    bg_words: null,
    section: null,
  }));
  return {
    song_id: songId,
    raw_lrc: lines
      .map(
        (l) =>
          `[${String(Math.floor(l.time_ms / 60000)).padStart(2, "0")}:${String(Math.floor((l.time_ms % 60000) / 1000)).padStart(2, "0")}.00]${l.text}`,
      )
      .join("\\n"),
    lines,
    offset_ms: 0,
    source: "manual",
  };
}

test.describe("scrollbar platform contract", () => {
  test("desktop marker applies thin semantic scrollbar with transparent track", async ({
    page,
  }) => {
    await page.goto("/");
    await setPlatformMarker(page, "desktop");

    const root = await readScrollbarStyles(
      page,
      "[data-window-chrome-platform='desktop']",
    );
    expect(root).not.toBeNull();
    expect(root!.colorScheme).toContain("dark");

    // The thin/non-auto contract is expressed through the standard properties,
    // which Chromium supports. WebKit uses the ::-webkit-scrollbar fallback and
    // reports "auto" for scrollbar-width, so only assert the standard contract
    // where the engine actually exposes it.
    const isChromium = engineName(page) === "chromium";
    if (isChromium) {
      expect(root!.scrollbarWidth).toBe("thin");
      expect(root!.scrollbarColor).not.toBe("auto");
      expect(root!.scrollbarColor.length).toBeGreaterThan(0);
    }

    // The root token resolves to the neutral default on every engine.
    const token = await page.evaluate(() => {
      return getComputedStyle(document.documentElement)
        .getPropertyValue("--scrollbar-thumb")
        .trim();
    });
    expect(token).toBe("#6e6e73");

    // A nested scroll descendant inherits the desktop contract.
    const nested = await readScrollbarStyles(page, "[data-testid='song-list']");
    expect(nested).not.toBeNull();
    if (isChromium) {
      expect(nested!.scrollbarWidth).toBe("thin");
      expect(nested!.scrollbarColor).not.toBe("auto");
    }
  });

  test("mac marker keeps native auto scrollbar and dark color-scheme", async ({
    page,
  }) => {
    await page.goto("/");
    await setPlatformMarker(page, "mac");

    const root = await readScrollbarStyles(
      page,
      "[data-window-chrome-platform='mac']",
    );
    expect(root).not.toBeNull();
    expect(root!.colorScheme).toContain("dark");

    // scrollbar-width/color must stay auto so WKWebView keeps native overlay
    // behavior. Some engines do not expose the property at all — feature-detect
    // rather than asserting a single serialization.
    if (isExposed(root!.scrollbarWidth)) {
      expect(root!.scrollbarWidth).toBe("auto");
    }
    if (isExposed(root!.scrollbarColor)) {
      expect(root!.scrollbarColor).toBe("auto");
    }

    // Descendants inherit auto and must not receive author geometry.
    const nested = await readScrollbarStyles(page, "[data-testid='song-list']");
    expect(nested).not.toBeNull();
    if (isExposed(nested!.scrollbarWidth)) {
      expect(nested!.scrollbarWidth).toBe("auto");
    }
    if (isExposed(nested!.scrollbarColor)) {
      expect(nested!.scrollbarColor).toBe("auto");
    }
  });

  test("settings scroll surface produces real overflow and inherits the platform contract", async ({
    page,
  }) => {
    // A shorter viewport forces the settings overlay (inset-0) to overflow so a
    // real scroll container exists; the default 3-song fixture does not overflow
    // the virtualized song list.
    await page.setViewportSize({ width: 1280, height: 400 });
    await page.goto("/");
    await setPlatformMarker(page, "desktop");

    // Open the settings overlay — it has many sections and scrolls.
    await page.getByRole("button", { name: "Settings" }).click();
    const heading = page.getByRole("heading", { name: "Preferences" });
    await expect(heading).toBeVisible();
    // The overlay root is the overflow-y-auto ancestor of the title.
    const overlay = page
      .locator("div.overflow-y-auto")
      .filter({ has: heading });
    await expect(overlay).toBeVisible();

    const overflow = await overlay.evaluate((el) => ({
      scrollHeight: el.scrollHeight,
      clientHeight: el.clientHeight,
    }));
    expect(overflow.scrollHeight).toBeGreaterThan(overflow.clientHeight);

    // Programmatic scroll moves the thumb.
    await overlay.evaluate((el) => {
      el.scrollTop = 120;
    });
    const after = await overlay.evaluate((el) => el.scrollTop);
    expect(after).toBeGreaterThan(0);

    // The overlay inherits the desktop thin contract where the engine exposes
    // the standard properties (Chromium).
    if (engineName(page) === "chromium") {
      const styles = await overlay.evaluate((el) => {
        const cs = getComputedStyle(el);
        return {
          scrollbarWidth: cs.scrollbarWidth,
          scrollbarColor: cs.scrollbarColor,
        };
      });
      expect(styles.scrollbarWidth).toBe("thin");
      expect(styles.scrollbarColor).not.toBe("auto");
    }
  });

  test("library song list produces real overflow and inherits the platform contract", async ({
    page,
  }) => {
    // Populate enough songs to overflow the virtualized song list. The
    // virtualizer estimates 68px per row + 4px gap, so 60 songs produce
    // ~4316px of content — far beyond a 400px viewport.
    const songs = generateSongs(60);
    await page.addInitScript(
      `window.__OPENKARA_E2E__.setMockSongs(${JSON.stringify(songs)});`,
    );

    await page.setViewportSize({ width: 1280, height: 400 });
    await page.goto("/");
    await setPlatformMarker(page, "desktop");

    const songList = page.getByTestId("song-list");
    await expect(songList).toBeVisible();
    await expect(page.getByText("Test Song 0")).toBeVisible();

    await assertRealOverflow(songList);
    await assertDesktopThinContract(page, songList);
  });

  test("lyrics panel produces real overflow and inherits the platform contract", async ({
    page,
    tauriMock,
  }) => {
    // Override the mock lyrics with 60 synced lines so the lyrics viewport
    // overflows.  Then play a song to trigger fetchLyrics.
    const lyrics = generateLyrics(60, "aaa111");
    await page.setViewportSize({ width: 1280, height: 400 });
    await page.goto("/");
    await setPlatformMarker(page, "desktop");

    await tauriMock.setMockLyrics(lyrics);

    // Play the first song to trigger lyrics fetch.
    await page.getByText("Bohemian Rhapsody").dblclick();
    const viewport = page.getByTestId("lyrics-scroll-viewport");
    await expect(viewport).toBeVisible({ timeout: 5000 });
    // Wait for at least one lyric line to render.
    await expect(page.getByText("Lyric line 0")).toBeVisible({
      timeout: 5000,
    });

    await assertRealOverflow(viewport);
    await assertDesktopThinContract(page, viewport);
  });

  test("queue panel produces real overflow and inherits the platform contract", async ({
    page,
  }) => {
    // Populate enough songs so the queue can reference them, then push 40
    // song IDs into the queue store via the same BroadcastChannel the app
    // uses for cross-webview sync.
    const songs = generateSongs(60);
    await page.addInitScript(
      `window.__OPENKARA_E2E__.setMockSongs(${JSON.stringify(songs)});`,
    );

    await page.setViewportSize({ width: 1280, height: 400 });
    await page.goto("/");
    await setPlatformMarker(page, "desktop");

    // Push 40 song hashes into the queue store via BroadcastChannel. The
    // queue store subscribes to "openkara.queue" and accepts messages from
    // a different originId (matching the cross-webview sync contract).
    const queueIds = songs.slice(0, 40).map((s) => s.hash);
    await page.evaluate((ids) => {
      const channel = new BroadcastChannel("openkara.queue");
      channel.postMessage({
        originId: "e2e-scrollbar-test",
        payload: { queue: ids, playHistory: [] },
      });
      channel.close();
    }, queueIds);

    // Open the queue panel.
    const queueButton = page.getByRole("button", { name: /queue/i });
    await queueButton.click();
    await expect(page.getByText(/up next/i)).toBeVisible({ timeout: 5000 });

    // The scrollable list inside the queue panel.
    const queueScroll = page
      .getByTestId("queue-panel")
      .locator("div.flex-1.overflow-y-auto");
    await expect(queueScroll).toBeVisible();
    // Wait for queue items to render.
    await expect(page.getByText("Test Song 0").first()).toBeVisible({
      timeout: 5000,
    });

    await assertRealOverflow(queueScroll);
    await assertDesktopThinContract(page, queueScroll);
  });

  test("forced colors return scrollbar control to the system", async ({
    page,
  }) => {
    await page.goto("/");
    await setPlatformMarker(page, "desktop");

    // Forced-colors emulation is supported in Chromium; WebKit skips gracefully.
    test.skip(
      engineName(page) !== "chromium",
      "forced-colors emulation is Chromium-only",
    );

    await page.emulateMedia({ forcedColors: "active" });
    const root = await readScrollbarStyles(
      page,
      "[data-window-chrome-platform='desktop']",
    );
    expect(root).not.toBeNull();
    if (isExposed(root!.scrollbarColor)) {
      expect(root!.scrollbarColor).toBe("auto");
    }
  });
});
