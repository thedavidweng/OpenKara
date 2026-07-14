import { expect, test } from "./fixtures/base-test";

/**
 * Volume-slider rail geometry contract for the playback bar.
 *
 * Reads actual getBoundingClientRect() and computed style — class assertions
 * alone are insufficient. Verifies the shared layout-token table produces the
 * specified rail widths, symmetric outer gutters, and no overflow at every
 * density boundary.
 *
 * Density is selected by the playback-bar CONTAINER width (viewport minus
 * sidebar), not the viewport itself. With a default 260px sidebar:
 *   relaxed  container >= 1120  → viewport >= 1380
 *   compact  container 960..1119 → viewport 1220..1379
 *   tight    container < 960     → viewport < 1220
 *
 * Boundary tests exercise the exact 1120/960/760/759 container-width
 * thresholds specified in issue #116.
 */

const TOLERANCE = 0.5;

interface RailGeometry {
  width: number;
  flexShrink: string;
  right: number;
}

async function readRailGeometry(
  page: import("@playwright/test").Page,
  selector: string,
): Promise<RailGeometry> {
  return page.evaluate((sel) => {
    const el = document.querySelector(sel) as HTMLInputElement | null;
    if (!el) throw new Error("Slider not found: " + sel);
    const rect = el.getBoundingClientRect();
    const cs = window.getComputedStyle(el);
    return {
      width: rect.width,
      flexShrink: cs.flexShrink,
      right: rect.right,
    };
  }, selector);
}

async function readBarRect(page: import("@playwright/test").Page) {
  return page.evaluate(() => {
    const bar = document.querySelector(
      '[data-playback-bar-visual-variant="unified"]',
    ) as HTMLElement | null;
    if (!bar) throw new Error("Playback bar not found");
    return bar.getBoundingClientRect();
  });
}

async function assertNoHorizontalOverflow(
  page: import("@playwright/test").Page,
) {
  const overflow = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    clientWidth: document.documentElement.clientWidth,
  }));
  expect(overflow.scrollWidth).toBeLessThanOrEqual(overflow.clientWidth);
}

async function assertZonesDoNotIntersect(
  page: import("@playwright/test").Page,
) {
  const zones = await page.evaluate(() => {
    const center = document.querySelector(
      '[data-playback-zone="center"]',
    ) as HTMLElement | null;
    const right = document.querySelector(
      '[data-playback-zone="right"]',
    ) as HTMLElement | null;
    if (!center || !right) return null;
    return {
      centerRight: center.getBoundingClientRect().right,
      rightLeft: right.getBoundingClientRect().left,
    };
  });
  expect(zones).not.toBeNull();
  if (zones) {
    expect(zones.centerRight).toBeLessThanOrEqual(zones.rightLeft + TOLERANCE);
  }
}

