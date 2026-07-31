import { expect, test } from "./fixtures/accessibility-test";

test.describe("App shell accessibility", () => {
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
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

  test("app shell has no axe violations in dark and light themes", async ({
    a11y,
  }) => {
    await a11y.axeForThemes();
  });

  test("window chrome and drag region do not trap keyboard focus", async ({
    page,
  }) => {
    const toggle = page.getByRole("button", { name: "Toggle Sidebar" });
    await toggle.focus();
    await expect(toggle).toBeFocused();

    await page.keyboard.press("Tab");
    const afterFirstTab = await page.evaluate(() => {
      const active = document.activeElement;
      return {
        tag: active?.tagName ?? null,
        name:
          active?.getAttribute("aria-label") ??
          (active as HTMLElement | null)?.innerText?.trim().slice(0, 80) ??
          null,
        isBody: active === document.body,
      };
    });
    expect(afterFirstTab.isBody).toBe(false);
    expect(afterFirstTab.tag).not.toBeNull();

    await page.keyboard.press("Tab");
    await page.keyboard.press("Tab");
    const stillInApp = await page.evaluate(() => {
      const active = document.activeElement;
      return Boolean(
        active &&
        active !== document.body &&
        document.documentElement.contains(active),
      );
    });
    expect(stillInApp).toBe(true);
  });
});
