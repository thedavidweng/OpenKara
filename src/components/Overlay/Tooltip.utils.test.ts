import { describe, expect, test } from "vitest";
import { tooltipVisibilityReducer, getTooltipPosition } from "./Tooltip.utils";

describe("tooltipVisibilityReducer", () => {
  test("pointer-enter sets visibility to true", () => {
    expect(tooltipVisibilityReducer(false, { type: "pointer-enter" })).toBe(
      true,
    );
  });

  test("pointer-enter keeps true if already true", () => {
    expect(tooltipVisibilityReducer(true, { type: "pointer-enter" })).toBe(
      true,
    );
  });

  test("pointer-leave sets visibility to false", () => {
    expect(tooltipVisibilityReducer(true, { type: "pointer-leave" })).toBe(
      false,
    );
  });

  test("pointer-leave keeps false if already false", () => {
    expect(tooltipVisibilityReducer(false, { type: "pointer-leave" })).toBe(
      false,
    );
  });

  test("focus sets visibility to true", () => {
    expect(tooltipVisibilityReducer(false, { type: "focus" })).toBe(true);
  });

  test("blur sets visibility to false", () => {
    expect(tooltipVisibilityReducer(true, { type: "blur" })).toBe(false);
  });

  test("escape sets visibility to false", () => {
    expect(tooltipVisibilityReducer(true, { type: "escape" })).toBe(false);
  });

  test("escape from already-hidden state stays false", () => {
    expect(tooltipVisibilityReducer(false, { type: "escape" })).toBe(false);
  });
});

describe("getTooltipPosition", () => {
  const VIEWPORT_PADDING = 8;

  test("centers tooltip horizontally above the anchor when there is room", () => {
    const anchorRect = {
      left: 100,
      top: 200,
      width: 80,
      height: 30,
      right: 180,
      bottom: 230,
    };
    const tooltipSize = { width: 120, height: 40 };
    const viewport = { width: 800, height: 600 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    // Unclamped left = 100 + 80/2 - 120/2 = 100 + 40 - 60 = 80
    expect(result.left).toBe(80);
    // topAbove = 200 - 40 - 8 = 152, which >= 8 (VIEWPORT_PADDING), so above
    expect(result.top).toBe(152);
  });

  test("falls back to below anchor when not enough space above", () => {
    // Anchor is near the top of the viewport
    const anchorRect = {
      left: 100,
      top: 20,
      width: 80,
      height: 30,
      right: 180,
      bottom: 50,
    };
    const tooltipSize = { width: 120, height: 40 };
    const viewport = { width: 800, height: 600 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    // topAbove = 20 - 40 - 8 = -28, which < 8, so fallback below
    // topBelow = Math.min(50 + 8, 600 - 40 - 8) = Math.min(58, 552) = 58
    expect(result.top).toBe(58);
  });

  test("clamps tooltip to the left viewport edge", () => {
    // Anchor is near the left edge so centered tooltip would go off-screen
    const anchorRect = {
      left: 5,
      top: 200,
      width: 40,
      height: 30,
      right: 45,
      bottom: 230,
    };
    const tooltipSize = { width: 200, height: 40 };
    const viewport = { width: 800, height: 600 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    // Unclamped left = 5 + 40/2 - 200/2 = 5 + 20 - 100 = -75
    // Clamped to VIEWPORT_PADDING = 8
    expect(result.left).toBe(VIEWPORT_PADDING);
  });

  test("clamps tooltip to the right viewport edge", () => {
    // Anchor is near the right edge so centered tooltip would overflow
    const anchorRect = {
      left: 700,
      top: 200,
      width: 80,
      height: 30,
      right: 780,
      bottom: 230,
    };
    const tooltipSize = { width: 200, height: 40 };
    const viewport = { width: 800, height: 600 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    // Unclamped left = 700 + 80/2 - 200/2 = 700 + 40 - 100 = 640
    // maxRight = 800 - 200 - 8 = 592
    expect(result.left).toBe(592);
  });

  test("clamps fallback-below position to viewport bottom", () => {
    // Anchor near bottom and near top (to force below) but near bottom too
    const anchorRect = {
      left: 100,
      top: 10,
      width: 80,
      height: 30,
      right: 180,
      bottom: 40,
    };
    const tooltipSize = { width: 120, height: 40 };
    const viewport = { width: 800, height: 100 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    // topAbove = 10 - 40 - 8 = -38 < 8, fallback below
    // topBelow = Math.min(40 + 8, 100 - 40 - 8) = Math.min(48, 52) = 48
    expect(result.top).toBe(48);
  });

  test("when tooltip is wider than viewport, left clamps to VIEWPORT_PADDING", () => {
    const anchorRect = {
      left: 100,
      top: 200,
      width: 80,
      height: 30,
      right: 180,
      bottom: 230,
    };
    const tooltipSize = { width: 1000, height: 40 };
    const viewport = { width: 800, height: 600 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    // Unclamped left = 100 + 40 - 500 = -360
    // rightEdge = 800 - 1000 - 8 = -208
    // Math.max(Math.min(-360, -208), 8) = Math.max(-360, 8) = 8
    expect(result.left).toBe(VIEWPORT_PADDING);
  });
});
