import { expect, test } from "./fixtures/accessibility-test";

test.describe("Player accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("playback controls expose accessible names and roles", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Earfquake" }).dblclick();

    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });
    await expect(page.getByRole("slider", { name: /seek/i })).toBeVisible();
    await expect(page.getByRole("slider", { name: "Volume" })).toBeVisible();
    await expect(page.getByRole("button", { name: /previous/i })).toBeVisible();
    await expect(page.getByRole("button", { name: /next/i })).toBeVisible();
  });

  test.fixme("playback bar has no axe violations during playback", async ({
    a11y,
  }) => {
    test.fixme("TODO: implement axe scan on the playback bar");
    await a11y.axeCheck();
  });

  test.fixme("volume and seek sliders are keyboard operable and announce values", async ({
    page,
    a11y,
  }) => {
    test.fixme("TODO: implement slider keyboard and announcement checks");
    const seek = page.getByRole("slider", { name: /seek/i });
    await a11y.startLiveRegionMonitor();
    await seek.focus();
    await page.keyboard.press("ArrowRight");
  });

  test.fixme("now-playing info is exposed as a status region", async ({
    page,
  }) => {
    test.fixme("TODO: implement now-playing status region check");
    await expect(page.getByText("Earfquake")).toBeVisible();
  });
});
