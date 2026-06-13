import { describe, expect, test } from "vitest";
import { findActiveLyricLineIndex } from "./lyrics-timing";

const lines = [
  { time_ms: 0 },
  { time_ms: 1000 },
  { time_ms: 3000 },
  { time_ms: 5000 },
];

describe("lyrics-timing", () => {
  test("findActiveLyricLineIndex returns the latest line at or before playback", () => {
    expect(findActiveLyricLineIndex(lines, -1)).toBe(-1);
    expect(findActiveLyricLineIndex(lines, 0)).toBe(0);
    expect(findActiveLyricLineIndex(lines, 1500)).toBe(1);
    expect(findActiveLyricLineIndex(lines, 5000)).toBe(3);
  });

  test("findActiveLyricLineIndex holds the same line until the next timestamp", () => {
    expect(findActiveLyricLineIndex(lines, 1000)).toBe(1);
    expect(findActiveLyricLineIndex(lines, 2000)).toBe(1);
    expect(findActiveLyricLineIndex(lines, 2999)).toBe(1);
    expect(findActiveLyricLineIndex(lines, 3000)).toBe(2);
  });
});
