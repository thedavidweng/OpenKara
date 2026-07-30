import { expect, test } from "./fixtures/accessibility-test";

test.describe("Error handling accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
  });

  test("app loads without an error boundary fallback", async ({ page }) => {
    await expect(
      page.getByRole("heading", { name: "Something went wrong" }),
    ).toHaveCount(0);
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test.fixme("error toasts use role alert and include retry actions", async ({
    page,
    a11y,
  }) => {
    test.fixme("TODO: implement error toast accessibility checks");
    await a11y.startLiveRegionMonitor();
    await expect(page.getByRole("alert")).toBeVisible();
  });

  test.fixme("error boundary fallback has a focusable reload control", async ({
    page,
  }) => {
    test.fixme("TODO: implement error boundary focus check");
    await expect(page.getByRole("button", { name: "Reload" })).toBeVisible();
  });
});
