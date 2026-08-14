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
    // The mock catalog songs should be visible in the sidebar song list
    await expect(page.getByText("Earfquake")).toBeVisible();
    await expect(page.getByText("One Last Kiss")).toBeVisible();
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
    const searchBox = page.getByRole("textbox", { name: /search/i });
    const songList = page.getByTestId("song-list");
    await expect(searchBox).toBeVisible();

    await searchBox.fill("See You");
    // Wait for the store's 300ms debounce to apply before asserting filter.
    await expect(songList.getByText("Earfquake")).not.toBeVisible();
    await expect(songList.getByText("See You Again")).toBeVisible();

    await searchBox.clear();
    await expect(songList.getByText("Earfquake")).toBeVisible();
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
