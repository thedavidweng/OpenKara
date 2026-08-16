import { describe, expect, test } from "vitest";
import { resolveRomanFillUnits, resolveWordRomans } from "./lyric-word-romans";

describe("resolveWordRomans", () => {
  test("uses supplied word roman when any token has it", () => {
    expect(
      resolveWordRomans(
        [
          { text: "君", time_ms: 0, end_ms: 500, roman: "kimi" },
          { text: "の", time_ms: 500, end_ms: 800 },
        ],
        "ignored line",
      ),
    ).toEqual(["kimi", null]);
  });

  test("pairs a spaced line roman when the token count matches", () => {
    expect(
      resolveWordRomans(
        [
          { text: "忘", time_ms: 0, end_ms: 200 },
          { text: "れ", time_ms: 200, end_ms: 400 },
        ],
        "wa re",
      ),
    ).toEqual(["wa", "re"]);
  });

  test("packs leftover mora onto a mixed kanji-kana word", () => {
    expect(
      resolveWordRomans(
        [{ text: "忘れられない人", time_ms: 0, end_ms: 2000 }],
        "wasurerarenai hito",
      ),
    ).toEqual(["wasurerarenai hito"]);
  });

  test("packs pinyin onto multi-character Chinese words", () => {
    expect(
      resolveWordRomans(
        [
          { text: "你好", time_ms: 0, end_ms: 400 },
          { text: "世界", time_ms: 400, end_ms: 800 },
        ],
        "ni hao shi jie",
      ),
    ).toEqual(["ni hao", "shi jie"]);
  });
});

describe("resolveRomanFillUnits", () => {
  test("wipes every mora of a kanji-heavy Japanese line", () => {
    const units = resolveRomanFillUnits(
      [
        { text: "私", time_ms: 86780, end_ms: 87158 },
        { text: "の", time_ms: 87158, end_ms: 87283 },
        { text: "心", time_ms: 87283, end_ms: 87680 },
        { text: "の", time_ms: 87680, end_ms: 87836 },
        { text: "プ", time_ms: 87836, end_ms: 87967 },
        { text: "ロ", time_ms: 87967, end_ms: 88108 },
        { text: "ジェク", time_ms: 88108, end_ms: 88488 },
        { text: "ター", time_ms: 88636, end_ms: 88971 },
      ],
      "wa ta shi no ko ko ro no pu ro je ku ta a",
    );

    expect(units?.map((unit) => unit.text)).toEqual([
      "wa",
      "ta",
      "shi",
      "no",
      "ko",
      "ko",
      "ro",
      "no",
      "pu",
      "ro",
      "je",
      "ku",
      "ta",
      "a",
    ]);
  });

  test("ignores quote marks when packing a Japanese line", () => {
    const units = resolveRomanFillUnits(
      [
        { text: "「写", time_ms: 80585, end_ms: 80851 },
        { text: "真", time_ms: 80851, end_ms: 80952 },
        { text: "は", time_ms: 80952, end_ms: 81080 },
        { text: "苦", time_ms: 81080, end_ms: 81472 },
        { text: "手", time_ms: 81472, end_ms: 81588 },
        { text: "な", time_ms: 81588, end_ms: 81728 },
        { text: "ん", time_ms: 81728, end_ms: 81843 },
        { text: "だ」", time_ms: 81843, end_ms: 82209 },
      ],
      "sha shi n wa ni ga te na n da",
    );

    expect(units?.map((unit) => unit.text)).toEqual([
      "sha",
      "shi",
      "n",
      "wa",
      "ni",
      "ga",
      "te",
      "na",
      "n",
      "da",
    ]);
  });
});