test.describe("Playback volume rail layout geometry", () => {
  test.beforeEach(async ({ page, tauriMock }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();

    // Start playback so the right-zone volume controls render.
    await page.getByText("Bohemian Rhapsody").dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });

    // Configure a two-stem playing snapshot so the inline Vocals and
    // Accompaniment sliders render in relaxed/compact densities.
    await tauriMock.setPlaybackSnapshot({
      song_id: "aaa111",
      state: "playing",
      is_playing: true,
      has_stems: true,
      stem_mode: "two_stem",
      stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
      volume: 1,
    });
    await tauriMock.setSeparationCompleted("aaa111");
  });

  // -------------------------------------------------------------------------
  // Relaxed density — rails are 88/88/104 px, 24px Master right inset.
  // -------------------------------------------------------------------------
  test.describe("relaxed density (1380px viewport → 1120px container boundary)", () => {
    test.use({ viewport: { width: 1380, height: 800 } });

    test("rails are 88/88/104 px with flex-shrink:0 and 24px Master right inset", async ({
      page,
    }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "relaxed",
      );

      const vocals = await readRailGeometry(
        page,
        '[aria-label="Vocals"][type="range"]',
      );
      const accomp = await readRailGeometry(
        page,
        '[aria-label="Accompaniment"][type="range"]',
      );
      const master = await readRailGeometry(
        page,
        '[aria-label="Volume"][type="range"]',
      );

      expect(vocals.width).toBeCloseTo(88, 5);
      expect(accomp.width).toBeCloseTo(88, 5);
      expect(master.width).toBeCloseTo(104, 5);

      expect(vocals.flexShrink).toBe("0");
      expect(accomp.flexShrink).toBe("0");
      expect(master.flexShrink).toBe("0");

      // Master right edge is 24px inside the bar's right edge.
      const barRect = await readBarRect(page);
      expect(barRect.right - master.right).toBeCloseTo(24, 5);
    });

    test("no horizontal overflow and zones do not intersect", async ({
      page,
    }) => {
      await assertNoHorizontalOverflow(page);
      await assertZonesDoNotIntersect(page);
    });
  });

  // -------------------------------------------------------------------------
  // Compact density — rails are 72/72/80 px, 16px Master right inset.
  // -------------------------------------------------------------------------
  test.describe("compact density (1220px viewport → 960px container boundary)", () => {
    test.use({ viewport: { width: 1220, height: 800 } });

    test("rails are 72/72/80 px with flex-shrink:0 and 16px Master right inset", async ({
      page,
    }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "compact",
      );

      const vocals = await readRailGeometry(
        page,
        '[aria-label="Vocals"][type="range"]',
      );
      const accomp = await readRailGeometry(
        page,
        '[aria-label="Accompaniment"][type="range"]',
      );
      const master = await readRailGeometry(
        page,
        '[aria-label="Volume"][type="range"]',
      );

      expect(vocals.width).toBeCloseTo(72, 5);
      expect(accomp.width).toBeCloseTo(72, 5);
      expect(master.width).toBeCloseTo(80, 5);

      expect(vocals.flexShrink).toBe("0");
      expect(accomp.flexShrink).toBe("0");
      expect(master.flexShrink).toBe("0");

      const barRect = await readBarRect(page);
      expect(barRect.right - master.right).toBeCloseTo(16, 5);
    });

    test("no horizontal overflow and zones do not intersect", async ({
      page,
    }) => {
      await assertNoHorizontalOverflow(page);
      await assertZonesDoNotIntersect(page);
    });
  });

  // -------------------------------------------------------------------------
  // Tight density with metadata (1020px viewport → 760px container boundary).
  // -------------------------------------------------------------------------
  test.describe("tight density with metadata (1020px viewport → 760px container)", () => {
    test.use({ viewport: { width: 1020, height: 800 } });

    test("no inline stem sliders, Master is 64px with 16px right inset", async ({
      page,
    }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "tight",
      );

      // No inline Vocals/Accompaniment sliders in tight mode.
      await expect(
        page.locator('[aria-label="Vocals"][type="range"]'),
      ).toHaveCount(0);
      await expect(
        page.locator('[aria-label="Accompaniment"][type="range"]'),
      ).toHaveCount(0);

      const master = await readRailGeometry(
        page,
        '[aria-label="Volume"][type="range"]',
      );
      expect(master.width).toBeCloseTo(64, 5);
      expect(master.flexShrink).toBe("0");

      const barRect = await readBarRect(page);
      expect(barRect.right - master.right).toBeCloseTo(16, 5);
    });

    test("left zone is visible at 760px boundary", async ({ page }) => {
      // At exactly 760px container, metadata should NOT collapse (threshold is < 760).
      await expect(page.locator('[data-playback-zone="left"]')).toBeVisible();
    });

    test("no horizontal overflow and zones do not intersect", async ({
      page,
    }) => {
      await assertNoHorizontalOverflow(page);
      await assertZonesDoNotIntersect(page);
    });
  });

  // -------------------------------------------------------------------------
  // Tight density with metadata collapsed (1019px → 759px container boundary).
  // -------------------------------------------------------------------------
  test.describe("tight density with metadata collapsed (1019px viewport → 759px container)", () => {
    test.use({ viewport: { width: 1019, height: 800 } });

    test("left zone collapses, Master is 64px, no overlap or overflow", async ({
      page,
    }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "tight",
      );

      // Metadata collapsed — no left zone (threshold is < 760).
      await expect(page.locator('[data-playback-zone="left"]')).toHaveCount(0);

      const master = await readRailGeometry(
        page,
        '[aria-label="Volume"][type="range"]',
      );
      expect(master.width).toBeCloseTo(64, 5);
      expect(master.flexShrink).toBe("0");

      const barRect = await readBarRect(page);
      expect(barRect.right - master.right).toBeCloseTo(16, 5);

      await assertNoHorizontalOverflow(page);
      await assertZonesDoNotIntersect(page);
    });
  });

  // -------------------------------------------------------------------------
  // Slider interaction — keyboard, pointer drag, tooltip text, IPC calls.
  // -------------------------------------------------------------------------
  test.describe("relaxed density — slider interaction", () => {
    test.use({ viewport: { width: 1380, height: 800 } });

    test("master volume slider responds to keyboard and calls set_volume", async ({
      page,
      tauriMock,
    }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "relaxed",
      );

      const slider = page.getByRole("slider", { name: "Volume" });
      await expect(slider).toBeVisible();

      // Focus and use keyboard to change volume.
      await slider.focus();
      await page.keyboard.press("Home");
      await page.keyboard.press("ArrowRight");

      // Verify set_volume was called.
      const calls = await tauriMock.getInvokeCalls();
      const setVolumeCalls = calls.filter((c) => c.cmd === "set_volume");
      expect(setVolumeCalls.length).toBeGreaterThan(0);
    });

    test("vocals slider responds to keyboard and calls set_stem_volume", async ({
      page,
      tauriMock,
    }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "relaxed",
      );

      const slider = page.getByRole("slider", { name: "Vocals" });
      await expect(slider).toBeVisible();

      await slider.focus();
      await page.keyboard.press("ArrowLeft");

      const calls = await tauriMock.getInvokeCalls();
      const setStemVolumeCalls = calls.filter(
        (c) => c.cmd === "set_stem_volume",
      );
      expect(setStemVolumeCalls.length).toBeGreaterThan(0);
    });

    test("master volume slider responds to pointer drag and calls set_volume", async ({
      page,
      tauriMock,
    }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "relaxed",
      );

      const slider = page.getByRole("slider", { name: "Volume" });
      await expect(slider).toBeVisible();

      const box = await slider.boundingBox();
      expect(box).not.toBeNull();
      if (!box) return;

      // Drag from center to near the right edge to increase volume.
      const startX = box.x + box.width * 0.5;
      const startY = box.y + box.height / 2;
      const endX = box.x + box.width * 0.9;

      await page.mouse.move(startX, startY);
      await page.mouse.down();
      await page.mouse.move(endX, startY, { steps: 5 });
      await page.mouse.up();

      const calls = await tauriMock.getInvokeCalls();
      const setVolumeCalls = calls.filter((c) => c.cmd === "set_volume");
      expect(setVolumeCalls.length).toBeGreaterThan(0);
    });

    test("tooltip shows current volume percentage on hover", async ({
      page,
    }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "relaxed",
      );

      const slider = page.getByRole("slider", { name: "Volume" });
      await expect(slider).toBeVisible();

      // Hover over the slider to trigger the tooltip.
      const box = await slider.boundingBox();
      expect(box).not.toBeNull();
      if (!box) return;

      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);

      // The tooltip has a 600ms show delay — wait for it.
      const tooltip = page.locator('[role="tooltip"]');
      await expect(tooltip).toBeVisible({ timeout: 2000 });

      // Volume is 100% in the test fixture, so the tooltip should show "100%".
      await expect(tooltip).toContainText("Volume");
      await expect(tooltip).toContainText("100%");
    });
  });
});
