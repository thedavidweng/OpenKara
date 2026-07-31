import {
  ACCESSIBILITY_MATRIX,
  expect,
  test,
} from "./fixtures/accessibility-test";

test.describe("Themes and motion accessibility", () => {
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("theme and motion helpers apply to the document", async ({
    page,
    a11y,
  }) => {
    await a11y.setTheme("dark");
    const theme = await page.evaluate(
      () => document.documentElement.dataset.theme,
    );
    expect(theme).toBe("dark");

    await a11y.setReducedMotion(true);
    const reducedMotion = await page.evaluate(
      () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
    );
    expect(reducedMotion).toBe(true);

    await a11y.setForcedColors(true);
    const forcedColors = await page.evaluate(
      () => window.matchMedia("(forced-colors: active)").matches,
    );
    expect(forcedColors).toBe(true);

    await a11y.setZoom(1.5);
    const zoom = await page.evaluate(
      () => document.documentElement.dataset.a11yZoom,
    );
    expect(zoom).toBe("1.5");
  });

  for (const theme of ["dark", "light"] as const) {
    test(`${theme} theme passes axe with default motion`, async ({ a11y }) => {
      await a11y.disableTransitions();
      await a11y.setReducedMotion(false);
      await a11y.setTheme(theme);
      await a11y.axeCheck();
    });

    test(`${theme} theme passes axe with reduced motion`, async ({ a11y }) => {
      await a11y.disableTransitions();
      await a11y.setReducedMotion(true);
      await a11y.setTheme(theme);
      await a11y.axeCheck();
    });
  }

  for (const theme of ["dark", "light"] as const) {
    test(`${theme} theme remains operable at 200% zoom`, async ({
      page,
      a11y,
    }) => {
      await a11y.disableTransitions();
      await a11y.setTheme(theme);
      await a11y.setZoom(2);

      await expect(
        page.getByRole("button", { name: "Settings" }),
      ).toBeVisible();
      await expect(page.getByRole("textbox", { name: "Search" })).toBeVisible();
      await a11y.axeCheck();
    });
  }

  test("200% zoom keeps toolbar controls focusable and labeled", async ({
    page,
    a11y,
  }) => {
    await a11y.disableTransitions();
    await a11y.setTheme("dark");
    await a11y.setZoom(2);

    const settings = page.getByRole("button", { name: "Settings" });
    await settings.focus();
    await expect(settings).toBeFocused();
    await expect(settings).toHaveAccessibleName("Settings");
  });

  if (ACCESSIBILITY_MATRIX) {
    for (const theme of ["dark", "light"] as const) {
      test(`${theme} theme with forced colors passes axe`, async ({ a11y }) => {
        await a11y.disableTransitions();
        await a11y.setTheme(theme);
        await a11y.setForcedColors(true);
        await a11y.axeCheck();
      });

      test(`${theme} theme at 400% zoom keeps reflow shell operable`, async ({
        page,
        a11y,
      }) => {
        await a11y.disableTransitions();
        await a11y.setTheme(theme);
        await a11y.setZoom(4);

        await expect(
          page.getByRole("button", { name: "Settings" }),
        ).toBeVisible();
        await page.getByRole("button", { name: "Settings" }).click();
        await expect(
          page.getByRole("dialog", { name: "Preferences" }),
        ).toBeVisible();
        await a11y.axeCheck();
      });
    }

    test("simplified Chinese locale keeps the shell labeled for axe", async ({
      page,
      a11y,
    }) => {
      await page.getByRole("button", { name: "Settings" }).click();
      const dialog = page.getByRole("dialog", { name: "Preferences" });
      await expect(dialog).toBeVisible();

      const language = dialog.locator("select").filter({
        has: page.locator("option[value='zh-CN']"),
      });
      await language.selectOption("zh-CN");
      await page.keyboard.press("Escape");

      await expect(
        page
          .getByRole("button", { name: "设置" })
          .or(page.getByRole("button", { name: "Settings" })),
      ).toBeVisible({ timeout: 5000 });

      await a11y.disableTransitions();
      await a11y.setTheme("dark");
      await a11y.axeCheck();
      await a11y.setTheme("light");
      await a11y.axeCheck();
    });
  }
});
