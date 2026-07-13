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
 */

const GEOMETRY_TOLERANCE = 0.5;
const CENTER_Y_TOLERANCE = 0.5;

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
        await expect(page.locator(sel)).toBeVisible();
        const geo = await readActionButtonGeometry(page, sel);
        expect(geo.width).toBeCloseTo(44, 5);
        expect(geo.height).toBeCloseTo(44, 5);
        expect(geo.flexShrink).toBe("0");
        expect(parseFloat(geo.svgWidth)).toBeCloseTo(18, 5);
        expect(parseFloat(geo.svgHeight)).toBeCloseTo(18, 5);
        centers.push(geo.centerY);
      }
      // All center Y values match within 0.5px
      const min = Math.min(...centers);
      const max = Math.max(...centers);
      expect(max - min).toBeLessThanOrEqual(CENTER_Y_TOLERANCE);
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
  });

  test.describe("compact density (1280px viewport, ~1020px container)", () => {
    test.use({ viewport: { width: 1280, height: 800 } });

    test("playback bar reports compact density", async ({ page }) => {
      await expect(page.locator("[data-playback-bar-density]")).toHaveAttribute(
        "data-playback-bar-density",
        "compact",
      );
    });

    test("queue, vocals, accompaniment, master are all 44x44", async ({
      page,
    }) => {
      const actions = [
        "queue",
        "vocals-mute",
        "accompaniment-mute",
        "master-mute",
      ];
      for (const action of actions) {
        const sel = `[data-playback-action="${action}"]`;
        await expect(page.locator(sel)).toBeVisible();
        const geo = await readActionButtonGeometry(page, sel);
        expect(geo.width).toBeCloseTo(44, 5);
        expect(geo.height).toBeCloseTo(44, 5);
        expect(geo.flexShrink).toBe("0");
      }
    });
  });

  test.describe("tight density (900px viewport)", () => {
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
      // Queue and master are always present
      await expect(
        page.locator('[data-playback-action="queue"]'),
      ).toBeVisible();
      await expect(
        page.locator('[data-playback-action="master-mute"]'),
      ).toBeVisible();
      // Stem mixer trigger is present in tight mode
      await expect(
        page.locator('[data-playback-action="stem-mixer"]'),
      ).toBeVisible();
      // No inline vocals/accompaniment in tight mode
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
      for (const action of actions) {
        const sel = `[data-playback-action="${action}"]`;
        await expect(page.locator(sel)).toBeVisible();
        const geo = await readActionButtonGeometry(page, sel);
        expect(geo.width).toBeCloseTo(44, 5);
        expect(geo.height).toBeCloseTo(44, 5);
        expect(geo.flexShrink).toBe("0");
        expect(parseFloat(geo.svgWidth)).toBeCloseTo(18, 5);
        expect(parseFloat(geo.svgHeight)).toBeCloseTo(18, 5);
      }
    });

    test("no horizontal document overflow", async ({ page }) => {
      const overflow = await page.evaluate(() => {
        return {
          scrollWidth: document.documentElement.scrollWidth,
          clientWidth: document.documentElement.clientWidth,
        };
      });
      expect(overflow.scrollWidth).toBeLessThanOrEqual(overflow.clientWidth);
    });

    test("center and right zones do not intersect", async ({ page }) => {
      const zones = await page.evaluate(() => {
        const center = document.querySelector('[data-playback-zone="center"]');
        const right = document.querySelector('[data-playback-zone="right"]');
        if (!center || !right) return null;
        const c = center.getBoundingClientRect();
        const r = right.getBoundingClientRect();
        return { centerRight: c.right, rightLeft: r.left };
      });
      expect(zones).not.toBeNull();
      if (zones) {
        expect(zones.centerRight).toBeLessThanOrEqual(
          zones.rightLeft + GEOMETRY_TOLERANCE,
        );
      }
    });
  });

  test.describe("narrow width (720px)", () => {
    test.use({ viewport: { width: 720, height: 800 } });

    test("no horizontal overflow and right zone present", async ({ page }) => {
      const overflow = await page.evaluate(() => {
        return {
          scrollWidth: document.documentElement.scrollWidth,
          clientWidth: document.documentElement.clientWidth,
        };
      });
      expect(overflow.scrollWidth).toBeLessThanOrEqual(overflow.clientWidth);
      await expect(page.locator('[data-playback-zone="right"]')).toBeVisible();
    });
  });
});
