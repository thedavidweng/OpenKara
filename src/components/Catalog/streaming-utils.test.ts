import { describe, expect, test } from "vitest";
import {
  firstGrapheme,
  formatDuration,
  formatUpdated,
} from "./streaming-utils";

describe("streaming-utils", () => {
  test("firstGrapheme keeps whole characters", () => {
    expect(firstGrapheme("night")).toBe("N");
    expect(firstGrapheme("周杰伦")).toBe("周");
    expect(firstGrapheme("👩‍👩‍👧 family")).toBe("👩‍👩‍👧");
    expect(firstGrapheme("   ")).toBe("♪");
  });

  test("formatDuration renders m:ss and a dash when unknown", () => {
    expect(formatDuration(65000)).toBe("1:05");
    expect(formatDuration(null)).toBe("—");
  });

  test("formatUpdated formats a date and tolerates unknown values", () => {
    expect(formatUpdated(1_700_000_000_000, "en")).toContain("2023");
    expect(formatUpdated(null)).toBeNull();
    expect(formatUpdated(0)).toBeNull();
  });
});
