import { expect, test } from "./fixtures/accessibility-test";

test.describe("Player accessibility", () => {
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
    await page.getByRole("button", { name: "Earfquake" }).dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });
  });

  test("playback controls expose accessible names and roles", async ({
    page,
  }) => {
    await expect(page.getByRole("slider", { name: /seek/i })).toBeVisible();
    await expect(page.getByRole("slider", { name: "Volume" })).toBeVisible();
    await expect(page.getByRole("button", { name: /previous/i })).toBeVisible();
    await expect(page.getByRole("button", { name: /next/i })).toBeVisible();
  });

  test("playback bar has no axe violations during playback", async ({
    a11y,
  }) => {
    await a11y.axeForThemes();
  });

  test("volume and seek sliders are keyboard operable", async ({ page }) => {
    const seek = page.getByRole("slider", { name: /seek/i });
    await seek.focus();
    await expect(seek).toBeFocused();

    const before =
      (await seek.getAttribute("aria-valuenow")) ?? (await seek.inputValue());
    await page.keyboard.press("ArrowRight");
    const after =
      (await seek.getAttribute("aria-valuenow")) ?? (await seek.inputValue());
    expect(before).not.toBeNull();
    expect(after).not.toBeNull();
    expect(Number(after)).toBeGreaterThanOrEqual(Number(before));

    const volume = page.getByRole("slider", { name: "Volume" });
    await volume.focus();
    await expect(volume).toBeFocused();
    const volumeBefore = await volume.inputValue();
    await page.keyboard.press("ArrowLeft");
    const volumeAfter = await volume.inputValue();
    expect(Number(volumeAfter)).toBeLessThanOrEqual(Number(volumeBefore));
  });

  test("now-playing metadata remains visible during playback", async ({
    page,
  }) => {
    await expect(page.getByText("Earfquake").first()).toBeVisible();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible();
  });
});
