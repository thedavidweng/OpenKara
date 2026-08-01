import { expect, test } from "./fixtures/accessibility-test";

test.describe("Library accessibility", () => {
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("library controls have accessible names and the song list is reachable", async ({
    page,
  }) => {
    await expect(page.getByRole("textbox", { name: "Search" })).toBeVisible();
    await expect(page.getByRole("combobox", { name: "Sort by" })).toBeVisible();
    await expect(page.getByTestId("song-list")).toBeVisible();
    await expect(page.getByRole("button", { name: "Earfquake" })).toBeVisible();
  });

  test("virtualized song list rows are keyboard focusable", async ({
    page,
  }) => {
    const song = page.getByRole("button", { name: "Earfquake" });
    await song.focus();
    await expect(song).toBeFocused();

    await page.keyboard.press("ArrowDown");
    const focusedName = await page.evaluate(() => {
      const active = document.activeElement;
      return (
        active?.getAttribute("aria-label") ??
        (active as HTMLElement | null)?.innerText?.trim().split("\n")[0] ??
        null
      );
    });
    expect(focusedName).toBeTruthy();
  });

  test("alphabet rail has correct labels and does not break focus order", async ({
    page,
  }) => {
    await page.getByTestId("sort-mode-selector").selectOption("title_asc");
    const rail = page.getByRole("navigation", { name: /alphabet/i });
    await expect(rail).toBeVisible();

    const jumpButtons = page.getByRole("button", { name: /Jump to/i });
    await expect(jumpButtons.first()).toBeVisible();
    await jumpButtons.first().focus();
    await expect(jumpButtons.first()).toBeFocused();

    await page.keyboard.press("Tab");
    const stillInDocument = await page.evaluate(
      () =>
        document.activeElement != null &&
        document.activeElement !== document.body,
    );
    expect(stillInDocument).toBe(true);
  });

  test("library has no axe violations after searching and filtering", async ({
    page,
    a11y,
  }) => {
    await page.getByRole("textbox", { name: "Search" }).fill("Earf");
    await expect(page.getByRole("button", { name: "Earfquake" })).toBeVisible();
    await a11y.axeForThemes();
  });
});
