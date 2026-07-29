// @vitest-environment node

import { afterEach, describe, expect, test } from "vitest";
import {
  bucketsForRailWidth,
  getWaveformCache,
  resetWaveformCacheForTests,
  setWaveformCache,
  waveformCacheSizeForTests,
} from "./waveform-cache";

afterEach(() => {
  resetWaveformCacheForTests();
});

describe("bucketsForRailWidth", () => {
  test("clamps to 24..=1000 and rounds cssWidth*dpr/3 at 1x DPR", () => {
    expect(bucketsForRailWidth(30, 1)).toBe(24); // 10 → clamp 24
    expect(bucketsForRailWidth(600, 1)).toBe(200);
    expect(bucketsForRailWidth(4000, 1)).toBe(1000); // 1333 → clamp 1000
    expect(bucketsForRailWidth(0, 1)).toBe(200);
    expect(bucketsForRailWidth(Number.NaN, 1)).toBe(200);
    expect(bucketsForRailWidth(600, Number.NaN)).toBe(200);
    expect(bucketsForRailWidth(600, 0)).toBe(200);
  });

  test("scales bucket count by DPR at the same CSS width", () => {
    expect(bucketsForRailWidth(600, 1)).toBe(200);
    expect(bucketsForRailWidth(600, 2)).toBe(400);
    expect(bucketsForRailWidth(600, 3)).toBe(600);
    expect(bucketsForRailWidth(30, 1)).toBe(24); // 10 → clamp 24
    expect(bucketsForRailWidth(30, 2)).toBe(24); // 20 → clamp 24
    expect(bucketsForRailWidth(4000, 1)).toBe(1000);
    expect(bucketsForRailWidth(4000, 2)).toBe(1000); // 2666 → clamp 1000
  });

  test("quantizes so small DPR-only deltas are no-ops", () => {
    expect(bucketsForRailWidth(299, 1)).toBe(100);
    expect(bucketsForRailWidth(299, 1.005)).toBe(100);
    expect(bucketsForRailWidth(299, 1.02)).toBe(102);
  });
});

describe("waveform LRU", () => {
  test("promotes on get and evicts oldest after 96 entries", () => {
    for (let i = 0; i < 96; i++) {
      setWaveformCache(`song-${i}`, 200, [0.1]);
    }
    expect(waveformCacheSizeForTests()).toBe(96);
    expect(getWaveformCache("song-0", 200)?.peaks).toEqual([0.1]);
    setWaveformCache("song-new", 200, [0.9]);
    expect(waveformCacheSizeForTests()).toBe(96);
    expect(getWaveformCache("song-1", 200)).toBeNull();
    expect(getWaveformCache("song-0", 200)?.peaks).toEqual([0.1]);
    expect(getWaveformCache("song-new", 200)?.peaks).toEqual([0.9]);
  });

  test("keys are bucket-sensitive", () => {
    setWaveformCache("song-a", 100, [0.2]);
    setWaveformCache("song-a", 200, [0.4]);
    expect(getWaveformCache("song-a", 100)?.peaks).toEqual([0.2]);
    expect(getWaveformCache("song-a", 200)?.peaks).toEqual([0.4]);
  });

  test("does not cache empty peaks", () => {
    setWaveformCache("remote", 200, []);
    expect(getWaveformCache("remote", 200)).toBeNull();
    expect(waveformCacheSizeForTests()).toBe(0);
  });
});
