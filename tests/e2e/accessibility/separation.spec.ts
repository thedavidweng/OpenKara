import { expect, test } from "./fixtures/accessibility-test";

test.describe("Separation accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
  });

  test("separation controls are labeled and reachable", async ({ page }) => {
    await expect(
      page.getByRole("button", { name: /separate all/i }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Separated" })).toBeVisible();
  });

  test.fixme("running separation is exposed as a progressbar with an accessible name", async ({
    page,
    a11y,
  }) => {
    test.fixme("TODO: implement progressbar label check");
    await a11y.startLiveRegionMonitor();
    await page.getByRole("button", { name: /separate all/i }).click();
    await expect(page.getByRole("progressbar")).toBeVisible();
  });

  test.fixme("separation completion and errors are announced to screen readers", async ({
    a11y,
  }) => {
    test.fixme("TODO: implement live-region announcement checks");
    await a11y.startLiveRegionMonitor();
  });
});
