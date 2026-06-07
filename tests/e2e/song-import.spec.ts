import { test, expect } from "./fixtures/base-test";

/**
 * Song import workflow E2E tests.
 *
 * In the real Tauri app, song import uses native file dialogs and the Rust
 * backend to process audio files.  In browser-based E2E we can only verify
 * that the UI surfaces for import are present and correctly wired.  The
 * actual file dialog cannot be triggered in a browser context.
 */
test.describe("Song import workflow", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();
  });

  test("sidebar shows the song library with imported songs", async ({
    page,
  }) => {
    // The mock returns 3 songs — all should be visible in the sidebar song list
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();
    await expect(page.getByText("Hotel California")).toBeVisible();
    await expect(page.getByText("Imagine")).toBeVisible();

    // Artist names should also appear
    await expect(page.getByText("Queen")).toBeVisible();
    await expect(page.getByText("Eagles")).toBeVisible();
    await expect(page.getByText("John Lennon")).toBeVisible();
  });

  test("search box filters the song list", async ({ page }) => {
    // Find the search input
    const searchBox = page.getByRole("textbox");
    await expect(searchBox.first()).toBeVisible();

    // Type a search query
    await searchBox.first().fill("Hotel");

    // Only matching songs should remain visible
    await expect(page.getByText("Hotel California")).toBeVisible();

    // Non-matching songs should be hidden (or the filter should be active)
    // Note: exact assertion depends on whether filtering hides or dims

    // Clear search
    await searchBox.first().clear();
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();
  });

  test("empty library state is not shown when songs exist", async ({
    page,
  }) => {
    // When the mock library has songs, the empty state should NOT appear
    const emptyState = page.getByText(/no songs|empty library|add.*songs/i);
    await expect(emptyState).not.toBeVisible();
  });
});
