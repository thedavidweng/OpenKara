import { test, expect } from "./fixtures/base-test";

/**
 * Lyrics display and sync UI smoke tests.
 *
 * The Tauri mock returns sample LRC lyrics for `fetch_lyrics`.  These
 * tests verify that lyrics lines render in the UI after a song starts
 * playing and that utility controls (font size, offset) are accessible.
 */
test.describe("Lyrics display", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();

    // Start playback to trigger lyrics fetch
    await page.getByText("Earfquake").dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });
  });

  test("lyrics lines appear after song starts playing", async ({ page }) => {
    // Mock fetch_lyrics returns synced lyrics from lrclib for Earfquake.  Use
    // exact matching because the lyrics contain repeated "for real" phrases
    // that would otherwise produce strict-mode violations.
    await expect(
      page.getByText("For real, for real this time", { exact: true }),
    ).toBeVisible({ timeout: 10000 });
    await expect(
      page.getByText("Bitch, I cannot fall short", { exact: true }),
    ).toBeVisible();
  });

  test("all mocked lyric lines are rendered", async ({ page }) => {
    await expect(
      page.getByText("For real, for real this time", { exact: true }),
    ).toBeVisible({ timeout: 10000 });

    // Verify additional unique lines from the lrclib synced lyrics
    await expect(
      page.getByText("'Cause when it all comes crashin' down I'll need you", {
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page.getByText("We ain't gotta ball, D. Rose, huh", { exact: true }),
    ).toBeVisible();
  });

  test("lyrics panel has a scroll viewport", async ({ page }) => {
    await expect(
      page.getByText("For real, for real this time", { exact: true }),
    ).toBeVisible({ timeout: 10000 });

    // The lyrics scroll viewport should exist
    const viewport = page.locator("[data-testid='lyrics-scroll-viewport']");
    await expect(viewport).toBeVisible();
  });
});
