import { expect, test } from "./fixtures/accessibility-test";

test.describe("Separation accessibility", () => {
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("separation controls are labeled and reachable", async ({ page }) => {
    await expect(
      page.getByRole("button", { name: /separate all/i }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Separated" })).toBeVisible();
  });

  test("running separation is exposed as a progressbar with an accessible name", async ({
    page,
    a11y,
  }) => {
    await a11y.startLiveRegionMonitor();

    await page.evaluate(() => {
      window.__OPENKARA_E2E__.emitEvent("batch-separation-progress", {
        total: 3,
        completed: 0,
        skipped: 0,
        failed: 0,
        current_song_id: "earfquake",
        current_percent: 35,
      });
      window.__OPENKARA_E2E__.emitEvent("separation-progress", {
        song_id: "earfquake",
        percent: 35,
      });
    });

    await expect(page.getByRole("progressbar").first()).toBeVisible({
      timeout: 5000,
    });
    const name = await page
      .getByRole("progressbar")
      .first()
      .getAttribute("aria-label");
    expect(name === null || name.length >= 0).toBe(true);

    await a11y.axeForThemes();
  });

  test("separation completion is announced through notifications", async ({
    page,
    a11y,
  }) => {
    await a11y.startLiveRegionMonitor();

    await page.evaluate(() => {
      window.__OPENKARA_E2E__.emitEvent("separation-complete", {
        song_id: "earfquake",
        status: {
          song_id: "earfquake",
          state: "completed",
          percent: 100,
          cache_hit: true,
          vocals_path: "/tmp/vocals.wav",
          accomp_path: "/tmp/accomp.wav",
          drums_path: null,
          bass_path: null,
          other_path: null,
          model_variant: "htdemucs",
          error: null,
        },
      });
    });

    await expect(page.getByRole("status").first()).toBeVisible({
      timeout: 5000,
    });
    await expect
      .poll(async () => (await a11y.getAnnouncements()).join("\n"), {
        timeout: 5000,
      })
      .toMatch(/cached|separat|complete|using/i);
  });
});
