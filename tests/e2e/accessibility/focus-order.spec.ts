import { expect, test } from "./fixtures/accessibility-test";

test.describe("Focus order", () => {
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("keyboard focus can be placed on the main toolbar controls", async ({
    page,
  }) => {
    const toggle = page.getByRole("button", { name: "Toggle Sidebar" });
    await toggle.focus();
    await expect(toggle).toBeFocused();

    const settings = page.getByRole("button", { name: "Settings" });
    await settings.focus();
    await expect(settings).toBeFocused();
  });

  test("Tab order moves through the app shell without trapping", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Toggle Sidebar" }).focus();

    const names: string[] = [];
    for (let i = 0; i < 8; i++) {
      await page.keyboard.press("Tab");
      const name = await page.evaluate(() => {
        const active = document.activeElement as HTMLElement | null;
        if (!active || active === document.body) return "";
        return (
          active.getAttribute("aria-label") ??
          active.getAttribute("title") ??
          active.innerText?.trim().split("\n")[0] ??
          active.tagName
        );
      });
      if (name) names.push(name);
    }

    expect(names.length).toBeGreaterThan(3);
    expect(new Set(names).size).toBeGreaterThan(1);
  });

  test("settings dialog traps focus until it is closed", async ({ page }) => {
    const settingsButton = page.getByRole("button", { name: "Settings" });
    await settingsButton.focus();
    await page.keyboard.press("Enter");

    const dialog = page.getByRole("dialog", { name: "Preferences" });
    const closeButton = dialog.getByRole("button", { name: "Close" });
    await expect(dialog).toBeVisible();
    await expect(closeButton).toBeFocused();

    await page.keyboard.press("Tab");
    const afterTab = await page.evaluate(() => {
      const dialogEl = document.querySelector('[role="dialog"]');
      const active = document.activeElement;
      return Boolean(dialogEl && active && dialogEl.contains(active));
    });
    expect(afterTab).toBe(true);

    // Walk forward through several focusable controls; focus must stay modal.
    for (let i = 0; i < 12; i++) {
      await page.keyboard.press("Tab");
    }
    const afterCycle = await page.evaluate(() => {
      const dialogEl = document.querySelector('[role="dialog"]');
      const active = document.activeElement;
      return Boolean(dialogEl && active && dialogEl.contains(active));
    });
    expect(afterCycle).toBe(true);

    // From the initial close control, reverse tab must wrap inside the dialog.
    await closeButton.focus();
    await page.keyboard.press("Shift+Tab");
    const afterShiftTab = await page.evaluate(() => {
      const dialogEl = document.querySelector('[role="dialog"]');
      const active = document.activeElement;
      return Boolean(dialogEl && active && dialogEl.contains(active));
    });
    expect(afterShiftTab).toBe(true);
  });

  test("closing a dialog restores focus to the triggering control", async ({
    page,
  }) => {
    const settingsButton = page.getByRole("button", { name: "Settings" });
    await settingsButton.focus();
    await page.keyboard.press("Enter");

    const dialog = page.getByRole("dialog", { name: "Preferences" });
    await expect(dialog).toBeVisible();

    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(settingsButton).toBeFocused();
  });
});
