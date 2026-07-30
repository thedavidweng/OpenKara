import { expect, test } from "./fixtures/accessibility-test";

test.describe("Themes and motion accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
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

  test.fixme("dark and light themes pass axe in default and reduced-motion states", async ({
    a11y,
  }) => {
    test.fixme("TODO: implement theme/motion axe matrix");
    await a11y.setTheme("dark");
    await a11y.setReducedMotion(true);
    await a11y.axeCheck();
    await a11y.setTheme("light");
    await a11y.setReducedMotion(false);
    await a11y.axeCheck();
  });

  test.fixme("forced colors and 200% zoom keep controls focusable and labeled", async ({
    a11y,
  }) => {
    test.fixme("TODO: implement forced-colors and zoom checks");
    await a11y.setForcedColors(true);
    await a11y.setZoom(2);
    await a11y.axeCheck();
  });
});
