import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import {
  parseTtml,
  parseTtmlTimestamp,
} from "../scripts/parse-preview-ttml.mjs";

describe("parseTtmlTimestamp", () => {
  test("reads minute-second-fraction tags", () => {
    expect(parseTtmlTimestamp("00:20.686")).toBe(20_686);
    expect(parseTtmlTimestamp("02:47.615")).toBe(167_615);
  });
});

describe("parseTtml", () => {
  test("keeps word timing, line roman, and background words", () => {
    const ttml = `<tt>
      <body>
        <div>
          <p begin="00:20.686" end="00:22.460">
            <span begin="00:20.686" end="00:20.904">初</span>
            <span begin="00:20.904" end="00:21.039">め</span>
            <span ttm:role="x-translation">ignored</span>
            <span ttm:role="x-roman">ha ji me</span>
          </p>
          <p begin="02:45.799" end="02:48.803">
            <span begin="02:45.799" end="02:46.345">忘</span>
            <span begin="02:46.345" end="02:46.545">れ</span>
            <span ttm:role="x-bg">
              <span begin="02:46.375" end="02:46.478">(Oh</span>
              <span begin="02:46.740" end="02:46.877">oh</span>
            </span>
            <span ttm:role="x-roman">wa su re</span>
          </p>
        </div>
      </body>
    </tt>`;

    const lines = parseTtml(ttml);
    expect(lines).toHaveLength(2);
    expect(lines[0].text).toBe("初め");
    expect(lines[0].roman).toBe("ha ji me");
    expect(lines[0].words).toEqual([
      { time_ms: 20_686, end_ms: 20_904, text: "初", roman: null },
      { time_ms: 20_904, end_ms: 21_039, text: "め", roman: null },
    ]);
    expect(lines[1].text).toBe("忘れ");
    expect(lines[1].bg_words).toEqual([
      { time_ms: 166_375, end_ms: 166_478, text: "(Oh", roman: null },
      { time_ms: 166_740, end_ms: 166_877, text: "oh", roman: null },
    ]);
  });

  test("attaches nested x-roman to the timed word", () => {
    const lines = parseTtml(`<p begin="00:10.000" end="00:12.000">
      <span begin="00:10.000" end="00:11.000">君<span ttm:role="x-roman">kimi</span></span>
      <span begin="00:11.000" end="00:12.000">の<span ttm:role="x-roman">no</span></span>
    </p>`);
    expect(lines[0].words?.[0]?.roman).toBe("kimi");
    expect(lines[0].words?.[1]?.roman).toBe("no");
    expect(lines[0].roman).toBe("kimi no");
  });
});

describe("One Last Kiss catalog TTML", () => {
  test("parses the cached AMLL document when present", () => {
    let raw;
    try {
      raw = readFileSync("/tmp/one-last-kiss.ttml", "utf8");
    } catch {
      return;
    }
    const lines = parseTtml(raw);
    expect(lines.length).toBeGreaterThan(20);
    const first = lines[0];
    expect(first.text).toContain("初");
    expect(first.words?.length).toBeGreaterThan(1);
    expect(first.roman).toBeTruthy();
    const forgotten = lines.find((line) =>
      line.text.includes("忘れられない人"),
    );
    expect(forgotten?.words?.some((word) => word.text.includes("忘"))).toBe(
      true,
    );
    expect(forgotten?.bg_words?.length).toBeGreaterThan(0);
  });
});
