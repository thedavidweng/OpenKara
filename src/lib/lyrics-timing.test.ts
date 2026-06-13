import { describe, expect, test } from "vitest";
import {
  findActiveLyricLineIndex,
  getLinePlaybackProgress,
  getVirtualLineCenter,
  smoothstep,
} from "./lyrics-timing";

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

  test("getLinePlaybackProgress interpolates within the current line window", () => {
    expect(getLinePlaybackProgress(lines, 1, 1000)).toBe(0);
    expect(getLinePlaybackProgress(lines, 1, 2000)).toBe(0.5);
    expect(getLinePlaybackProgress(lines, 1, 2999)).toBeCloseTo(0.9995, 3);
    expect(getLinePlaybackProgress(lines, 3, 6000)).toBe(0);
  });

  test("getVirtualLineCenter advances continuously between timestamps", () => {
    expect(getVirtualLineCenter(lines, 1000)).toBe(1);
    expect(getVirtualLineCenter(lines, 2000)).toBe(1.5);
    expect(getVirtualLineCenter(lines, 4000)).toBe(2.5);
  });

  test("smoothstep eases at the ends", () => {
    expect(smoothstep(0)).toBe(0);
    expect(smoothstep(1)).toBe(1);
    expect(smoothstep(0.5)).toBe(0.5);
  });
});
