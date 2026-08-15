import { test, expect } from "./fixtures/base-test";

const VIEWPORT = "[data-testid='lyrics-scroll-viewport']";

const VISUAL_LYRICS = {
  song_id: "earfquake",
  raw_lrc: "visual check",
  offset_ms: 0,
  source: "amll",
  lines: [
    {
      time_ms: 0,
      text: "忘れられない人",
      words: [
        { text: "忘れ", time_ms: 0, end_ms: 1600, roman: "wasure" },
        { text: "られ", time_ms: 1600, end_ms: 2800, roman: "rare" },
        { text: "ない", time_ms: 2800, end_ms: 3600, roman: "nai" },
        { text: "人", time_ms: 3600, end_ms: 5000, roman: "hito" },
      ],
      bg_words: [
        {
          text: "(I love you more than you'll ever know)",
          time_ms: 0,
          end_ms: 5000,
        },
      ],
      section: null,
      roman: "wasure rare nai hito",
    },
    {
      time_ms: 5200,
      text: "次の行",
      words: [
        { text: "次の", time_ms: 5200, end_ms: 7000, roman: "tsugi no" },
        { text: "行", time_ms: 7000, end_ms: 8500, roman: "gyou" },
      ],
      bg_words: null,
      section: null,
      roman: "tsugi no gyou",
    },
    {
      time_ms: 9000,
      text: "Can you give me one last kiss?",
      words: [
        { text: "Can ", time_ms: 9000, end_ms: 9300 },
        { text: "you ", time_ms: 9300, end_ms: 9600 },
        { text: "give ", time_ms: 9600, end_ms: 10000 },
        { text: "me ", time_ms: 10000, end_ms: 10300 },
        { text: "one ", time_ms: 10300, end_ms: 10700 },
        { text: "last ", time_ms: 10700, end_ms: 11200 },
        { text: "kiss?", time_ms: 11200, end_ms: 12000 },
      ],
      bg_words: null,
      section: null,
      roman: null,
    },
  ],
};

async function openCenteredRoman(
  page: import("@playwright/test").Page,
  tauriMock: {
    setMockLyrics: (lyrics: unknown) => Promise<void>;
    setPlaybackSnapshot: (patch: {
      is_playing?: boolean;
      state?: string;
      position_ms?: number;
    }) => Promise<unknown>;
  },
) {
  await page.goto("/");
  await expect(page.getByText("Earfquake")).toBeVisible();
  await tauriMock.setMockLyrics(VISUAL_LYRICS);
  await page.getByRole("button", { name: "Earfquake" }).dblclick();
  await expect(page.getByText("忘れられない人")).toBeVisible({
    timeout: 10_000,
  });
  await page.locator(VIEWPORT).hover();
  await page.getByRole("button", { name: "Romanized lyrics" }).click();
  await page.getByRole("button", { name: "Switch to centered lyrics" }).click();
  await expect(page.locator("[data-word-roman]").first()).toBeVisible();
}

async function freezeAt(
  tauriMock: {
    setPlaybackSnapshot: (patch: {
      is_playing?: boolean;
      state?: string;
      position_ms?: number;
    }) => Promise<unknown>;
  },
  positionMs: number,
) {
  await tauriMock.setPlaybackSnapshot({
    is_playing: false,
    state: "paused",
    position_ms: positionMs,
  });
}

