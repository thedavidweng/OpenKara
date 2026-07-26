import { describe, expect, test } from "vitest";
import { songDisplayTitle } from "./song-display";
import type { Song } from "@/types/ipc";

function song(overrides: Partial<Song>): Song {
  return {
    hash: "abc123",
    file_path: null,
    audio_source_kind: "original",
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language: null,
    title: null,
    artist: null,
    album: null,
    duration_ms: 0,
    cover_art: null,
    has_cover_art: false,
    imported_at: 0,
    original_ext: null,
    ...overrides,
  };
}

describe("songDisplayTitle", () => {
  test("prefers the metadata title", () => {
    expect(
      songDisplayTitle(
        song({ title: "Bohemian Rhapsody", file_path: "/m/a.mp3" }),
      ),
    ).toBe("Bohemian Rhapsody");
  });

  test("falls back to the file name when there is no title", () => {
    expect(
      songDisplayTitle(song({ file_path: "/music/karaoke/track.mp3" })),
    ).toBe("track.mp3");
  });

  test("splits Windows paths on the backslash", () => {
    expect(
      songDisplayTitle(song({ file_path: "C:\\Users\\d\\Music\\track.mp3" })),
    ).toBe("track.mp3");
  });

  test("falls back to the hash when there is no title and no path", () => {
    expect(songDisplayTitle(song({}))).toBe("abc123");
  });

  test("returns an empty string for a song that is not in the library", () => {
    expect(songDisplayTitle(undefined)).toBe("");
  });
});
