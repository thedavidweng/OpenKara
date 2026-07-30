import { expect, test } from "./fixtures/accessibility-test";

test.describe("App shell accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
  });

  test("toolbar controls have accessible names and the sidebar is present", async ({
    page,
  }) => {
    await expect(
      page.getByRole("button", { name: "Toggle Sidebar" }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Import" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Settings" })).toBeVisible();
    await expect(
      page.locator("[data-window-shell-section='sidebar']"),
    ).toBeVisible();
  });

  test.fixme("app shell has no axe violations in dark and light themes", async ({
    a11y,
  }) => {
    test.fixme("TODO: implement axe scans for dark and light themes");
    await a11y.setTheme("dark");
    await a11y.axeCheck();
    await a11y.setTheme("light");
    await a11y.axeCheck();
  });

  test.fixme("window chrome and drag region do not trap keyboard focus", async ({
    page,
  }) => {
    test.fixme("TODO: implement focus-order check for window chrome");
    await page.getByRole("button", { name: "Toggle Sidebar" }).focus();
  });
});
