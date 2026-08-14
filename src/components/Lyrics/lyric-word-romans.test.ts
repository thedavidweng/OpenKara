import { describe, expect, test } from "vitest";
import { resolveWordRomans } from "./lyric-word-romans";

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

  test("falls back to a line sub-row when counts do not match", () => {
    expect(
      resolveWordRomans(
        [{ text: "忘れられない人", time_ms: 0, end_ms: 2000 }],
        "wasurerarenai hito",
      ),
    ).toBeNull();
  });
});
