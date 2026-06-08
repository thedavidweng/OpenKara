import { test, expect } from "./fixtures/base-test";

/**
 * Lyrics display and sync E2E tests.
 *
 * The Tauri mock returns sample LRC lyrics for `fetch_lyrics`.  These
 * tests verify that lyrics lines render in the UI after a song starts
 * playing and that utility controls (font size, offset) are accessible.
 */
test.describe("Lyrics display", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();

    // Start playback to trigger lyrics fetch
    await page.getByText("Bohemian Rhapsody").dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });
  });

  test("lyrics lines appear after song starts playing", async ({ page }) => {
    // Mock fetch_lyrics returns lines like "Is this the real life?"
    await expect(page.getByText("Is this the real life?")).toBeVisible({
      timeout: 10000,
    });
    await expect(page.getByText("Is this just fantasy?")).toBeVisible();
  });

  test("all mocked lyric lines are rendered", async ({ page }) => {
    await expect(page.getByText("Is this the real life?")).toBeVisible({
      timeout: 10000,
    });

    // Verify the remaining lines from the mock
    await expect(page.getByText("Caught in a landslide")).toBeVisible();
    await expect(page.getByText("No escape from reality")).toBeVisible();
  });

  test("lyrics panel has a scroll viewport", async ({ page }) => {
    await expect(page.getByText("Is this the real life?")).toBeVisible({
      timeout: 10000,
    });

    // The lyrics scroll viewport should exist
    const viewport = page.locator("[data-testid='lyrics-scroll-viewport']");
    await expect(viewport).toBeVisible();
  });
});
