import { expect, test } from "./fixtures/accessibility-test";

test.describe("Error handling accessibility", () => {
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("app loads without an error boundary fallback", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "Something went wrong" }),
    ).toHaveCount(0);
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("error toasts use role alert and include retry actions", async ({
    page,
    a11y,
  }) => {
    await a11y.startLiveRegionMonitor();

    await page.evaluate(() => {
      window.__OPENKARA_E2E__.emitEvent("playback-error", {
        song_id: "earfquake",
        error: {
          code: "audio_decode_failed",
          message: "Could not decode audio fixture",
          retryable: true,
          fallback: "retry",
        },
      });
    });

    const alert = page.getByRole("alert").filter({
      hasText: /Could not decode audio fixture/i,
    });
    await expect(alert.first()).toBeVisible({ timeout: 5000 });
    await expect(
      page.getByRole("button", { name: /try again|retry/i }),
    ).toBeVisible();
    await expect(
      alert.first().getByRole("button", { name: "Close" }),
    ).toBeVisible();

    await expect
      .poll(async () => (await a11y.getAnnouncements()).join("\n"))
      .toMatch(/Could not decode audio fixture/i);

    await a11y.disableTransitions();
    await a11y.setTheme("dark");
    await a11y.axeCheck();
  });

  test("shell remains axe-clean after an error toast", async ({
    page,
    a11y,
  }) => {
    await page.evaluate(() => {
      window.__OPENKARA_E2E__.emitEvent("playback-error", {
        song_id: "earfquake",
        error: {
          code: "internal",
          message: "Synthetic accessibility error",
          retryable: false,
          fallback: "keep_current_state",
        },
      });
    });

    await expect(page.getByRole("alert").first()).toBeVisible({
      timeout: 5000,
    });
    await a11y.axeForThemes();
  });
});
