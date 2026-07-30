import { expect, test } from "./fixtures/accessibility-test";

test.describe("Focus order", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
  });

  test("keyboard focus can be placed on the main toolbar controls", async ({
    page,
  }) => {
    const toggle = page.getByRole("button", { name: "Toggle Sidebar" });
    await toggle.focus();
    await expect(toggle).toBeFocused();
  });

  test.fixme("Tab order follows the visual order through the app shell", async ({
    page,
  }) => {
    test.fixme("TODO: implement full tab-order check");
    await page.getByRole("button", { name: "Toggle Sidebar" }).focus();
    await page.keyboard.press("Tab");
  });

  test.fixme("settings dialog traps focus until it is closed", async ({
    page,
  }) => {
    test.fixme("TODO: implement settings focus trap check");
    await page.getByRole("button", { name: "Settings" }).click();
    await page.keyboard.press("Tab");
    await page.keyboard.press("Tab");
  });

  test.fixme("closing a dialog or panel restores focus to the triggering control", async ({
    page,
  }) => {
    test.fixme("TODO: implement focus restoration check");
    const settingsButton = page.getByRole("button", { name: "Settings" });
    await settingsButton.click();
    await page.keyboard.press("Escape");
    await expect(settingsButton).toBeFocused();
  });
});
