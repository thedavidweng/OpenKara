import { describe, expect, test } from "vitest";
import { buildPlainTextPageStartIndices } from "./plain-text-pages";

describe("buildPlainTextPageStartIndices", () => {
  test("returns [0] for empty line heights", () => {
    expect(buildPlainTextPageStartIndices([], 500, 10)).toEqual([0]);
  });

  test("single page when all lines fit", () => {
    // 3 lines of 20px + 2 gaps of 10px = 80px, available = 500
    expect(buildPlainTextPageStartIndices([20, 20, 20], 500, 10)).toEqual([0]);
  });

  test("breaks into pages when lines exceed available height", () => {
    // Each line 30px, gap 10px, available 80px
    // Page 1: line0(30) + gap(10) + line1(30) = 70, fits
    // line2 would make 70 + 10 + 30 = 110 > 80, so page break
    // Page 2: line2(30) + gap(10) + line3(30) = 70, fits
    const pages = buildPlainTextPageStartIndices([30, 30, 30, 30], 80, 10);
    expect(pages).toEqual([0, 2]);
  });

  test("handles zero gap", () => {
    // 3 lines of 40px, no gap, available 100
    // line0(40) + line1(40) = 80, fits
    // line2 would make 120 > 100, page break
    expect(buildPlainTextPageStartIndices([40, 40, 40], 100, 0)).toEqual([
      0, 2,
    ]);
  });

  test("handles line taller than available height", () => {
    // A single line taller than available height gets its own page
    expect(buildPlainTextPageStartIndices([200], 100, 10)).toEqual([0]);
  });

  test("handles varying line heights", () => {
    // available = 100, gap = 5
    // line0(50) = 50, fits
    // line1(60): 50 + 5 + 60 = 115 > 100, page break at index 1
    // line1(60) = 60, fits
    // line2(30): 60 + 5 + 30 = 95, fits
    expect(buildPlainTextPageStartIndices([50, 60, 30], 100, 5)).toEqual([
      0, 1,
    ]);
  });

  test("floors negative or zero available height to 1", () => {
    // Should not throw, and should produce one page per line
    const pages = buildPlainTextPageStartIndices([10, 10], 0, 5);
    expect(pages).toEqual([0, 1]);
  });

  test("floors negative gap to 0", () => {
    // Negative gap treated as 0
    expect(buildPlainTextPageStartIndices([30, 30], 100, -5)).toEqual([0]);
  });
});
