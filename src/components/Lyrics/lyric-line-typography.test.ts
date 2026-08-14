import { describe, expect, test } from "vitest";
import {
  CENTERED_BG_FONT_SIZE,
  CENTERED_LINE_FONT_SIZE_BASE,
  CENTERED_ROMAN_FONT_SIZE,
  getCenteredLineFontSize,
  shouldEmphasizeWord,
  wordTokenGap,
} from "./lyric-line-typography";

describe("wordTokenGap", () => {
  test("does not insert a space between CJK syllables", () => {
    expect(wordTokenGap("止", "め")).toBe("");
    expect(wordTokenGap("喪失", "の")).toBe("");
  });

  test("inserts a space between Latin words", () => {
    expect(wordTokenGap("Can", "you")).toBe(" ");
    expect(wordTokenGap("last", "kiss?")).toBe(" ");
  });

  test("still emits a visible gap after word stacks trim edge spaces", () => {
    expect(wordTokenGap("Can ", "you")).toBe(" ");
    expect(wordTokenGap("Can", " you")).toBe(" ");
  });
});

describe("centered focus type scale", () => {
  test("puts the viewport line size on the line, not a rem step", () => {
    expect(getCenteredLineFontSize(0)).toBe(CENTERED_LINE_FONT_SIZE_BASE);
    expect(getCenteredLineFontSize(-2)).toBe(
      `calc(${CENTERED_LINE_FONT_SIZE_BASE} * 0.76)`,
    );
    expect(getCenteredLineFontSize(2)).toBe(
      `calc(${CENTERED_LINE_FONT_SIZE_BASE} * 1.28)`,
    );
  });

  test("keeps roman and background as em of the line with a readable floor", () => {
    expect(CENTERED_ROMAN_FONT_SIZE).toBe("max(0.5em, 10px)");
    expect(CENTERED_BG_FONT_SIZE).toBe("max(0.7em, 10px)");
  });
});

describe("shouldEmphasizeWord", () => {
  test("marks a long CJK syllable and a short long-held Latin word", () => {
    expect(shouldEmphasizeWord({ text: "忘", time_ms: 0, end_ms: 1200 })).toBe(
      true,
    );
    expect(
      shouldEmphasizeWord({ text: "hello", time_ms: 0, end_ms: 1200 }),
    ).toBe(true);
    expect(shouldEmphasizeWord({ text: "a", time_ms: 0, end_ms: 1200 })).toBe(
      false,
    );
    expect(shouldEmphasizeWord({ text: "忘", time_ms: 0, end_ms: 400 })).toBe(
      false,
    );
  });
});
