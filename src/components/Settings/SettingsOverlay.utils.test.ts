import { describe, expect, test } from "vitest";
import { formatBytes } from "./SettingsOverlay.utils";

describe("formatBytes", () => {
  test("returns '0 B' for zero bytes", () => {
    expect(formatBytes(0)).toBe("0 B");
  });

  test("formats bytes under 1 KB", () => {
    expect(formatBytes(512)).toBe("512 B");
  });

  test("formats kilobytes with one decimal", () => {
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
  });

  test("formats megabytes with one decimal", () => {
    expect(formatBytes(1048576)).toBe("1.0 MB");
    expect(formatBytes(2621440)).toBe("2.5 MB");
  });

  test("formats gigabytes with one decimal", () => {
    expect(formatBytes(1073741824)).toBe("1.0 GB");
  });
});
