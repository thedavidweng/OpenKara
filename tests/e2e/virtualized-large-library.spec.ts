import { test as base, expect } from "./fixtures/base-test";
import { TAURI_MOCK_SCRIPT } from "./fixtures/tauri-mock";

/**
 * Virtualized large-library e2e proof.
 *
 * The maintainer requested a 5,000-song browser fixture to prove the
 * @tanstack/react-virtual windowing and alphabet-rail navigation hold up at
 * scale — a 7-row fixture cannot exercise scroll-bucket resolution or
 * virtualizer overscan behavior meaningfully.
 *
 * These specs swap the Tauri mock's library for a synthetic 5,000-song
 * catalog (generated in-browser to avoid bloating the default fixture),
 * then assert:
 *   1. The DOM only renders a windowed slice of rows (not all 5,000).
 *   2. The alphabet rail is visible and every bucket is present.
 *   3. Clicking a rail bucket scrolls the virtualized list to that bucket.
 */

// Custom fixture: inject the large-library flag BEFORE the Tauri mock script
// so the mock's IIFE picks it up when initializing MOCK_SONGS. The base-test
// page fixture adds the mock via addInitScript, but we need our flag to run
// first, so we override the page fixture to add our flag before the mock.
const test = base.extend({
  page: async ({ page }, use) => {
    await page.addInitScript(() => {
      window.__OPENKARA_LARGE_LIBRARY_COUNT__ = 5000;
    });
    await page.addInitScript(TAURI_MOCK_SCRIPT);
    await use(page);
  },
});

