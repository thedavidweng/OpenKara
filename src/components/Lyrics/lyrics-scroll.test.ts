// @vitest-environment jsdom

import { describe, expect, test } from "vitest";
import {
  collectLineSnapScrollTops,
  getCenteredScrollTop,
  nearestSnapScrollTop,
  projectDecelerationOffset,
  resolveLyricScrollLanding,
  stepLineSnapScrollTop,
} from "./lyrics-scroll";

describe("getCenteredScrollTop", () => {
  test("centers the active lyric line when there is enough space", () => {
    expect(
      getCenteredScrollTop({
        viewportHeight: 400,
        scrollHeight: 1200,
        lineOffsetTop: 500,
        lineHeight: 80,
      }),
    ).toBe(340);
  });

  test("clamps to the top of the viewport", () => {
    expect(
      getCenteredScrollTop({
        viewportHeight: 400,
        scrollHeight: 1200,
        lineOffsetTop: 40,
        lineHeight: 60,
      }),
    ).toBe(0);
  });

  test("clamps to the bottom of the viewport", () => {
    expect(
      getCenteredScrollTop({
        viewportHeight: 400,
        scrollHeight: 1200,
        lineOffsetTop: 1080,
        lineHeight: 80,
      }),
    ).toBe(800);
  });

  test("uses the lyric line midpoint even when line heights vary", () => {
    expect(
      getCenteredScrollTop({
        viewportHeight: 500,
        scrollHeight: 2000,
        lineOffsetTop: 720,
        lineHeight: 140,
      }),
    ).toBe(540);
  });

  test("places the focus line above the optical center", () => {
    expect(
      getCenteredScrollTop({
        viewportHeight: 400,
        scrollHeight: 1200,
        lineOffsetTop: 500,
        lineHeight: 80,
        alignPosition: 0.38,
      }),
    ).toBe(388);
  });
});

describe("lyric line snap landings", () => {
  test("projects a flick with Apple's deceleration form", () => {
    expect(projectDecelerationOffset(0)).toBe(0);
    expect(projectDecelerationOffset(1000)).toBeCloseTo(499, 0);
    expect(projectDecelerationOffset(-1000)).toBeCloseTo(-499, 0);
  });

  test("picks the nearest snap from a projected rest point", () => {
    const snaps = [0, 120, 240, 360];
    expect(nearestSnapScrollTop(snaps, 130)).toBe(120);
    expect(resolveLyricScrollLanding(snaps, 130, 0)).toBe(120);
    expect(resolveLyricScrollLanding(snaps, 130, 800)).toBe(360);
    expect(resolveLyricScrollLanding([], 130, 800)).toBeNull();
  });

  test("steps one snap in the wheel direction", () => {
    const snaps = [0, 120, 240];
    expect(stepLineSnapScrollTop(snaps, 0, 1)).toBe(120);
    expect(stepLineSnapScrollTop(snaps, 120, 1)).toBe(240);
    expect(stepLineSnapScrollTop(snaps, 240, 1)).toBe(240);
    expect(stepLineSnapScrollTop(snaps, 240, -1)).toBe(120);
    expect(stepLineSnapScrollTop(snaps, 0, -1)).toBe(0);
  });

  test("collects unique snap tops for each measured line", () => {
    const container = document.createElement("div");
    Object.defineProperty(container, "clientHeight", { value: 400 });
    Object.defineProperty(container, "scrollHeight", { value: 1200 });

    for (const [index, top] of [80, 280, 480].entries()) {
      const line = document.createElement("div");
      line.dataset.lyricsLineIndex = String(index);
      Object.defineProperty(line, "offsetTop", { value: top });
      Object.defineProperty(line, "clientHeight", { value: 80 });
      container.append(line);
    }

    expect(collectLineSnapScrollTops(container, 3, 0.38)).toEqual([
      0, 168, 368,
    ]);
  });
});
