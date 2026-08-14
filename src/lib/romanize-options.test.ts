import { describe, expect, test, vi } from "vitest";
import type { RomanizeResult, Romanizer } from "lyric-romanizer";
import { OPTIONS_BY_LANGUAGE, romanizeLinesWith } from "./romanize-options";
import { SONG_LANGUAGES } from "@/components/Library/song-list-item-menu";

function fakeRomanizer(
  impl: (
    lines: readonly string[],
    options?: unknown,
  ) => Promise<RomanizeResult>,
) {
  const romanizeLines = vi.fn(impl);
  const romanizer: Romanizer = {
    romanizeLine: vi.fn(async (line: string) => line),
    romanizeLines,
    warmup: vi.fn(async () => undefined),
  };
  return { romanizer, romanizeLines };
}

describe("OPTIONS_BY_LANGUAGE", () => {
  test("maps every SongLanguage to a defined, scripted option", () => {
    for (const language of SONG_LANGUAGES) {
      const options = OPTIONS_BY_LANGUAGE[language];
      expect(options).toBeDefined();
      expect(options.script).toBeTruthy();
    }
  });

  test("pins japanese to the japanese script with no dialect", () => {
    expect(OPTIONS_BY_LANGUAGE.japanese).toEqual({ script: "japanese" });
  });

  test("pins mandarin and cantonese to chinese with the right dialect", () => {
    expect(OPTIONS_BY_LANGUAGE.mandarin).toEqual({
      script: "chinese",
      dialect: "mandarin",
    });
    expect(OPTIONS_BY_LANGUAGE.cantonese).toEqual({
      script: "chinese",
      dialect: "cantonese",
    });
  });
});

describe("romanizeLinesWith — pinned language", () => {
  test("romanizes the whole array in one call with the pinned options", async () => {
    const { romanizer, romanizeLines } = fakeRomanizer(async (lines) => ({
      script: "japanese",
      lines: lines.map((line) => `romaji:${line}`),
    }));

    const result = await romanizeLinesWith(
      romanizer,
      ["恋愛", "約束"],
      "japanese",
    );

    expect(result).toEqual(["romaji:恋愛", "romaji:約束"]);
    expect(romanizeLines).toHaveBeenCalledTimes(1);
    expect(romanizeLines).toHaveBeenCalledWith(["恋愛", "約束"], {
      script: "japanese",
    });
  });

  test("passes the cantonese dialect for cantonese", async () => {
    const { romanizer, romanizeLines } = fakeRomanizer(async (lines) => ({
      script: "chinese",
      lines: lines.map((line) => `jyut:${line}`),
    }));

    await romanizeLinesWith(romanizer, ["你好"], "cantonese");

    expect(romanizeLines).toHaveBeenCalledTimes(1);
    expect(romanizeLines).toHaveBeenCalledWith(["你好"], {
      script: "chinese",
      dialect: "cantonese",
    });
  });

  test("includes Latin lines in the single pinned call", async () => {
    const { romanizer, romanizeLines } = fakeRomanizer(async (lines) => ({
      script: "japanese",
      lines: lines.map((line) => (line === "Hello" ? line : `romaji:${line}`)),
    }));

    const result = await romanizeLinesWith(
      romanizer,
      ["Hello", "恋愛"],
      "japanese",
    );

    expect(result).toEqual(["Hello", "romaji:恋愛"]);
    expect(romanizeLines).toHaveBeenCalledTimes(1);
    expect(romanizeLines).toHaveBeenCalledWith(["Hello", "恋愛"], {
      script: "japanese",
    });
  });

  test("falls back to the original line when the engine throws", async () => {
    const { romanizer } = fakeRomanizer(async () => {
      throw new Error("engine boom");
    });

    const result = await romanizeLinesWith(romanizer, ["恋愛"], "japanese");

    expect(result).toEqual(["恋愛"]);
  });

  test("keeps the original line when the engine returns nothing", async () => {
    const { romanizer } = fakeRomanizer(async () => ({
      script: "japanese",
      lines: [],
    }));

    const result = await romanizeLinesWith(romanizer, ["恋愛"], "japanese");

    expect(result).toEqual(["恋愛"]);
  });
});

describe("romanizeLinesWith — unknown language", () => {
  test("passes the whole array in one call with no options (null)", async () => {
    const { romanizer, romanizeLines } = fakeRomanizer(async (lines) => ({
      script: "japanese",
      lines: lines.map((line) => `romaji:${line}`),
    }));

    const result = await romanizeLinesWith(
      romanizer,
      ["恋愛", "君を想う", "夜空"],
      null,
    );

    expect(result).toEqual(["romaji:恋愛", "romaji:君を想う", "romaji:夜空"]);
    expect(romanizeLines).toHaveBeenCalledTimes(1);
    expect(romanizeLines).toHaveBeenCalledWith(["恋愛", "君を想う", "夜空"]);
  });

  test("uses the whole-array path when language is omitted (undefined)", async () => {
    const { romanizer, romanizeLines } = fakeRomanizer(async (lines) => ({
      script: "chinese",
      lines: lines.map((line) => `x:${line}`),
    }));

    const result = await romanizeLinesWith(romanizer, ["你好"]);

    expect(result).toEqual(["x:你好"]);
    expect(romanizeLines).toHaveBeenCalledTimes(1);
    expect(romanizeLines).toHaveBeenCalledWith(["你好"]);
  });

  test("keeps a line the engine omits from its result", async () => {
    const { romanizer } = fakeRomanizer(async () => ({
      script: "japanese",
      lines: ["romaji:恋愛"],
    }));

    const result = await romanizeLinesWith(romanizer, ["恋愛", "夜空"], null);

    expect(result).toEqual(["romaji:恋愛", "夜空"]);
  });

  test("falls back to the original lines when the engine throws", async () => {
    const { romanizer } = fakeRomanizer(async () => {
      throw new Error("engine boom");
    });

    const result = await romanizeLinesWith(romanizer, ["恋愛", "夜空"], null);

    expect(result).toEqual(["恋愛", "夜空"]);
  });
});
