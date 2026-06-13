import { test, expect } from "./fixtures/base-test";

/**
 * Smoke test — verifies the critical happy path through OpenKara:
 *
 *  1. App loads and shows the main layout (library is set up)
 *  2. Sidebar displays the song library
 *  3. A song can be selected from the library
 *  4. Playback controls are visible and interactive
 *  5. Queue panel can be toggled
 *  6. Lyrics panel shows content for the active song
 */
test.describe("Smoke: happy path", () => {
  test("app loads with library and full layout", async ({ page }) => {
    await page.goto("/");

    // The main app layout should render (sidebar + main content)
    await expect(
      page.locator("[data-window-shell-section='sidebar']"),
    ).toBeVisible();

    // Sidebar should show the song list
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();
    await expect(page.getByText("Hotel California")).toBeVisible();
    await expect(page.getByText("Imagine")).toBeVisible();
  });

  test("selecting a song shows it in the playback area", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();

    // Double-click a song to play it (or single click then play button)
    await page.getByText("Bohemian Rhapsody").dblclick();

    // The play button should now show a pause icon (song is playing)
    // Look for the play/pause toggle button by its aria-label
    const pauseButton = page.getByRole("button", { name: /pause/i });
    await expect(pauseButton).toBeVisible({ timeout: 5000 });
  });

  test("play controls are interactive", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();

    // Play a song
    await page.getByText("Bohemian Rhapsody").dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });

    // Pause
    await page.getByRole("button", { name: /pause/i }).click();
    await expect(
      page.getByRole("button", { exact: true, name: "Play" }),
    ).toBeVisible({ timeout: 5000 });
  });

  test("queue panel toggles open and closed", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();

    // Find and click the queue toggle button
    const queueButton = page.getByRole("button", { name: /queue/i });
    await expect(queueButton).toBeVisible();
    await queueButton.click();

    // Queue panel header should appear
    await expect(page.getByText(/up next/i)).toBeVisible();

    // Close the queue panel
    await queueButton.click();
  });

  test("lyrics section responds to song selection", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();

    // Before any song is selected, the lyrics area should show "select a song"
    // or similar prompt (depends on i18n key "lyrics.selectSong")
    const lyricsPrompt = page.getByText(/select.*song|choose.*song/i);
    await expect(lyricsPrompt).toBeVisible();

    // Play a song
    await page.getByText("Bohemian Rhapsody").dblclick();

    // After playing, lyrics should load (mock returns lyrics)
    await expect(page.getByText("Is this the real life?")).toBeVisible({
      timeout: 10000,
    });
  });
});
