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
