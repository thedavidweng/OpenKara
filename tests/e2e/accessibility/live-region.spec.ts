import { expect, test } from "./fixtures/accessibility-test";

test.describe("Live regions", () => {
  test.describe.configure({ retries: 0 });

  test("live region monitor captures polite announcements", async ({
    page,
    a11y,
  }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");

    await a11y.startLiveRegionMonitor();
    await page.evaluate(() => {
      const region = document.createElement("div");
      region.setAttribute("aria-live", "polite");
      region.setAttribute("id", "a11y-test-live-region");
      document.body.appendChild(region);
      window.setTimeout(() => {
        region.textContent = "Test announcement captured";
      }, 50);
    });

    await expect
      .poll(async () => (await a11y.getAnnouncements()).join("\n"))
      .toContain("Test announcement captured");
  });

  test("model bootstrap banners use polite and assertive live regions", async ({
    page,
    a11y,
  }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
    await a11y.startLiveRegionMonitor();

    await page.evaluate(() => {
      window.__OPENKARA_E2E__.emitEvent("model-bootstrap-progress", {
        state: "downloading",
        model_path: "",
        downloaded_bytes: 1024,
        total_bytes: 4096,
        error: null,
        variant: "htdemucs",
      });
    });

    const politeBanner = page.getByRole("status").filter({
      hasText: /download|model/i,
    });
    await expect(politeBanner.first()).toBeVisible({ timeout: 5000 });
    await expect(politeBanner.first()).toHaveAttribute("aria-live", "polite");

    await page.evaluate(() => {
      window.__OPENKARA_E2E__.emitEvent("model-bootstrap-error", {
        state: "failed",
        model_path: "",
        downloaded_bytes: null,
        total_bytes: null,
        error: {
          code: "network_unavailable",
          message: "Model download interrupted",
          retryable: true,
          fallback: "retry",
        },
        variant: "htdemucs",
      });
    });

    const alertBanner = page.getByRole("alert").filter({
      hasText: /failed|download|model/i,
    });
    await expect(alertBanner.first()).toBeVisible({ timeout: 5000 });
    await expect(alertBanner.first()).toHaveAttribute("aria-live", "assertive");

    await expect
      .poll(async () => (await a11y.getAnnouncements()).join("\n"))
      .toMatch(/download|model|failed/i);
  });

  test("toast notifications are announced and can be dismissed", async ({
    page,
    a11y,
  }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
    await a11y.startLiveRegionMonitor();

    await page.evaluate(() => {
      window.__OPENKARA_E2E__.emitEvent("playback-error", {
        song_id: "earfquake",
        error: {
          code: "song_not_found",
          message: "Track missing from library",
          retryable: false,
          fallback: "refresh_library",
        },
      });
    });

    const toast = page.getByRole("alert").filter({
      hasText: /Track missing from library/i,
    });
    await expect(toast.first()).toBeVisible({ timeout: 5000 });

    await expect
      .poll(async () => (await a11y.getAnnouncements()).join("\n"))
      .toMatch(/Track missing from library/i);

    await toast.first().getByRole("button", { name: "Close" }).click();
    await expect(toast).toHaveCount(0);
  });

  test("queue panel open does not clear live-region monitoring", async ({
    page,
    a11y,
  }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
    await a11y.startLiveRegionMonitor();
    await page.getByRole("button", { name: "Queue" }).click();
    await expect(page.getByTestId("queue-panel")).toBeVisible();

    await page.evaluate(() => {
      const region = document.createElement("div");
      region.setAttribute("role", "status");
      region.setAttribute("aria-live", "polite");
      region.textContent = "Queue status update";
      document.body.appendChild(region);
    });

    await expect
      .poll(async () => (await a11y.getAnnouncements()).join("\n"))
      .toContain("Queue status update");
  });
});
