import { test, expect } from "./fixtures/base-test";

/**
 * Library sort mode UI smoke tests.
 *
 * Verifies the sort-mode selector is visible, persists through the settings
 * mock, and reorders the song list across all three modes using the fixture
 * songs in tests/e2e/fixtures/tauri-mock.ts.
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
    await expect(firstSong).toContainText("Bohemian Rhapsody");
  });

  test("title_asc sorts alphabetically by title", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    // The zh-Hans-CN collator sorts CJK before Latin, so 北京之夜 comes first.
    // Among Latin titles, Alpha 2 sorts before Alpha 10 (numeric collation).
    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      "北京之夜",
    );

    // All 7 songs fit in the 800px viewport, so verify full order via hash.
    const hashes = await songList
      .locator("[data-song-hash]")
      .evaluateAll((els) => els.map((el) => el.getAttribute("data-song-hash")));
    expect(hashes).toEqual([
      "fff666", // 北京之夜 (CJK first)
      "ddd444", // Alpha 2 (numeric: 2 < 10)
      "eee555", // Alpha 10
      "aaa111", // Bohemian Rhapsody
      "bbb222", // Hotel California
      "ccc333", // Imagine
      "ggg777", // (null title sorts last)
    ]);
  });

  test("artist_asc sorts by artist then title", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("artist_asc");

    // 崔健 (CJK artist) sorts first.
    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      "北京之夜",
    );

    const hashes = await songList
      .locator("[data-song-hash]")
      .evaluateAll((els) => els.map((el) => el.getAttribute("data-song-hash")));
    expect(hashes).toEqual([
      "fff666", // 北京之夜 / 崔健 (CJK first)
      "bbb222", // Hotel California / Eagles
      "ccc333", // Imagine / John Lennon
      "aaa111", // Bohemian Rhapsody / Queen
      "ddd444", // Alpha 2 / The Beta
      "eee555", // Alpha 10 / The Beta (tie on artist, title breaks)
      "ggg777", // (null artist sorts last)
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
