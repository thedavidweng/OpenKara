import { describe, expect, test } from "vitest";
import {
  canUseMeasuredFocusLayout,
  FOCUS_LINE_GAP_PX,
  layoutFocusLineTops,
} from "./lyrics-focus-layout";

describe("layoutFocusLineTops", () => {
  test("stacks reserved slots from the head pad", () => {
    const { tops, stageHeight } = layoutFocusLineTops([40, 20, 30], 10, 100);

    expect(tops).toEqual([100, 150, 180]);
    expect(stageHeight).toBe(310);
  });

  test("keeps a one-line stage tall enough to center", () => {
    const { tops, stageHeight } = layoutFocusLineTops(
      [48],
      FOCUS_LINE_GAP_PX,
      80,
    );

    expect(tops).toEqual([80]);
    expect(stageHeight).toBe(208);
  });
});

describe("canUseMeasuredFocusLayout", () => {
  test("rejects jsdom-zero measurements so list flow stays intact", () => {
    expect(canUseMeasuredFocusLayout([0, 0, 0])).toBe(false);
    expect(canUseMeasuredFocusLayout([24, 0])).toBe(true);
  });
});
