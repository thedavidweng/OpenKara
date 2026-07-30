import { expect, test } from "./fixtures/accessibility-test";

test.describe("Live regions", () => {
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

  test.fixme("model and runtime banners use polite and assertive live regions", async ({
    a11y,
  }) => {
    test.fixme("TODO: implement banner live-region checks");
    await a11y.startLiveRegionMonitor();
  });

  test.fixme("queue reordering announcements are spoken in the correct order", async ({
    page,
    a11y,
  }) => {
    test.fixme("TODO: implement queue announcement checks");
    await a11y.startLiveRegionMonitor();
    await page.getByRole("button", { name: "Queue" }).click();
  });

  test.fixme("toast notifications are announced and can be dismissed", async ({
    page,
    a11y,
  }) => {
    test.fixme("TODO: implement toast live-region checks");
    await a11y.startLiveRegionMonitor();
    await expect(page.getByRole("button", { name: "Close" })).toBeVisible();
  });
});
