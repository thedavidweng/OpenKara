import { describe, expect, test } from "vitest";
import {
  buildAudiencePresentationSpec,
  colorToCss,
} from "./audience-presentation";

describe("buildAudiencePresentationSpec", () => {
  test("returns default font size for step 0", () => {
    const spec = buildAudiencePresentationSpec(0);
    expect(spec.fontSizePx).toBe(72);
  });

  test("maps font steps to correct sizes", () => {
    expect(buildAudiencePresentationSpec(-2).fontSizePx).toBe(48);
    expect(buildAudiencePresentationSpec(-1).fontSizePx).toBe(60);
    expect(buildAudiencePresentationSpec(1).fontSizePx).toBe(96);
    expect(buildAudiencePresentationSpec(2).fontSizePx).toBe(96);
  });

  test("clamps out-of-range steps", () => {
    // Steps beyond [-2, 2] should clamp to the boundary values
    expect(buildAudiencePresentationSpec(-10).fontSizePx).toBe(48);
    expect(buildAudiencePresentationSpec(10).fontSizePx).toBe(96);
  });

  test("includes all required layout properties", () => {
    const spec = buildAudiencePresentationSpec(0);
    expect(spec.contentWidthRatio).toBe(0.92);
    expect(spec.contentMaxWidthPx).toBe(1600);
    expect(spec.horizontalPaddingPx).toBe(64);
    expect(spec.verticalPaddingPx).toBe(56);
    expect(spec.lineGapPx).toBe(40);
    expect(spec.lineHeightMultiple).toBe(1.08);
    expect(spec.activeScale).toBe(1.05);
  });

  test("includes all color objects", () => {
    const spec = buildAudiencePresentationSpec(0);
    expect(spec.activeTextColor).toBeDefined();
    expect(spec.pastTextColor).toBeDefined();
    expect(spec.futureTextColor).toBeDefined();
    expect(spec.plainTextColor).toBeDefined();
    expect(spec.statusTextColor).toBeDefined();
    expect(spec.activeGlowColor).toBeDefined();
  });
});

describe("colorToCss", () => {
  test("converts pure white", () => {
    expect(colorToCss({ red: 1, green: 1, blue: 1, alpha: 1 })).toBe(
      "rgba(255, 255, 255, 1)",
    );
  });

  test("converts pure black", () => {
    expect(colorToCss({ red: 0, green: 0, blue: 0, alpha: 1 })).toBe(
      "rgba(0, 0, 0, 1)",
    );
  });

  test("converts semi-transparent color", () => {
    expect(colorToCss({ red: 1, green: 0, blue: 0, alpha: 0.5 })).toBe(
      "rgba(255, 0, 0, 0.5)",
    );
  });

  test("rounds fractional channel values", () => {
    // 72/255 ≈ 0.282..., should round to 72
    expect(
      colorToCss({ red: 72 / 255, green: 72 / 255, blue: 72 / 255, alpha: 1 }),
    ).toBe("rgba(72, 72, 72, 1)");
  });
});
