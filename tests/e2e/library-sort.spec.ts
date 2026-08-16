import { test, expect } from "./fixtures/base-test";

/**
 * Library sort mode UI smoke tests.
 *
 * Verifies the sort-mode selector is visible, persists through the settings
 * mock, and reorders the song list across all three modes using the fixture
 * songs in tests/e2e/fixtures/tauri-mock.ts (sourced from
 * src/mock/preview-songs.ts).
 */
test.describe("Library sort modes", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("song-list")).toBeVisible();
  });

  test("sort mode selector is visible on the local music heading", async ({
    page,
  }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await expect(selector).toBeVisible();
    await expect(selector).toHaveValue("recently_imported");
  });

  test("recently_imported shows newest songs first", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("recently_imported");

    const songList = page.getByTestId("song-list");
    const firstSong = songList.locator("[data-song-hash]").first();
    await expect(firstSong).toContainText("One Last Kiss");
  });

  test("title_asc sorts alphabetically by title", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    // The list uses the same A-Z/# pinyin-aware bucket order as the alphabet
    // rail.  "ALL THE LOVE" sorts into the A bucket first.
    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      "ALL THE LOVE",
    );

    // All catalog songs fit in the 800px viewport, so verify full order via hash.
    const hashes = await songList
      .locator("[data-song-hash]")
      .evaluateAll((els) => els.map((el) => el.getAttribute("data-song-hash")));
    expect(hashes).toEqual([
      "all-the-love", // ALL THE LOVE
      "counting-stars", // Counting Stars
      "earfquake", // Earfquake
      "feel-good-inc", // Feel Good Inc.
      "one-last-kiss", // One Last Kiss
      "see-you-again", // See You Again (feat. Kali Uchis)
      "three-empty-words", // Three Empty Words
    ]);
  });

  test("artist_asc sorts by artist then title", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("artist_asc");

    // Gorillaz is the first artist bucket in this fixture.
    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      "Feel Good Inc.",
    );

    const hashes = await songList
      .locator("[data-song-hash]")
      .evaluateAll((els) => els.map((el) => el.getAttribute("data-song-hash")));
    expect(hashes).toEqual([
      "feel-good-inc", // Feel Good Inc. / Gorillaz
      "one-last-kiss", // One Last Kiss / Hikaru Utada
      "counting-stars", // Counting Stars / OneRepublic
      "three-empty-words", // Three Empty Words / Shawn Mendes
      "earfquake", // Earfquake / Tyler, The Creator
      "see-you-again", // See You Again / Tyler, The Creator & Kali Uchis
      "all-the-love", // ALL THE LOVE / Ye feat. Andre Troutman
    ]);
  });

  test("sort mode invokes set_library_sort_mode IPC", async ({
    page,
    tauriMock,
  }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const calls = await tauriMock.getInvokeCalls();
    const sortCall = calls.find((c) => c.cmd === "set_library_sort_mode");
    expect(sortCall).toBeDefined();
    expect(sortCall.args).toEqual({ mode: "title_asc" });
  });
});
