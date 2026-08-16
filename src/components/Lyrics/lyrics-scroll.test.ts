// @vitest-environment jsdom

import { describe, expect, test } from "vitest";
import { FOCUS_ALIGN_POSITION, getCenteredScrollTop } from "./lyrics-scroll";

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

  test("places the focus line slightly above the optical center", () => {
    expect(
      getCenteredScrollTop({
        viewportHeight: 400,
        scrollHeight: 1200,
        lineOffsetTop: 500,
        lineHeight: 80,
        alignPosition: FOCUS_ALIGN_POSITION,
      }),
    ).toBe(360);
  });
});
