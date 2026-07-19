import { test, expect } from "./fixtures/base-test";

/**
 * Alphabet rail e2e smoke tests.
 *
 * Verifies the rail is visible in title_asc/artist_asc modes, hidden in
 * recently_imported, and that clicking a letter scrolls the song list.
 */
test.describe("Alphabet rail", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("song-list")).toBeVisible();
  });

  test("rail is hidden in recently_imported mode", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("recently_imported");
    await expect(
      page.getByRole("navigation", { name: "Alphabet navigation" }),
    ).toBeHidden();
  });

  test("rail is visible in title_asc mode", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");
    await expect(
      page.getByRole("navigation", { name: "Alphabet navigation" }),
    ).toBeVisible();
  });

  test("rail is visible in artist_asc mode", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("artist_asc");
    await expect(
      page.getByRole("navigation", { name: "Alphabet navigation" }),
    ).toBeVisible();
  });

  test("clicking a letter button marks the resolved section", async ({
    page,
  }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    // The short default fixture fits fully in the viewport, so a C jump does
    // not produce a measurable scroll offset. Verify the visible resolved
    // section marker here; the 5,000-song spec covers actual scroll movement.
    // The fixture's title_asc buckets are A, C, E, F, S, T.
    const buttonC = rail.locator("button[data-bucket='C']");
    await buttonC.click();
    await expect(buttonC).toHaveAttribute("aria-current", "true");
  });

  test("keyboard typeahead on a missing letter falls forward to the next mapped bucket", async ({
    page,
  }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    // Focus a button in the rail and type "b" — not mapped in this fixture
    // (title_asc buckets are A, C, E, F, S, T), should fall forward to C
    // (the nearest mapped bucket after B).
    const firstButton = rail.locator("button").first();
    await firstButton.focus();

    // Type "b" on the focused button — the keydown handler is on the container
    // and bubbles up.
    await page.keyboard.press("b");

    // The aria-current button should be the resolved bucket (C)
    const activeButton = rail.locator("button[aria-current='true']");
    await expect(activeButton).toHaveAttribute("data-bucket", "C");
  });
});