test.describe("Lyrics visual check", () => {
  test("captures centered, mid-wipe, next line, and left-aligned frames", async ({
    page,
    tauriMock,
  }) => {
    await openCenteredRoman(page, tauriMock);

    await freezeAt(tauriMock, 200);
    await expect(page.locator("[data-lyrics-line-index='0']")).toBeVisible();
    await page.locator(VIEWPORT).screenshot({
      path: "test-results/visual/centered-start.png",
    });

    await freezeAt(tauriMock, 800);
    await page.waitForTimeout(80);
    await page.locator(VIEWPORT).screenshot({
      path: "test-results/visual/centered-mid-first-word.png",
    });

    await freezeAt(tauriMock, 2400);
    await page.waitForTimeout(80);
    await page.locator(VIEWPORT).screenshot({
      path: "test-results/visual/centered-mid-line.png",
    });

    await freezeAt(tauriMock, 6200);
    await page.waitForTimeout(80);
    await page.locator(VIEWPORT).screenshot({
      path: "test-results/visual/centered-next-line.png",
    });

    const first = page.locator("[data-lyrics-line-index='0']");
    const wordRomans = first.locator("[data-word-roman]");
    await freezeAt(tauriMock, 800);
    await expect(first.getByText("忘れ")).toBeVisible();
    await expect(first.getByText("られ")).toBeVisible();
    await expect(first.getByText("ない")).toBeVisible();
    await expect(first.getByText("人")).toBeVisible();
    await expect(wordRomans).toHaveCount(4);

    let wipe: string[] = [];
    await expect
      .poll(
        async () => {
          wipe = await page.evaluate(() => {
            const fills = [
              ...document.querySelectorAll<HTMLElement>(
                "[data-lyrics-line-index='0'] [data-karaoke-fill]",
              ),
            ];
            return fills.map(
              (el) => el.style.webkitMaskPosition || el.style.maskPosition,
            );
          });
          const first = wipe[0] ?? "";
          const last = wipe[3] ?? "";
          return first !== last && last.includes("-");
        },
        { timeout: 2000, intervals: [50, 80, 120] },
      )
      .toBe(true);
    expect(wipe[0] ?? "").not.toBe(wipe[3] ?? "");
    expect(wipe[3] ?? "").toContain("-");
    const boxes = await wordRomans.evaluateAll((nodes) =>
      nodes.map((node) => {
        const box = node.getBoundingClientRect();
        const style = getComputedStyle(node);
        return {
          text: node.textContent,
          x: box.x,
          y: box.y,
          h: box.height,
          font: Number.parseFloat(style.fontSize),
        };
      }),
    );
    expect(boxes[0]?.text).toBe("wasure");
    expect(boxes[1]?.x ?? 0).toBeGreaterThan(boxes[0]?.x ?? 0);
    expect(boxes.every((box) => box.font >= 14)).toBe(true);

    await page.locator(VIEWPORT).hover();
    await page
      .getByRole("button", { name: "Switch to left-aligned lyrics" })
      .click();
    await freezeAt(tauriMock, 800);
    await page.waitForTimeout(80);
    await page.locator(VIEWPORT).screenshot({
      path: "test-results/visual/left-aligned.png",
    });

    const leftLine = page.locator("[data-lyrics-line-index='0']");
    await expect(leftLine.getByText("忘れ")).toBeVisible();
    await expect(leftLine.getByText("られ")).toBeVisible();
    await expect(leftLine.getByText("人")).toBeVisible();
    await expect(
      page.getByText("Can you give me one last kiss?"),
    ).toBeVisible();

    const metrics = await page.evaluate(() => {
      const fill = document.querySelector<HTMLElement>("[data-karaoke-fill]");
      const stage = document.querySelector("[data-lyrics-stage]");
      if (!fill) {
        return null;
      }
      const style = getComputedStyle(fill);
      return {
        stage: stage?.getAttribute("data-lyrics-stage"),
        maskImage: style.webkitMaskImage || style.maskImage,
        maskPosition: style.webkitMaskPosition || style.maskPosition,
        bright: fill
          .closest("button")
          ?.style.getPropertyValue("--bright-mask-alpha"),
        dark: fill
          .closest("button")
          ?.style.getPropertyValue("--dark-mask-alpha"),
      };
    });
    expect(metrics?.maskImage ?? "").toContain("linear-gradient");
    expect(metrics?.bright).toBe("1");
  });
});
