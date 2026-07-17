import { expect, test } from "./fixtures/base-test";

/**
 * Geometry + pressed-state contract for the five primary right-side
 * playback-bar action buttons. Reads actual getBoundingClientRect() and
 * computed style — class assertions alone are insufficient.
 *
 * Density is selected by the playback-bar CONTAINER width (viewport minus
 * sidebar), not the viewport itself. With a default 260px sidebar:
 *   relaxed  container >= 1120  → viewport >= ~1380
 *   compact  container 960..1119 → viewport ~1220..1379
 *   tight    container < 960     → viewport < ~1220
 *
 * Review coverage boundaries (CSS px): 1280, 1040, 900, 760, 720.
 * 1440 is retained as an additional relaxed-density sample. The suite runs
 * in both Chromium and WebKit (Tauri on macOS renders WKWebView).
 */

const GEOMETRY_TOLERANCE = 0.5;
const CENTER_Y_TOLERANCE = 0.5;

// The E2E environment runs on non-mac platforms, so useWindowShellState
// returns DESKTOP_WINDOW_SHELL_STATE (sidebarWidth: 260) rather than the
// Tauri mock's get_window_shell_state payload (sidebar_width: 280), which is
// only fetched on mac. The playback-bar container width is viewport minus
// sidebar, so container thresholds map to viewport thresholds as below.
const SIDEBAR_WIDTH = 260;
const METADATA_COLLAPSE_CONTAINER = 760;
const COVER_ART_COLLAPSE_CONTAINER = 780;

interface ButtonGeometry {
  width: number;
  height: number;
  flexShrink: string;
  centerY: number;
  svgWidth: string;
  svgHeight: string;
}

async function readActionButtonGeometry(
  page: import("@playwright/test").Page,
  selector: string,
): Promise<ButtonGeometry> {
  return page.evaluate((sel) => {
    const el = document.querySelector(sel) as HTMLElement | null;
    if (!el) throw new Error("Button not found: " + sel);
    const rect = el.getBoundingClientRect();
    const svg = el.querySelector("svg");
    const cs = window.getComputedStyle(el);
    return {
      width: rect.width,
      height: rect.height,
      flexShrink: cs.flexShrink,
      centerY: rect.top + rect.height / 2,
      svgWidth: svg?.getAttribute("width") ?? "",
      svgHeight: svg?.getAttribute("height") ?? "",
    };
  }, selector);
}

/** Assert a button is a 44x44 non-shrinking footprint with an 18x18 icon. */
async function expectActionFootprint(
  page: import("@playwright/test").Page,
  selector: string,
): Promise<number> {
  await expect(page.locator(selector)).toBeVisible();
  const geo = await readActionButtonGeometry(page, selector);
  expect(geo.width).toBeCloseTo(44, 5);
  expect(geo.height).toBeCloseTo(44, 5);
  expect(geo.flexShrink).toBe("0");
  expect(parseFloat(geo.svgWidth)).toBeCloseTo(18, 5);
  expect(parseFloat(geo.svgHeight)).toBeCloseTo(18, 5);
  return geo.centerY;
}

/** Assert no center/right-zone intersection and no document overflow. */
async function expectNoOverflowOrZoneIntersection(
  page: import("@playwright/test").Page,
): Promise<void> {
  const overflow = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    clientWidth: document.documentElement.clientWidth,
  }));
  expect(overflow.scrollWidth).toBeLessThanOrEqual(overflow.clientWidth);

  const zones = await page.evaluate(() => {
    const center = document.querySelector('[data-playback-zone="center"]');
    const right = document.querySelector('[data-playback-zone="right"]');
    if (!center || !right) return null;
    const c = center.getBoundingClientRect();
    const r = right.getBoundingClientRect();
    // Reject clipping: each zone must retain a positive width (a collapsed
    // zone indicates the grid overflowed and the browser clipped it).
    // Extending beyond the viewport is already covered by the
    // scrollWidth/clientWidth check above.
    return {
      centerRight: c.right,
      rightLeft: r.left,
      centerWidth: c.width,
      rightWidth: r.width,
    };
  });
  expect(zones).not.toBeNull();
  if (zones) {
    expect(zones.centerRight).toBeLessThanOrEqual(
      zones.rightLeft + GEOMETRY_TOLERANCE,
    );
    expect(zones.centerWidth).toBeGreaterThan(0);
    expect(zones.rightWidth).toBeGreaterThan(0);
  }
}

