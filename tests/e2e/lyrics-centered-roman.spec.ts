import { test, expect } from "./fixtures/base-test";

const VIEWPORT = "[data-testid='lyrics-scroll-viewport']";

const CENTERED_ROMAN_LYRICS = {
  song_id: "earfquake",
  raw_lrc: "one last kiss",
  offset_ms: 0,
  source: "amll",
  lines: [
    {
      time_ms: 0,
      text: "忘れられない人",
      words: [
        {
          text: "忘れられない人",
          time_ms: 0,
          end_ms: 4000,
          roman: "wasurerarenai hito",
        },
      ],
      bg_words: [
        {
          text: "(I love you more than you'll ever know)",
          time_ms: 0,
          end_ms: 4000,
        },
      ],
      section: null,
      roman: "wasurerarenai hito",
    },
    {
      time_ms: 5000,
      text: "次の行",
      words: [{ text: "次の行", time_ms: 5000, end_ms: 8000 }],
      bg_words: null,
      section: null,
      roman: "tsugi no gyou",
    },
  ],
};

test.describe("Centered romanization stack", () => {
  test("keeps pronunciation readable under the main line, not a crushed third row", async ({
    page,
    tauriMock,
  }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
    await tauriMock.setMockLyrics(CENTERED_ROMAN_LYRICS);
    await page.getByRole("button", { name: "Earfquake" }).dblclick();
    await expect(page.getByText("忘れられない人")).toBeVisible({
      timeout: 10_000,
    });

    await page.locator(VIEWPORT).hover();
    await page.getByRole("button", { name: "Romanized lyrics" }).click();
    await page
      .getByRole("button", { name: "Switch to centered lyrics" })
      .click();

    const activeLine = page.locator("[data-lyrics-line-index='0']");
    const roman = activeLine.locator("[data-word-roman]");
    const bg = activeLine.locator("[data-lyrics-bg]");
    const main = activeLine.getByText("忘れられない人");
    await expect(roman).toBeVisible();
    await expect(roman).toHaveText("wasurerarenai hito");
    await expect(bg).toBeVisible();
    await expect(activeLine.locator("[data-lyrics-roman]")).toHaveCount(0);

    const [romanBox, bgBox, mainBox, romanFontPx] = await Promise.all([
      roman.boundingBox(),
      bg.boundingBox(),
      main.boundingBox(),
      roman.evaluate((el) => Number.parseFloat(getComputedStyle(el).fontSize)),
    ]);

    expect(mainBox).toBeTruthy();
    expect(romanBox).toBeTruthy();
    expect(bgBox).toBeTruthy();
    expect(romanBox!.y).toBeGreaterThan(mainBox!.y);
    expect(bgBox!.y).toBeGreaterThan(romanBox!.y);
    expect(romanBox!.height).toBeGreaterThanOrEqual(16);
    expect(romanFontPx).toBeGreaterThanOrEqual(16);

    await page.locator(VIEWPORT).screenshot({
      path: "test-results/lyrics-centered-roman.png",
    });
  });
});
