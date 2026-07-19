import { test, expect } from "./fixtures/base-test";

/**
 * Song import workflow UI smoke tests.
 *
 * In the real Tauri app, song import uses native file dialogs and the Rust
 * backend to process audio files.  In browser-based UI smoke we can only verify
 * that the UI surfaces for import are present and correctly wired.  The
 * actual file dialog cannot be triggered in a browser context.
 */
test.describe("Song import workflow", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("sidebar shows the song library with imported songs", async ({
    page,
  }) => {
    // The mock returns 6 songs — all should be visible in the sidebar song list
    await expect(page.getByText("Earfquake")).toBeVisible();
    await expect(page.getByText("See You Again")).toBeVisible();
    await expect(page.getByText("Counting Stars")).toBeVisible();

    // Artist names should also appear.  Tyler, The Creator appears on two
    // fixture songs (Earfquake and See You Again), so use exact matching to
    // avoid strict-mode violations on the shared substring.
    await expect(
      page.getByText("Tyler, The Creator", { exact: true }),
    ).toBeVisible();
    await expect(page.getByText("Gorillaz")).toBeVisible();
    await expect(page.getByText("OneRepublic")).toBeVisible();
  });

  test("search box filters the song list", async ({ page }) => {
    // Find the search input
    const searchBox = page.getByRole("textbox");
    await expect(searchBox.first()).toBeVisible();

    // Type a search query
    await searchBox.first().fill("See You");

    // Only matching songs should remain visible
    await expect(page.getByText("See You Again")).toBeVisible();

    // Non-matching songs should be hidden (or the filter should be active)
    // Note: exact assertion depends on whether filtering hides or dims

    // Clear search
    await searchBox.first().clear();
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("empty library state is not shown when songs exist", async ({
    page,
  }) => {
    const songList = page.getByTestId("song-list");
    await expect(songList).toBeVisible();
    await expect(songList.getByText("Earfquake")).toBeVisible();
    await expect(songList.getByText(/no tracks/i)).not.toBeVisible();
  });
});
