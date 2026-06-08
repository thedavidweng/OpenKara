import { test, expect } from "./fixtures/base-test";

/**
 * Playback controls E2E tests.
 *
 * Verifies play/pause, skip, and seek bar interactions against the
 * mocked Tauri backend.  The mock returns deterministic playback state
 * snapshots so we can assert UI transitions.
 */
test.describe("Playback controls", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();
  });

  test("double-clicking a song starts playback", async ({ page }) => {
    await page.getByText("Bohemian Rhapsody").dblclick();

    // The play button should become a pause button
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });
  });

  test("pause button stops playback", async ({ page }) => {
    // Start playback
    await page.getByText("Bohemian Rhapsody").dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });

    // Pause
    await page.getByRole("button", { name: /pause/i }).click();

    // Should show play button again
    await expect(page.getByRole("button", { name: /play/i })).toBeVisible({
      timeout: 5000,
    });
  });

  test("skip forward and back buttons exist and are clickable", async ({
    page,
  }) => {
    await page.getByText("Bohemian Rhapsody").dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });

    // Skip forward
    const skipForward = page.getByRole("button", { name: /next/i });
    await expect(skipForward).toBeVisible();
    await skipForward.click();

    // Skip back
    const skipBack = page.getByRole("button", { name: /previous/i });
    await expect(skipBack).toBeVisible();
    await skipBack.click();
  });

  test("seek bar is visible during playback", async ({ page }) => {
    await page.getByText("Bohemian Rhapsody").dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });

    // The seek bar should be rendered as a slider
    const seekBar = page.getByRole("slider", { name: /seek/i });
    await expect(seekBar).toBeVisible();
  });

  test("now-playing info shows song title during playback", async ({
    page,
  }) => {
    await page.getByText("Bohemian Rhapsody").dblclick();

    // The playback bar area should show the currently playing song title
    // There may be multiple instances (sidebar + now-playing), so use .first()
    await expect(page.getByText("Bohemian Rhapsody").first()).toBeVisible();
  });
});
