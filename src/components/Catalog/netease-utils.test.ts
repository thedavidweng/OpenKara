import { describe, expect, test } from "vitest";
import {
  firstGrapheme,
  formatDuration,
  formatUpdated,
  sizedCoverUrl,
} from "./netease-utils";

describe("netease-utils", () => {
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

  test("sizedCoverUrl appends the CDN size parameter", () => {
    expect(sizedCoverUrl("https://p1.music.126.net/a.jpg", 200)).toBe(
      "https://p1.music.126.net/a.jpg?param=200y200",
    );
    expect(sizedCoverUrl("https://p1.music.126.net/a.jpg?x=1", 512)).toBe(
      "https://p1.music.126.net/a.jpg?x=1&param=512y512",
    );
    expect(sizedCoverUrl(null, 200)).toBeNull();
  });

  test("formatUpdated formats a date and tolerates unknown values", () => {
    expect(formatUpdated(1_700_000_000_000, "en")).toContain("2023");
    expect(formatUpdated(null)).toBeNull();
    expect(formatUpdated(0)).toBeNull();
  });
});
