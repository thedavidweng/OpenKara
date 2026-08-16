import { describe, expect, test } from "vitest";
import { visibleRomanizedText } from "./lyrics-roman-visibility";

describe("visibleRomanizedText", () => {
  test("hides a Latin echo of the original line", () => {
    expect(
      visibleRomanizedText(
        "'Cause you make my earthquake (Earthquake)",
        "'Cause you make my earthquake (Earthquake)",
      ),
    ).toBeUndefined();
    expect(
      visibleRomanizedText("Hello world", "hello   world"),
    ).toBeUndefined();
  });

  test("keeps a real pronunciation that differs from the original", () => {
    expect(visibleRomanizedText("忘れられない人", "wasurerarenai hito")).toBe(
      "wasurerarenai hito",
    );
    expect(visibleRomanizedText("你好世界", "ni hao shi jie")).toBe(
      "ni hao shi jie",
    );
  });

  test("hides empty romanization", () => {
    expect(visibleRomanizedText("你好", "   ")).toBeUndefined();
    expect(visibleRomanizedText("你好", undefined)).toBeUndefined();
  });
});
