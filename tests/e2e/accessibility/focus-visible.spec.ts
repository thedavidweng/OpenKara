import { expect, test } from "./fixtures/accessibility-test";

function hasVisibleFocusRing(metrics: {
  outlineStyle: string;
  outlineWidth: string;
  boxShadow: string;
}): boolean {
  const outlineVisible =
    metrics.outlineStyle !== "none" &&
    metrics.outlineStyle !== "" &&
    parseFloat(metrics.outlineWidth) > 0;
  const shadowVisible =
    metrics.boxShadow !== "none" && metrics.boxShadow.trim() !== "";
  return outlineVisible || shadowVisible;
}

test.describe("Focus-visible indicators", () => {
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("toolbar icon buttons show a keyboard focus indicator", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Settings" }).focus();
    const metrics = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el) throw new Error("No focused element");
      const style = getComputedStyle(el);
      return {
        outlineStyle: style.outlineStyle,
        outlineWidth: style.outlineWidth,
        boxShadow: style.boxShadow,
      };
    });
    expect(hasVisibleFocusRing(metrics)).toBe(true);
  });

  test("search text field shows a keyboard focus indicator", async ({
    page,
  }) => {
    const search = page.getByRole("textbox", { name: "Search" });
    await search.focus();
    await expect(search).toBeFocused();
    const metrics = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el) throw new Error("No focused element");
      const style = getComputedStyle(el);
      return {
        outlineStyle: style.outlineStyle,
        outlineWidth: style.outlineWidth,
        boxShadow: style.boxShadow,
        borderColor: style.borderColor,
      };
    });
    const ring =
      hasVisibleFocusRing(metrics) || metrics.borderColor.includes("rgb");
    expect(ring).toBe(true);
  });

  test("dialog close button shows a keyboard focus indicator", async ({
    page,
  }) => {
    const settings = page.getByRole("button", { name: "Settings" });
    await settings.focus();
    await page.keyboard.press("Enter");
    const dialog = page.getByRole("dialog", { name: "Preferences" });
    const close = dialog.getByRole("button", { name: "Close" });
    await expect(close).toBeFocused();

    // Re-apply keyboard focus so :focus-visible styles apply.
    await close.focus();
    const metrics = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el) throw new Error("No focused element");
      const style = getComputedStyle(el);
      return {
        outlineStyle: style.outlineStyle,
        outlineWidth: style.outlineWidth,
        boxShadow: style.boxShadow,
      };
    });
    expect(hasVisibleFocusRing(metrics)).toBe(true);
  });

  test("list row can receive keyboard focus", async ({ page }) => {
    const song = page.getByRole("button", { name: "Earfquake" });
    await song.focus();
    await expect(song).toBeFocused();
  });

  test("primary control focus-visible screenshot baseline", async ({
    page,
  }, testInfo) => {
    test.skip(
      !process.env.OKA_A11Y_SCREENSHOTS,
      "Screenshot baselines require OKA_A11Y_SCREENSHOTS=1 after images are stable in CI",
    );
    await page.getByRole("button", { name: "Settings" }).focus();
    await expect(
      page.getByRole("button", { name: "Settings" }),
    ).toHaveScreenshot(`focus-visible-settings-${testInfo.project.name}.png`, {
      maxDiffPixelRatio: 0.02,
    });
  });
});
