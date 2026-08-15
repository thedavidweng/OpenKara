import { describe, expect, test, vi } from "vitest";
import {
  PREVIEW_LYRICS,
  PREVIEW_SONGS,
  PRIMARY_PREVIEW_SONG_HASH,
} from "./preview-songs";
import {
  E2E_MOCK_DATA,
  MOCK_DATA,
  PREVIEW_EARFQUAKE_START_MS,
  PREVIEW_OTHER_SONG_START_MS,
} from "./tauri-mock-data";
import { createTauriMock } from "./tauri-mock-impl";

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

  test("keeps cover art on every preview song", () => {
    for (const song of PREVIEW_SONGS) {
      expect(song.has_cover_art, song.hash).toBe(true);
    }
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

describe("preview playback start", () => {
  test("Earfquake starts at 00:23", async () => {
    const { internals } = createTauriMock(MOCK_DATA);
    const snapshot = (await internals.invoke("play", {
      songId: "earfquake",
    })) as { position_ms: number; song_id: string };
    expect(snapshot.song_id).toBe("earfquake");
    expect(snapshot.position_ms).toBe(PREVIEW_EARFQUAKE_START_MS);
  });

  test("switching songs loads that song's lyrics and demo start", async () => {
    const { internals } = createTauriMock(MOCK_DATA);
    await internals.invoke("play", { songId: "earfquake" });
    const firstLyrics = (await internals.invoke("fetch_lyrics", {
      songId: "earfquake",
    })) as { song_id: string; lines: Array<{ text: string }> };
    expect(firstLyrics.song_id).toBe("earfquake");
    expect(firstLyrics.lines[0]?.text).toContain("For real");

    const switched = (await internals.invoke("play", {
      songId: "one-last-kiss",
    })) as { position_ms: number; song_id: string };
    expect(switched.song_id).toBe("one-last-kiss");
    expect(switched.position_ms).toBe(PREVIEW_OTHER_SONG_START_MS);

    const nextLyrics = (await internals.invoke("fetch_lyrics", {
      songId: "one-last-kiss",
    })) as { song_id: string; lines: Array<{ text: string }> };
    expect(nextLyrics.song_id).toBe("one-last-kiss");
    expect(nextLyrics.lines[0]?.text).toContain("初");
  });

  test("play falls back to loopStartPositionMs when no play start is set", async () => {
    const { internals } = createTauriMock({
      ...E2E_MOCK_DATA,
      playStartPositionMs: undefined,
      playStartPositionBySongId: undefined,
      loopStartPositionMs: 12_000,
    });
    const snapshot = (await internals.invoke("play", {
      songId: "earfquake",
    })) as { position_ms: number };
    expect(snapshot.position_ms).toBe(12_000);
  });

  test("get_cover_art loads JPEG URLs when inline cover bytes are absent", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      arrayBuffer: async () => new Uint8Array([255, 216, 255]).buffer,
    });
    vi.stubGlobal("fetch", fetchMock);
    try {
      const { internals } = createTauriMock({
        ...E2E_MOCK_DATA,
        songs: E2E_MOCK_DATA.songs.map((song) => ({
          ...song,
          cover_art: null,
        })),
        coverArtUrls: { earfquake: "/covers/earfquake.jpg" },
      });
      const bytes = (await internals.invoke("get_cover_art", {
        hash: "earfquake",
      })) as number[];
      expect(fetchMock).toHaveBeenCalledWith("/covers/earfquake.jpg");
      expect(bytes).toEqual([255, 216, 255]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  test("stemsCompleted records a finished four-stem job for every song", async () => {
    const { internals } = createTauriMock({
      ...E2E_MOCK_DATA,
      stemsCompleted: true,
    });
    const statuses = (await internals.invoke(
      "get_all_separation_statuses",
    )) as Array<{
      song_id: string;
      state: string;
      drums_path: string | null;
      vocals_path: string | null;
      bass_path: string | null;
      other_path: string | null;
    }>;
    expect(statuses).toHaveLength(E2E_MOCK_DATA.songs.length);
    for (const status of statuses) {
      expect(status.state).toBe("completed");
      expect(status.drums_path).toContain(status.song_id);
      expect(status.vocals_path).toBeTruthy();
      expect(status.bass_path).toBeTruthy();
      expect(status.other_path).toBeTruthy();
    }
  });

  test("e2e mock play still starts at zero", async () => {
    const { internals } = createTauriMock(E2E_MOCK_DATA);
    const snapshot = (await internals.invoke("play", {
      songId: "earfquake",
    })) as { position_ms: number };
    expect(snapshot.position_ms).toBe(0);
  });

  test("setMockLyrics overrides catalog lyrics for the next fetch", async () => {
    const { internals, helpers } = createTauriMock(E2E_MOCK_DATA);
    helpers.setMockLyrics({
      raw_lrc: "override",
      lines: [
        {
          time_ms: 0,
          text: "Lyric line 0",
          words: null,
          bg_words: null,
          section: null,
          roman: null,
        },
      ],
      offset_ms: 0,
      source: "test",
    });
    const lyrics = (await internals.invoke("fetch_lyrics", {
      songId: "earfquake",
    })) as { lines: Array<{ text: string }> };
    expect(lyrics.lines[0]?.text).toBe("Lyric line 0");
  });
});