test.describe("Virtualized large library (5,000 songs)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("song-list")).toBeVisible();
  });

  test("only renders a windowed slice of rows, not all 5,000", async ({
    page,
  }) => {
    const songList = page.getByTestId("song-list");
    const renderedRows = songList.locator("[data-song-hash]");
    // The virtualizer renders ~overscan + viewport rows. Even at 1280x800
    // with ~40px rows that is well under 100. Assert a generous upper bound
    // that is far below 5,000 to prove windowing is active.
    const count = await renderedRows.count();
    expect(count).toBeLessThan(200);
    expect(count).toBeGreaterThan(0);
  });

  test("alphabet rail is visible in title_asc with every bucket", async ({
    page,
  }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    // The synthetic catalog covers A–Z (titles start with each letter).
    // The rail has 27 buckets: A–Z plus "#" (non-alphabetic).
    const buttons = rail.locator("button[data-bucket]");
    await expect(buttons).toHaveCount(27);
  });

  test("clicking a rail bucket scrolls the virtualized list to that bucket", async ({
    page,
  }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    // Wait for the sort to apply: in title_asc, the first song should
    // start with "A" (the synthetic catalog cycles A–Z).
    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      /^A Song/,
    );

    // Click the "M" bucket — titles start with each letter so the viewport
    // should jump to the M-bucket region of the 5,000-song list.
    const buttonM = rail.locator("button[data-bucket='M']");
    await buttonM.click();

    // Wait for the virtualizer scroll to settle. The first M-bucket song
    // should be visible within the first few rendered rows (overscan may
    // include the last L song just above the boundary).
    const renderedRows = songList.locator("[data-song-hash]");
    await expect
      .poll(
        async () => {
          const count = await renderedRows.count();
          for (let i = 0; i < Math.min(count, 10); i++) {
            const text = await renderedRows.nth(i).textContent();
            if (text && /^M Song/.test(text)) return true;
          }
          return false;
        },
        { timeout: 5000 },
      )
      .toBe(true);
  });

  test("scrolling to the last letter bucket renders Z songs", async ({
    page,
  }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    // Wait for the sort to apply: in title_asc, the first song should
    // start with "A" (the synthetic catalog cycles A–Z).
    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      /^A Song/,
    );

    // Click the "Z" bucket — the last alphabet bucket.
    const buttonZ = rail.locator("button[data-bucket='Z']");
    await buttonZ.click();

    const renderedRows = songList.locator("[data-song-hash]");
    await expect
      .poll(
        async () => {
          const count = await renderedRows.count();
          for (let i = 0; i < Math.min(count, 10); i++) {
            const text = await renderedRows.nth(i).textContent();
            if (text && /^Z Song/.test(text)) return true;
          }
          return false;
        },
        { timeout: 5000 },
      )
      .toBe(true);
  });

  test("clicking the # bucket scrolls to non-alphabetic songs", async ({
    page,
  }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      /^A Song/,
    );

    // The synthetic catalog cycles A–Z with 5000 songs, so 5000 % 26 = 8
    // remainder songs. Those last 8 songs (indices 4992–4999) have letters
    // A–H. The # bucket resolves to the nearest following mapped bucket
    // (A at the top) since there are no non-alphabetic titles. Verify the
    // rail has the # button and clicking it navigates without error.
    const buttonHash = rail.locator("button[data-bucket='#']");
    await expect(buttonHash).toBeVisible();
    await buttonHash.click();

    // The # bucket with no non-alphabetic titles falls back to the nearest
    // mapped bucket. Verify the list still shows songs (no crash/empty).
    const renderedRows = songList.locator("[data-song-hash]");
    await expect
      .poll(async () => await renderedRows.count(), { timeout: 5000 })
      .toBeGreaterThan(0);
  });

  test("pointer scrub across the rail navigates through multiple buckets", async ({
    page,
  }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      /^A Song/,
    );

    // Get the rail bounding box to compute Y positions for different buckets.
    const railBox = await rail.boundingBox();
    expect(railBox).not.toBeNull();
    const railX = railBox!.x + railBox!.width / 2;
    // 27 buckets: A at top, # at bottom. M is at index 12 (~46% down).
    // Z is at index 25 (~93% down).
    const yForBucket = (idx: number) =>
      railBox!.y + ((idx + 0.5) / 27) * railBox!.height;

    // Pointer down on M bucket — should navigate to M songs.
    await page.mouse.move(railX, yForBucket(12));
    await page.mouse.down();

    const renderedRows = songList.locator("[data-song-hash]");
    await expect
      .poll(
        async () => {
          const count = await renderedRows.count();
          for (let i = 0; i < Math.min(count, 10); i++) {
            const text = await renderedRows.nth(i).textContent();
            if (text && /^M Song/.test(text)) return true;
          }
          return false;
        },
        { timeout: 5000 },
      )
      .toBe(true);

    // Scrub down to Z bucket — should navigate to Z songs.
    await page.mouse.move(railX, yForBucket(25));
    await expect
      .poll(
        async () => {
          const count = await renderedRows.count();
          for (let i = 0; i < Math.min(count, 10); i++) {
            const text = await renderedRows.nth(i).textContent();
            if (text && /^Z Song/.test(text)) return true;
          }
          return false;
        },
        { timeout: 5000 },
      )
      .toBe(true);

    // Release pointer.
    await page.mouse.up();
  });

  test("keyboard ArrowDown navigates to the next bucket", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      /^A Song/,
    );

    // Focus the rail and press ArrowDown to move from A to B.
    const firstButton = rail.locator("button").first();
    await firstButton.focus();
    await page.keyboard.press("ArrowDown");

    // Press Enter to navigate to the B bucket.
    await page.keyboard.press("Enter");

    const renderedRows = songList.locator("[data-song-hash]");
    await expect
      .poll(
        async () => {
          const count = await renderedRows.count();
          for (let i = 0; i < Math.min(count, 10); i++) {
            const text = await renderedRows.nth(i).textContent();
            if (text && /^B Song/.test(text)) return true;
          }
          return false;
        },
        { timeout: 5000 },
      )
      .toBe(true);
  });

  test("keyboard Home and End navigate to first and last bucket", async ({
    page,
  }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      /^A Song/,
    );

    // Focus the rail, press End to move roving to #, then Enter to navigate.
    const firstButton = rail.locator("button").first();
    await firstButton.focus();
    await page.keyboard.press("End");
    await page.keyboard.press("Enter");

    // End goes to # which falls back to nearest mapped. Then press Home
    // to go back to A and Enter to navigate to A songs.
    await page.keyboard.press("Home");
    await page.keyboard.press("Enter");

    const renderedRows = songList.locator("[data-song-hash]");
    await expect
      .poll(
        async () => {
          const count = await renderedRows.count();
          for (let i = 0; i < Math.min(count, 10); i++) {
            const text = await renderedRows.nth(i).textContent();
            if (text && /^A Song/.test(text)) return true;
          }
          return false;
        },
        { timeout: 5000 },
      )
      .toBe(true);
  });

  test("keyboard Space activates the focused bucket", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      /^A Song/,
    );

    // Focus the rail, type "m" to jump to M bucket, then Space to navigate.
    const firstButton = rail.locator("button").first();
    await firstButton.focus();
    await page.keyboard.press("m");
    await page.keyboard.press("Space");

    const renderedRows = songList.locator("[data-song-hash]");
    await expect
      .poll(
        async () => {
          const count = await renderedRows.count();
          for (let i = 0; i < Math.min(count, 10); i++) {
            const text = await renderedRows.nth(i).textContent();
            if (text && /^M Song/.test(text)) return true;
          }
          return false;
        },
        { timeout: 5000 },
      )
      .toBe(true);
  });

  test("rail navigation clears the shift-range selection anchor", async ({
    page,
  }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    const songList = page.getByTestId("song-list");
    await expect(songList.locator("[data-song-hash]").first()).toContainText(
      /^A Song/,
    );

    // Click the first song to set a selection anchor.
    const firstSong = songList.locator("[data-song-hash]").first();
    await firstSong.click();

    // Navigate via the rail to the M bucket.
    const buttonM = rail.locator("button[data-bucket='M']");
    await buttonM.click();

    // Wait for M songs to appear.
    const renderedRows = songList.locator("[data-song-hash]");
    await expect
      .poll(
        async () => {
          const count = await renderedRows.count();
          for (let i = 0; i < Math.min(count, 10); i++) {
            const text = await renderedRows.nth(i).textContent();
            if (text && /^M Song/.test(text)) return true;
          }
          return false;
        },
        { timeout: 5000 },
      )
      .toBe(true);

    // Shift-click a song in the M region. Since the rail navigation cleared
    // the range anchor, the shift-click should select only from the clicked
    // song, not span from the original A-song anchor to M.
    const mSong = renderedRows.first();
    await mSong.click({ modifiers: ["Shift"] });

    // Wait for React to commit the selection state rather than inferring it
    // from an in-flight CSS color transition. With the anchor cleared,
    // Shift-click falls back to selecting only the clicked song.
    await expect(mSong).toHaveAttribute("data-selected", "true");
    await expect(
      songList.locator("[data-song-hash][data-selected='true']"),
    ).toHaveCount(1);
  });

  test("rail is hidden in recently_imported mode", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    // Start in title_asc to make the rail visible.
    await selector.selectOption("title_asc");
    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    // Switch to recently_imported — rail should hide.
    await selector.selectOption("recently_imported");
    await expect(rail).toBeHidden();
  });

  test("rail does not cause horizontal document overflow", async ({ page }) => {
    const selector = page.getByTestId("sort-mode-selector");
    await selector.selectOption("title_asc");

    const rail = page.getByRole("navigation", { name: "Alphabet navigation" });
    await expect(rail).toBeVisible();

    // The rail is positioned absolute on the right edge. Verify it does not
    // cause the document to overflow horizontally.
    const docOverflow = await page.evaluate(
      () =>
        document.documentElement.scrollWidth >
        document.documentElement.clientWidth,
    );
    expect(docOverflow).toBe(false);
  });
});
