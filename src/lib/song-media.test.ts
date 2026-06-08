import { describe, expect, test } from "vitest";
import {
  songCanBeSeparated,
  songHasCdgMedia,
  songSupportsInstrumentalFlag,
} from "./song-media";
import type { Song } from "@/types/ipc";

function makeSong(overrides: Partial<Song> = {}): Song {
  return {
    hash: "song-default",
    file_path: "music/song.mp3",
    audio_source_kind: "original",
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language: null,
    title: "Song",
    artist: null,
    album: null,
    duration_ms: 1000,
    cover_art: null,
    imported_at: 0,
    original_ext: "mp3",
    ...overrides,
  };
}

describe("songHasCdgMedia", () => {
  test("returns false for null", () => {
    expect(songHasCdgMedia(null)).toBe(false);
  });

  test("returns false for undefined", () => {
    expect(songHasCdgMedia(undefined)).toBe(false);
  });

  test("returns true for paired CDG songs", () => {
    expect(
      songHasCdgMedia(
        makeSong({
          cdg_path: "media-g/song-1.cdg",
          media_g_container: "paired",
        }),
      ),
    ).toBe(true);
  });

  test("returns true for media+g zip songs", () => {
    expect(
      songHasCdgMedia(makeSong({ cdg_path: null, media_g_container: "zip" })),
    ).toBe(true);
  });

  test("returns false for audio-only songs without CDG media", () => {
    expect(
      songHasCdgMedia(makeSong({ cdg_path: null, media_g_container: null })),
    ).toBe(false);
  });
});

describe("songSupportsInstrumentalFlag", () => {
  test("returns true for imported songs without Media+G graphics", () => {
    expect(songSupportsInstrumentalFlag(makeSong())).toBe(true);
  });

  test("returns false for CDG songs", () => {
    expect(
      songSupportsInstrumentalFlag(makeSong({ media_g_container: "zip" })),
    ).toBe(false);
  });

  test("returns true for null/undefined songs since they lack CDG media", () => {
    expect(songSupportsInstrumentalFlag(null)).toBe(true);
    expect(songSupportsInstrumentalFlag(undefined)).toBe(true);
  });
});

describe("songCanBeSeparated", () => {
  test("returns true for original audio-only non-instrumental songs", () => {
    expect(songCanBeSeparated(makeSong())).toBe(true);
  });

  test("returns false for songs already marked instrumental", () => {
    expect(songCanBeSeparated(makeSong({ instrumental: true }))).toBe(false);
  });

  test("returns false for CDG songs regardless of other flags", () => {
    expect(
      songCanBeSeparated(
        makeSong({ media_g_container: "zip", instrumental: false }),
      ),
    ).toBe(false);
  });

  test("returns false for non-original audio source kind", () => {
    expect(
      songCanBeSeparated(makeSong({ audio_source_kind: "stems_remote" })),
    ).toBe(false);
  });

  test("returns false for null/undefined songs", () => {
    expect(songCanBeSeparated(null)).toBe(false);
    expect(songCanBeSeparated(undefined)).toBe(false);
  });
});
