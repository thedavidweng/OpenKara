import { describe, expect, test } from "vitest";
import {
  PREVIEW_LYRICS,
  PREVIEW_SONGS,
  PRIMARY_PREVIEW_SONG_HASH,
} from "./preview-songs";

describe("shared preview catalog", () => {
  test("places One Last Kiss after Earfquake in recently imported order", () => {
    const sorted = [...PREVIEW_SONGS].sort(
      (left, right) => right.imported_at - left.imported_at,
    );
    expect(sorted.map((song) => song.hash).slice(0, 2)).toEqual([
      "earfquake",
      "one-last-kiss",
    ]);
    expect(PRIMARY_PREVIEW_SONG_HASH).toBe("earfquake");
  });

  test("embeds AMLL word-timed lyrics for One Last Kiss", () => {
    const lyrics = PREVIEW_LYRICS["one-last-kiss"];
    expect(lyrics.source).toBe("amll");
    expect(lyrics.raw_lrc.startsWith("<tt")).toBe(true);
    const forgotten = lyrics.lines.find((line) =>
      line.text.includes("忘れられない人"),
    );
    expect(forgotten?.words?.length).toBeGreaterThan(1);
    expect(forgotten?.bg_words?.length).toBeGreaterThan(0);
    expect(forgotten?.roman).toBeTruthy();
  });
});
