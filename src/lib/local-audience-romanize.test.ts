import { describe, expect, test } from "vitest";
import {
  LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
  LOCAL_AUDIENCE_ROMANIZE_SET_EVENT,
  LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT,
  FULLSCREEN_PLAYER_WINDOW_LABEL,
  MAIN_WINDOW_LABEL,
  buildLyricsIdentity,
} from "./local-audience-romanize";
import type { LyricLine } from "@/types/ipc";

const line = (
  time_ms: number,
  text: string,
  extras: Partial<LyricLine> = {},
): LyricLine => ({
  time_ms,
  text,
  words: extras.words ?? null,
  bg_words: extras.bg_words ?? null,
  section: extras.section ?? null,
});

describe("local-audience-romanize event constants", () => {
  test("uses the openkara:// scheme with focused event names", () => {
    expect(LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT).toBe(
      "openkara://local-audience-romanize-state",
    );
    expect(LOCAL_AUDIENCE_ROMANIZE_SET_EVENT).toBe(
      "openkara://local-audience-romanize-set",
    );
    expect(LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT).toBe(
      "openkara://local-audience-romanize-sync-request",
    );
  });

  test("targets the existing fullscreen-player and main window labels", () => {
    expect(FULLSCREEN_PLAYER_WINDOW_LABEL).toBe("fullscreen-player");
    expect(MAIN_WINDOW_LABEL).toBe("main");
  });
});

describe("buildLyricsIdentity", () => {
  test("returns null for empty lyrics so the receiver treats it as no data", () => {
    expect(buildLyricsIdentity([])).toBeNull();
  });

  test("serializes ordered time_ms and text deterministically", () => {
    const lines = [line(0, "你好"), line(1000, "世界")];
    expect(buildLyricsIdentity(lines)).toBe(
      JSON.stringify([
        [0, "你好"],
        [1000, "世界"],
      ]),
    );
  });

  test("ignores words, bg_words, and section so online-upgraded word timing does not change identity", () => {
    const a = buildLyricsIdentity([
      line(0, "你好", { words: [{ time_ms: 0, end_ms: 500, text: "你" }] }),
    ]);
    const b = buildLyricsIdentity([
      line(0, "你好", { words: [{ time_ms: 0, end_ms: 300, text: "你好" }] }),
    ]);
    expect(a).toBe(b);
  });

  test("distinguishes different text for the same time_ms", () => {
    expect(buildLyricsIdentity([line(0, "A")])).not.toBe(
      buildLyricsIdentity([line(0, "B")]),
    );
  });

  test("distinguishes different time_ms for the same text", () => {
    expect(buildLyricsIdentity([line(0, "A")])).not.toBe(
      buildLyricsIdentity([line(1000, "A")]),
    );
  });

  test("distinguishes different line order", () => {
    expect(buildLyricsIdentity([line(0, "A"), line(1000, "B")])).not.toBe(
      buildLyricsIdentity([line(1000, "B"), line(0, "A")]),
    );
  });
});
