import { describe, expect, test } from "vitest";
import { formatBytes, formatDuration } from "./format";

describe("formatDuration", () => {
  test("formats zero milliseconds", () => {
    expect(formatDuration(0)).toBe("0:00");
  });

  test("formats seconds under a minute", () => {
    expect(formatDuration(5000)).toBe("0:05");
    expect(formatDuration(59000)).toBe("0:59");
  });

  test("formats exact minute boundary", () => {
    expect(formatDuration(60000)).toBe("1:00");
  });

  test("formats minutes and seconds", () => {
    expect(formatDuration(90000)).toBe("1:30");
    expect(formatDuration(185000)).toBe("3:05");
  });

  test("truncates sub-second remainder", () => {
    expect(formatDuration(61999)).toBe("1:01");
  });

  test("formats large durations", () => {
    expect(formatDuration(3600000)).toBe("60:00");
  });
});

describe("formatBytes", () => {
  test("formats bytes under 1 KB", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1023)).toBe("1023 B");
  });

  test("formats kilobytes", () => {
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
  });

  test("formats megabytes", () => {
    expect(formatBytes(1024 * 1024)).toBe("1.0 MB");
    expect(formatBytes(1024 * 1024 * 2.5)).toBe("2.5 MB");
  });

  test("formats gigabytes", () => {
    expect(formatBytes(1024 * 1024 * 1024)).toBe("1.0 GB");
  });
});
