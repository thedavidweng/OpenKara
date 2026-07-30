import { expect, test } from "./fixtures/accessibility-test";

test.describe("Settings accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
  });

  test("settings opens as a modal dialog and the close button is focused", async ({
    page,
  }) => {
    const settingsButton = page.getByRole("button", { name: "Settings" });
    await settingsButton.focus();
    await page.keyboard.press("Enter");

    const dialog = page.getByRole("dialog", { name: "Preferences" });
    const closeButton = dialog.getByRole("button", { name: "Close" });
    await expect(dialog).toBeVisible();
    await expect(closeButton).toBeFocused();
  });

  test.fixme("settings dialog traps focus and restores it on close", async ({
    page,
  }) => {
    test.fixme("TODO: implement focus trap and restoration checks");
    await page.keyboard.press("Escape");
  });

  test.fixme("settings sections have no axe violations and correct heading structure", async ({
    page,
    a11y,
  }) => {
    test.fixme("TODO: implement section heading and axe checks");
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByText("Karaoke Library")).toBeVisible();
    await a11y.axeCheck();
  });

  test.fixme("form controls in settings have associated labels", async ({
    page,
  }) => {
    test.fixme("TODO: implement form label checks");
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByText("Appearance")).toBeVisible();
  });
});
