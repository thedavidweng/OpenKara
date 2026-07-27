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
    const pages = buildPlainTextPageStartIndices([30, 30, 30, 30], 80, 10);
    expect(pages).toEqual([0, 2]);
  });

  test("handles zero gap", () => {
    expect(buildPlainTextPageStartIndices([40, 40, 40], 100, 0)).toEqual([
      0, 2,
    ]);
  });

  test("handles line taller than available height", () => {
    // A single line taller than available height gets its own page
    expect(buildPlainTextPageStartIndices([200], 100, 10)).toEqual([0]);
  });

  test("handles varying line heights", () => {
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
