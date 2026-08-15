import { describe, expect, test } from "vitest";
import { romanizeLyricsLines } from "../../website/src/preview-romanizer";

describe("preview romanizer stub", () => {
  test("returns the supplied lines without starting a worker", async () => {
    const { result, requestId } = await romanizeLyricsLines(
      ["忘れたくないこと"],
      "japanese",
    );
    expect(result).toEqual(["忘れたくないこと"]);
    expect(requestId).toBe(-1);
  });
});