/** Assert metadata/cover collapse behavior matches the container width. */
async function expectMetadataCollapse(
  page: import("@playwright/test").Page,
  viewportWidth: number,
): Promise<void> {
  const containerWidth = viewportWidth - SIDEBAR_WIDTH;
  const metadataVisible = containerWidth >= METADATA_COLLAPSE_CONTAINER;
  const coverVisible = containerWidth >= COVER_ART_COLLAPSE_CONTAINER;

  const leftZone = page.locator('[data-playback-zone="left"]');
  if (metadataVisible) {
    await expect(leftZone).toBeVisible();
  } else {
    await expect(leftZone).toHaveCount(0);
  }

  // Cover art thumbnail is an <img> inside NowPlayingInfo. It only renders
  // when has_cover_art is true AND the container is wide enough.
  const coverImg = page.locator(
    '[data-playback-zone="left"] img, [data-now-playing-visual-variant] img',
  );
  if (coverVisible && metadataVisible) {
    await expect(coverImg.first()).toBeVisible();
  } else {
    await expect(coverImg).toHaveCount(0);
  }
}

test.describe("Playback controls geometry and pressed state", () => {
  test.beforeEach(async ({ page, tauriMock }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();

    // Start playback so the right-zone action buttons render.
    await page.getByText("Bohemian Rhapsody").dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });

    // Configure a two-stem playing snapshot so vocals/accompaniment mute
    // buttons become operational in relaxed/compact densities. Set the
    // snapshot BEFORE marking separation as completed so the loadStems call
    // triggered by the separation-complete event sees has_stems: true.
    await tauriMock.setPlaybackSnapshot({
      song_id: "aaa111",
      state: "playing",
      is_playing: true,
      has_stems: true,
      stem_mode: "two_stem",
      stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
      volume: 0.8,
    });
    await tauriMock.setSeparationCompleted("aaa111");
  });

  // ── Relaxed density (additional sample) ──────────────────────────────
  test.describe("relaxed density (1440px viewport, ~1180px container)", () => {
    test.use({ viewport: { width: 1440, height: 800 } });

    test("playback bar reports relaxed density", async ({ page }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "relaxed",
      );
    });

    test("queue, vocals, accompaniment, master are all 44x44 with 18px icons", async ({
      page,
    }) => {
      const actions = [
        "queue",
        "vocals-mute",
        "accompaniment-mute",
        "master-mute",
      ];
      const centers: number[] = [];
      for (const action of actions) {
        const sel = `[data-playback-action="${action}"]`;
        centers.push(await expectActionFootprint(page, sel));
      }
      // All center Y values match within 0.5px
      const min = Math.min(...centers);
      const max = Math.max(...centers);
      expect(max - min).toBeLessThanOrEqual(CENTER_Y_TOLERANCE);
    });

    test("no overflow, no center/right-zone intersection, metadata + cover visible", async ({
      page,
    }) => {
      await expectNoOverflowOrZoneIntersection(page);
      await expectMetadataCollapse(page, 1440);
    });

    test("master mute click toggles aria-pressed and data-active", async ({
      page,
    }) => {
      const sel = '[data-playback-action="master-mute"]';
      const btn = page.locator(sel);
      // Initially unmuted (volume=0.8)
      await expect(btn).toHaveAttribute("aria-pressed", "false");
      await expect(btn).not.toHaveAttribute("data-active", "true");

      await btn.click();

      // After muting, volume=0 → aria-pressed=true, data-active=true
      await expect(btn).toHaveAttribute("aria-pressed", "true");
      await expect(btn).toHaveAttribute("data-active", "true");

      // Click again to unmute
      await btn.click();
      await expect(btn).toHaveAttribute("aria-pressed", "false");
      await expect(btn).not.toHaveAttribute("data-active", "true");
    });

    test("vocals mute click toggles aria-pressed and data-active", async ({
      page,
    }) => {
      const sel = '[data-playback-action="vocals-mute"]';
      const btn = page.locator(sel);
      await expect(btn).toHaveAttribute("aria-pressed", "false");

      await btn.click();
      await expect(btn).toHaveAttribute("aria-pressed", "true");
      await expect(btn).toHaveAttribute("data-active", "true");

      await btn.click();
      await expect(btn).toHaveAttribute("aria-pressed", "false");
      await expect(btn).not.toHaveAttribute("data-active", "true");
    });

    test("active mute button CSS rule matches and applies accent color", async ({
      page,
    }) => {
      const sel = '[data-playback-action="vocals-mute"]';
      const btn = page.locator(sel);
      await btn.click();
      await expect(btn).toHaveAttribute("data-active", "true");

      // The [data-active="true"] rule sets color: var(--color-accent),
      // background-color, and box-shadow. Transitions on background-color
      // (from .motion-icon-button) can delay the computed background-color
      // value, so we verify the rule matches the element and that the
      // accent color token resolves correctly instead.
      const info = await page.evaluate((s) => {
        const el = document.querySelector(s) as HTMLElement;
        const cs = window.getComputedStyle(el);
        const rootCs = window.getComputedStyle(document.documentElement);
        const ruleMatches = el.matches(
          '.playback-bar-action-button[data-active="true"]',
        );
        return {
          ruleMatches,
          color: cs.color,
          accentToken: rootCs.getPropertyValue("--color-accent").trim(),
          selectedBgToken: rootCs
            .getPropertyValue("--color-control-selected-bg")
            .trim(),
        };
      }, sel);
      expect(info.ruleMatches).toBe(true);
      expect(info.accentToken).not.toBe("");
      expect(info.selectedBgToken).not.toBe("");
      expect(info.selectedBgToken).not.toBe("transparent");
    });

    test("queue button geometry does not change when panel opens", async ({
      page,
    }) => {
      const sel = '[data-playback-action="queue"]';
      const before = await readActionButtonGeometry(page, sel);
      await page.locator(sel).click();
      const after = await readActionButtonGeometry(page, sel);
      expect(after.width).toBeCloseTo(before.width, 5);
      expect(after.height).toBeCloseTo(before.height, 5);
    });

    test("master mute invokes set_volume and vocals mute invokes set_stem_volume", async ({
      page,
      tauriMock,
    }) => {
      const masterBtn = page.locator('[data-playback-action="master-mute"]');
      await masterBtn.click();
      await expect
        .poll(async () =>
          (await tauriMock.getInvokeCalls()).some(
            (call) => call.cmd === "set_volume",
          ),
        )
        .toBe(true);

      const vocalsBtn = page.locator('[data-playback-action="vocals-mute"]');
      await vocalsBtn.click();
      await expect
        .poll(async () =>
          (await tauriMock.getInvokeCalls()).some(
            (call) => call.cmd === "set_stem_volume",
          ),
        )
        .toBe(true);
    });
  });

  // ── Required boundary: 1280px (compact density) ──────────────────────
  test.describe("compact density (1280px viewport, ~1020px container)", () => {
    test.use({ viewport: { width: 1280, height: 800 } });

    test("playback bar reports compact density", async ({ page }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "compact",
      );
    });

    test("queue, vocals, accompaniment, master are all 44x44 with 18px icons", async ({
      page,
    }) => {
      const actions = [
        "queue",
        "vocals-mute",
        "accompaniment-mute",
        "master-mute",
      ];
      const centers: number[] = [];
      for (const action of actions) {
        const sel = `[data-playback-action="${action}"]`;
        centers.push(await expectActionFootprint(page, sel));
      }
      const min = Math.min(...centers);
      const max = Math.max(...centers);
      expect(max - min).toBeLessThanOrEqual(CENTER_Y_TOLERANCE);
    });

    test("no overflow, no center/right-zone intersection, metadata + cover visible", async ({
      page,
    }) => {
      await expectNoOverflowOrZoneIntersection(page);
      await expectMetadataCollapse(page, 1280);
    });

    test("master mute and vocals mute toggles maintain matching aria-pressed/state", async ({
      page,
    }) => {
      const masterBtn = page.locator('[data-playback-action="master-mute"]');
      await expect(masterBtn).toHaveAttribute("aria-pressed", "false");
      await expect(masterBtn).not.toHaveAttribute("data-active", "true");
      await masterBtn.click();
      await expect(masterBtn).toHaveAttribute("aria-pressed", "true");
      await expect(masterBtn).toHaveAttribute("data-active", "true");

      const vocalsBtn = page.locator('[data-playback-action="vocals-mute"]');
      await expect(vocalsBtn).toHaveAttribute("aria-pressed", "false");
      await vocalsBtn.click();
      await expect(vocalsBtn).toHaveAttribute("aria-pressed", "true");
      await expect(vocalsBtn).toHaveAttribute("data-active", "true");
    });

    test("volume and stem actions invoke the correct commands", async ({
      page,
      tauriMock,
    }) => {
      await page.locator('[data-playback-action="master-mute"]').click();
      await expect
        .poll(async () =>
          (await tauriMock.getInvokeCalls()).some(
            (call) => call.cmd === "set_volume",
          ),
        )
        .toBe(true);

      await page.locator('[data-playback-action="vocals-mute"]').click();
      await expect
        .poll(async () =>
          (await tauriMock.getInvokeCalls()).some(
            (call) => call.cmd === "set_stem_volume",
          ),
        )
        .toBe(true);
    });
  });

  // ── Required boundary: 1040px (tight density, cover-art threshold) ───
  // Container width ~780px sits exactly at the cover-art collapse threshold
  // (PLAYBACK_BAR_COVER_ART_COLLAPSE_WIDTH = 780). Metadata (760) is still
  // visible at this width.
  test.describe("tight density (1040px viewport, ~780px container)", () => {
    test.use({ viewport: { width: 1040, height: 800 } });

    test("playback bar reports tight density", async ({ page }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "tight",
      );
    });

    test("contains only queue, mixer, master — no inline stem actions", async ({
      page,
    }) => {
      await expect(
        page.locator('[data-playback-action="queue"]'),
      ).toBeVisible();
      await expect(
        page.locator('[data-playback-action="master-mute"]'),
      ).toBeVisible();
      await expect(
        page.locator('[data-playback-action="stem-mixer"]'),
      ).toBeVisible();
      await expect(
        page.locator('[data-playback-action="vocals-mute"]'),
      ).toHaveCount(0);
      await expect(
        page.locator('[data-playback-action="accompaniment-mute"]'),
      ).toHaveCount(0);
    });

    test("queue, mixer, master are all 44x44 with 18px icons", async ({
      page,
    }) => {
      const actions = ["queue", "stem-mixer", "master-mute"];
      const centers: number[] = [];
      for (const action of actions) {
        const sel = `[data-playback-action="${action}"]`;
        centers.push(await expectActionFootprint(page, sel));
      }
      const min = Math.min(...centers);
      const max = Math.max(...centers);
      expect(max - min).toBeLessThanOrEqual(CENTER_Y_TOLERANCE);
    });

    test("no overflow, no center/right-zone intersection", async ({ page }) => {
      await expectNoOverflowOrZoneIntersection(page);
    });

    test("metadata visible and cover art visible at the 780 container threshold", async ({
      page,
    }) => {
      // 1040 - 260 = 780 container; cover collapses at < 780, so it stays.
      await expectMetadataCollapse(page, 1040);
    });

    test("master mute toggle maintains matching aria-pressed/state", async ({
      page,
    }) => {
      const masterBtn = page.locator('[data-playback-action="master-mute"]');
      await expect(masterBtn).toHaveAttribute("aria-pressed", "false");
      await masterBtn.click();
      await expect(masterBtn).toHaveAttribute("aria-pressed", "true");
      await expect(masterBtn).toHaveAttribute("data-active", "true");
    });

    test("master mute invokes set_volume", async ({ page, tauriMock }) => {
      await page.locator('[data-playback-action="master-mute"]').click();
      await expect
        .poll(async () =>
          (await tauriMock.getInvokeCalls()).some(
            (call) => call.cmd === "set_volume",
          ),
        )
        .toBe(true);
    });
  });

  // ── Required boundary: 900px (tight density, metadata collapsed) ─────
  test.describe("tight density (900px viewport, ~640px container)", () => {
    test.use({ viewport: { width: 900, height: 800 } });

    test("playback bar reports tight density", async ({ page }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "tight",
      );
    });

    test("contains only queue, mixer, master — no inline stem actions", async ({
      page,
    }) => {
      await expect(
        page.locator('[data-playback-action="queue"]'),
      ).toBeVisible();
      await expect(
        page.locator('[data-playback-action="master-mute"]'),
      ).toBeVisible();
      await expect(
        page.locator('[data-playback-action="stem-mixer"]'),
      ).toBeVisible();
      await expect(
        page.locator('[data-playback-action="vocals-mute"]'),
      ).toHaveCount(0);
      await expect(
        page.locator('[data-playback-action="accompaniment-mute"]'),
      ).toHaveCount(0);
    });

    test("queue, mixer, master are all 44x44 with 18px icons", async ({
      page,
    }) => {
      const actions = ["queue", "stem-mixer", "master-mute"];
      const centers: number[] = [];
      for (const action of actions) {
        const sel = `[data-playback-action="${action}"]`;
        centers.push(await expectActionFootprint(page, sel));
      }
      const min = Math.min(...centers);
      const max = Math.max(...centers);
      expect(max - min).toBeLessThanOrEqual(CENTER_Y_TOLERANCE);
    });

    test("no overflow, no center/right-zone intersection", async ({ page }) => {
      await expectNoOverflowOrZoneIntersection(page);
    });

    test("metadata and cover art collapsed below the 760 threshold", async ({
      page,
    }) => {
      // 900 - 260 = 640 container; both metadata (760) and cover (780) collapse.
      await expectMetadataCollapse(page, 900);
    });

    test("master mute toggle maintains matching aria-pressed/state", async ({
      page,
    }) => {
      const masterBtn = page.locator('[data-playback-action="master-mute"]');
      await expect(masterBtn).toHaveAttribute("aria-pressed", "false");
      await masterBtn.click();
      await expect(masterBtn).toHaveAttribute("aria-pressed", "true");
      await expect(masterBtn).toHaveAttribute("data-active", "true");
    });
  });

  // ── Required boundary: 760px (tight density, metadata collapsed) ─────
  test.describe("tight density (760px viewport, ~500px container)", () => {
    test.use({ viewport: { width: 760, height: 800 } });

    test("playback bar reports tight density", async ({ page }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "tight",
      );
    });

    test("queue, mixer, master are all 44x44 with 18px icons", async ({
      page,
    }) => {
      const actions = ["queue", "stem-mixer", "master-mute"];
      const centers: number[] = [];
      for (const action of actions) {
        const sel = `[data-playback-action="${action}"]`;
        centers.push(await expectActionFootprint(page, sel));
      }
      const min = Math.min(...centers);
      const max = Math.max(...centers);
      expect(max - min).toBeLessThanOrEqual(CENTER_Y_TOLERANCE);
    });

    test("no overflow, no center/right-zone intersection", async ({ page }) => {
      await expectNoOverflowOrZoneIntersection(page);
    });

    test("metadata and cover art collapsed below the 760 threshold", async ({
      page,
    }) => {
      await expectMetadataCollapse(page, 760);
    });

    test("master mute toggle maintains matching aria-pressed/state", async ({
      page,
    }) => {
      const masterBtn = page.locator('[data-playback-action="master-mute"]');
      await expect(masterBtn).toHaveAttribute("aria-pressed", "false");
      await masterBtn.click();
      await expect(masterBtn).toHaveAttribute("aria-pressed", "true");
      await expect(masterBtn).toHaveAttribute("data-active", "true");
    });
  });

  // ── Required boundary: 720px (tight density, narrowest) ──────────────
  test.describe("narrow width (720px viewport, ~460px container)", () => {
    test.use({ viewport: { width: 720, height: 800 } });

    test("playback bar reports tight density", async ({ page }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "tight",
      );
    });

    test("queue, mixer, master are all 44x44 with 18px icons", async ({
      page,
    }) => {
      const actions = ["queue", "stem-mixer", "master-mute"];
      const centers: number[] = [];
      for (const action of actions) {
        const sel = `[data-playback-action="${action}"]`;
        centers.push(await expectActionFootprint(page, sel));
      }
      const min = Math.min(...centers);
      const max = Math.max(...centers);
      expect(max - min).toBeLessThanOrEqual(CENTER_Y_TOLERANCE);
    });

    test("no overflow, no center/right-zone intersection, right zone present", async ({
      page,
    }) => {
      await expectNoOverflowOrZoneIntersection(page);
      await expect(page.locator('[data-playback-zone="right"]')).toBeVisible();
    });

    test("metadata and cover art collapsed below the 760 threshold", async ({
      page,
    }) => {
      await expectMetadataCollapse(page, 720);
    });

    test("master mute toggle maintains matching aria-pressed/state", async ({
      page,
    }) => {
      const masterBtn = page.locator('[data-playback-action="master-mute"]');
      await expect(masterBtn).toHaveAttribute("aria-pressed", "false");
      await masterBtn.click();
      await expect(masterBtn).toHaveAttribute("aria-pressed", "true");
      await expect(masterBtn).toHaveAttribute("data-active", "true");
    });
  });
});
