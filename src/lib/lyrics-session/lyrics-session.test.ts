import { beforeEach, describe, expect, test, vi } from "vitest";
import { createMockBackend } from "@/lib/backend/mock-backend";
import {
  createTestLyricsSession,
  type TestLyricsSession,
} from "@/test-utils/lyrics-session";
import type { LyricLine, LyricsPayload, LyricsSource } from "@/types/ipc";

function line(
  timeMs: number,
  text: string,
  words: LyricLine["words"] = [],
): LyricLine {
  return { time_ms: timeMs, text, words, bg_words: null, section: null };
}

function payload(input: {
  songId?: string;
  lines: LyricLine[];
  source?: LyricsSource | null;
  offsetMs?: number;
  rawLrc?: string;
}): LyricsPayload {
  return {
    song_id: input.songId ?? "song-1",
    lines: input.lines,
    source: input.source ?? "embedded",
    offset_ms: input.offsetMs ?? 0,
    raw_lrc: input.rawLrc ?? "raw",
  };
}

interface Harness extends TestLyricsSession {
  lyrics: {
    fetchLyrics: ReturnType<typeof vi.fn>;
    fetchLyricsOnline: ReturnType<typeof vi.fn>;
    setLyricsOffset: ReturnType<typeof vi.fn>;
    saveManualLyrics: ReturnType<typeof vi.fn>;
  };
}

function setup(): Harness {
  const lyrics = {
    fetchLyrics: vi.fn(),
    fetchLyricsOnline: vi
      .fn()
      .mockResolvedValue(payload({ lines: [], source: null })),
    setLyricsOffset: vi.fn().mockResolvedValue(undefined),
    saveManualLyrics: vi.fn(),
  };
  const backend = createMockBackend({ overrides: { lyrics } });
  return { ...createTestLyricsSession({ backend }), lyrics };
}

async function load(harness: Harness, next: LyricsPayload): Promise<void> {
  harness.lyrics.fetchLyrics.mockResolvedValue(next);
  await harness.session.load(next.song_id);
}

const TIMED_LINES = [
  line(0, "zero"),
  line(1000, "one"),
  line(2000, "two"),
  line(3000, "three"),
];

describe("LyricsSession.load", () => {
  let harness: Harness;
  beforeEach(() => {
    harness = setup();
  });

  test("publishes the acquired lyrics, their source, and the stored offset", async () => {
    await load(
      harness,
      payload({
        lines: [line(1000, "Hello"), line(2000, "World")],
        source: "lrc_lib",
        offsetMs: 50,
        rawLrc: "[00:01.00]Hello\n[00:02.00]World",
      }),
    );

    const state = harness.session.getState();
    expect(state.songId).toBe("song-1");
    expect(state.lines.map((l) => l.text)).toEqual(["Hello", "World"]);
    expect(state.source).toBe("lrc_lib");
    expect(state.offsetMs).toBe(50);
    expect(state.rawLrc).toBe("[00:01.00]Hello\n[00:02.00]World");
    expect(state.isLoading).toBe(false);
    expect(harness.errors).toEqual([]);
  });

  test("keeps interleaved romaji out of the lyric list and off screen", async () => {
    await load(
      harness,
      payload({
        lines: [
          line(850, "どうでもいいような 夜だけど"),
          line(850, "doudemoiiyouna yorudakedo"),
          line(4850, "響めき 煌めきと君も"),
          line(4850, "kyoumeki koumekitokunmo"),
          line(25810, "まだ止まった 刻む針も"),
          line(25810, "madatomatta kizamuharimo"),
        ],
      }),
    );

    const state = harness.session.getState();
    expect(state.lines.map((l) => l.text)).toEqual([
      "どうでもいいような 夜だけど",
      "響めき 煌めきと君も",
      "まだ止まった 刻む針も",
    ]);
    expect(state.romanizedLines).toEqual([
      "doudemoiiyouna yorudakedo",
      "kyoumeki koumekitokunmo",
      "madatomatta kizamuharimo",
    ]);
    expect(state.showRomanized).toBe(false);

    harness.session.setRomanizedVisibility(true);
    expect(harness.session.getState().showRomanized).toBe(true);
    expect(harness.romanization.calls).toEqual([]);
  });

  test("recomputes romanization when the source transcribes only some lines", async () => {
    await load(
      harness,
      payload({
        lines: [
          line(0, "ライン一"),
          line(0, "rain ichi"),
          line(1000, "ライン二"),
          line(1000, "rain ni"),
          line(2000, "ライン三"),
          line(2000, "rain san"),
          line(3000, "ライン四"),
        ],
      }),
    );
    expect(harness.session.getState().romanizedLinesIdentity).toBeNull();

    harness.session.setRomanizedVisibility(true);
    await vi.waitFor(() => expect(harness.romanization.calls).toHaveLength(1));
  });

  test("reports the failure and empties the panel when acquisition throws", async () => {
    await load(harness, payload({ lines: [line(0, "old")] }));

    const error = new Error("fetch failed");
    harness.lyrics.fetchLyrics.mockRejectedValue(error);
    await harness.session.load("song-1");

    expect(harness.errors).toEqual([error]);
    const state = harness.session.getState();
    expect(state.lines).toEqual([]);
    expect(state.source).toBeNull();
    expect(state.rawLrc).toBe("");
    expect(state.isLoading).toBe(false);
  });
});

describe("LyricsSession automatic upgrade", () => {
  let harness: Harness;
  beforeEach(() => {
    harness = setup();
  });

  const unsynced = payload({
    lines: [line(0, "Line A"), line(0, "Line B")],
    source: "embedded",
    rawLrc: "Line A\nLine B",
  });
  const synced = payload({
    lines: [line(500, "Line A"), line(1500, "Line B")],
    source: "lrc_lib",
    rawLrc: "[00:00.50]Line A\n[00:01.50]Line B",
  });

  test("replaces unsynced lyrics with a timed online result", async () => {
    harness.lyrics.fetchLyricsOnline.mockResolvedValue(synced);

    await load(harness, unsynced);

    expect(harness.lyrics.fetchLyricsOnline).toHaveBeenCalledWith(
      "song-1",
      "automatic_upgrade",
    );
    const state = harness.session.getState();
    expect(state.lines.map((l) => l.time_ms)).toEqual([500, 1500]);
    expect(state.source).toBe("lrc_lib");
  });

  test.each([
    "manual",
    "manual_ttml",
    "manual_lys",
    "sidecar",
    "sidecar_ttml",
    "sidecar_lys",
  ] as const)("never overwrites %s lyrics", async (source) => {
    const protectedPayload = payload({
      lines: [line(0, "Hand written")],
      source,
      rawLrc: "Hand written",
    });

    await load(harness, protectedPayload);

    expect(harness.lyrics.fetchLyricsOnline).not.toHaveBeenCalled();
    expect(harness.session.getState().source).toBe(source);
    expect(harness.session.getState().lines.map((l) => l.text)).toEqual([
      "Hand written",
    ]);
  });

  test("skips lyrics that already came from the online source", async () => {
    await load(
      harness,
      payload({ lines: [line(0, "Solo")], source: "lrc_lib" }),
    );

    expect(harness.lyrics.fetchLyricsOnline).not.toHaveBeenCalled();
  });

  test("skips lyrics that carry any timing at all", async () => {
    await load(
      harness,
      payload({ lines: [line(0, "A"), line(1000, "B")], source: "embedded" }),
    );

    expect(harness.lyrics.fetchLyricsOnline).not.toHaveBeenCalled();
  });

  test("keeps the local lyrics when the online lookup fails", async () => {
    harness.lyrics.fetchLyricsOnline.mockRejectedValue(new Error("offline"));

    await load(harness, unsynced);

    expect(harness.session.getState().lines.map((l) => l.text)).toEqual([
      "Line A",
      "Line B",
    ]);
    expect(harness.session.getState().source).toBe("embedded");
    expect(harness.errors).toEqual([]);
  });

  test("keeps the local lyrics when the online result is also unsynced", async () => {
    harness.lyrics.fetchLyricsOnline.mockResolvedValue(
      payload({ lines: [line(0, "Line A")], source: "lrc_lib" }),
    );

    await load(harness, unsynced);

    expect(harness.session.getState().source).toBe("embedded");
  });

  test("drops the upgrade when the song went away while it was in flight", async () => {
    harness.lyrics.fetchLyricsOnline.mockImplementation(async () => {
      harness.session.clear();
      return synced;
    });

    await load(harness, unsynced);

    expect(harness.session.getState().songId).toBeNull();
    expect(harness.session.getState().lines).toEqual([]);
  });
});

describe("LyricsSession fetch generation", () => {
  test("a late response for the previous song never wins", async () => {
    const harness = setup();
    let releaseFirst: (value: LyricsPayload) => void = () => {};
    const firstResponse = new Promise<LyricsPayload>((resolve) => {
      releaseFirst = resolve;
    });

    harness.lyrics.fetchLyrics.mockImplementation(async (songId: string) =>
      songId === "song-A"
        ? firstResponse
        : payload({
            songId: "song-B",
            lines: [line(1000, "B line")],
            source: "lrc_lib",
          }),
    );

    const slowLoad = harness.session.load("song-A");
    await harness.session.load("song-B");

    expect(harness.session.getState().songId).toBe("song-B");

    releaseFirst(
      payload({
        songId: "song-A",
        lines: [line(500, "A stale line")],
        source: "embedded",
      }),
    );
    await slowLoad;

    expect(harness.session.getState().songId).toBe("song-B");
    expect(harness.session.getState().lines.map((l) => l.text)).toEqual([
      "B line",
    ]);
  });

  test("a late failure for the previous song never clears the current one", async () => {
    const harness = setup();
    let rejectFirst: (error: unknown) => void = () => {};
    const firstResponse = new Promise<LyricsPayload>((_resolve, reject) => {
      rejectFirst = reject;
    });

    harness.lyrics.fetchLyrics.mockImplementation(async (songId: string) =>
      songId === "song-A"
        ? firstResponse
        : payload({
            songId: "song-B",
            lines: [line(1000, "B line")],
            source: "lrc_lib",
          }),
    );

    const slowLoad = harness.session.load("song-A");
    await harness.session.load("song-B");

    rejectFirst(new Error("song-A gave up"));
    await slowLoad;

    expect(harness.session.getState().lines.map((l) => l.text)).toEqual([
      "B line",
    ]);
    expect(harness.errors).toEqual([]);
  });

  test("a late upgrade for the previous song never wins", async () => {
    const harness = setup();
    let releaseUpgrade: (value: LyricsPayload) => void = () => {};
    harness.lyrics.fetchLyricsOnline.mockReturnValue(
      new Promise<LyricsPayload>((resolve) => {
        releaseUpgrade = resolve;
      }),
    );

    harness.lyrics.fetchLyrics.mockImplementation(async (songId: string) =>
      songId === "song-A"
        ? payload({ songId: "song-A", lines: [line(0, "A unsynced")] })
        : payload({
            songId: "song-B",
            lines: [line(1000, "B line")],
            source: "lrc_lib",
          }),
    );

    const slowLoad = harness.session.load("song-A");
    await vi.waitFor(() =>
      expect(harness.lyrics.fetchLyricsOnline).toHaveBeenCalled(),
    );
    await harness.session.load("song-B");

    releaseUpgrade(
      payload({ songId: "song-A", lines: [line(500, "A upgraded")] }),
    );
    await slowLoad;

    expect(harness.session.getState().songId).toBe("song-B");
    expect(harness.session.getState().lines.map((l) => l.text)).toEqual([
      "B line",
    ]);
  });
});

describe("LyricsSession offset", () => {
  let harness: Harness;
  beforeEach(() => {
    harness = setup();
  });

  test("setOffset persists and publishes the new value", async () => {
    await harness.session.setOffset("song-1", 200);

    expect(harness.lyrics.setLyricsOffset).toHaveBeenCalledWith("song-1", 200);
    expect(harness.session.getState().offsetMs).toBe(200);
  });

  test("adjustOffset moves the stored offset by the delta in both directions", async () => {
    await load(harness, payload({ lines: TIMED_LINES, offsetMs: 100 }));

    await harness.session.adjustOffset("song-1", 50);
    expect(harness.lyrics.setLyricsOffset).toHaveBeenLastCalledWith(
      "song-1",
      150,
    );
    expect(harness.session.getState().offsetMs).toBe(150);

    await harness.session.adjustOffset("song-1", -80);
    expect(harness.lyrics.setLyricsOffset).toHaveBeenLastCalledWith(
      "song-1",
      70,
    );
    expect(harness.session.getState().offsetMs).toBe(70);
  });

  test("a rejected write falls back to the offset the backend still holds", async () => {
    await load(harness, payload({ lines: TIMED_LINES, offsetMs: 100 }));
    const error = new Error("write failed");
    harness.lyrics.setLyricsOffset.mockRejectedValue(error);
    harness.lyrics.fetchLyrics.mockResolvedValue(
      payload({ lines: TIMED_LINES, offsetMs: 100 }),
    );

    await harness.session.adjustOffset("song-1", 250);

    expect(harness.session.getState().offsetMs).toBe(100);
    expect(harness.errors).toEqual([error]);
  });

  test("an unreachable backend rewinds the optimistic offset", async () => {
    await load(harness, payload({ lines: TIMED_LINES, offsetMs: 100 }));
    harness.lyrics.setLyricsOffset.mockRejectedValue(new Error("write failed"));
    harness.lyrics.fetchLyrics.mockRejectedValue(new Error("also offline"));

    await harness.session.adjustOffset("song-1", 250);

    expect(harness.session.getState().offsetMs).toBe(100);
  });

  test("resetOffset persists zero and no-ops once already there", async () => {
    await load(harness, payload({ lines: TIMED_LINES, offsetMs: 1500 }));

    await harness.session.resetOffset("song-1");
    expect(harness.lyrics.setLyricsOffset).toHaveBeenCalledWith("song-1", 0);
    expect(harness.session.getState().offsetMs).toBe(0);

    harness.lyrics.setLyricsOffset.mockClear();
    await harness.session.resetOffset("song-1");
    expect(harness.lyrics.setLyricsOffset).not.toHaveBeenCalled();
  });

  test("resetOffset lifts a negative offset back to zero", async () => {
    await load(harness, payload({ lines: TIMED_LINES, offsetMs: -500 }));

    await harness.session.resetOffset("song-1");

    expect(harness.lyrics.setLyricsOffset).toHaveBeenCalledWith("song-1", 0);
    expect(harness.session.getState().offsetMs).toBe(0);
  });

  test("every position the session reports has the offset applied once", async () => {
    await load(harness, payload({ lines: TIMED_LINES, offsetMs: 500 }));

    expect(harness.session.toAdjustedMs(2500)).toBe(2000);
    expect(harness.session.toAdjustedMs(0)).toBe(-500);

    harness.clock.positionMs = 4200;
    expect(harness.session.readPositionMs()).toBe(4200);
    expect(harness.session.toAdjustedMs(harness.session.readPositionMs())).toBe(
      3700,
    );
  });
});

describe("LyricsSession active line derivation", () => {
  let harness: Harness;
  beforeEach(() => {
    harness = setup();
  });

  test("tracks the line under the playhead forwards and backwards", async () => {
    await load(harness, payload({ lines: TIMED_LINES }));

    const observed = [0, 900, 1000, 1999, 2600, 400, 9000].map((positionMs) => {
      harness.session.syncActiveLine(harness.session.toAdjustedMs(positionMs));
      return harness.session.getState().activeLineIndex;
    });

    expect(observed).toEqual([0, 0, 1, 1, 2, 0, 3]);
  });

  test("derives from the offset-corrected position, not the raw clock", async () => {
    await load(harness, payload({ lines: TIMED_LINES, offsetMs: 1000 }));

    harness.session.syncActiveLine(harness.session.toAdjustedMs(1900));
    expect(harness.session.getState().activeLineIndex).toBe(0);

    harness.session.syncActiveLine(harness.session.toAdjustedMs(2100));
    expect(harness.session.getState().activeLineIndex).toBe(1);
  });

  test("only publishes a change when the line actually changes", async () => {
    await load(harness, payload({ lines: TIMED_LINES }));

    let notifications = 0;
    harness.session.subscribe(() => {
      notifications += 1;
    });

    harness.session.syncActiveLine(1200);
    harness.session.syncActiveLine(1300);
    harness.session.syncActiveLine(1400);

    expect(notifications).toBe(1);
    expect(harness.session.getState().activeLineIndex).toBe(1);
  });

  test("stays put when the song has no lyrics to derive from", async () => {
    await load(harness, payload({ lines: [] }));

    harness.session.syncActiveLine(5000);

    expect(harness.session.getState().activeLineIndex).toBe(-1);
  });

  test("tracks the word inside the active line and clears it on lines without words", async () => {
    await load(
      harness,
      payload({
        lines: [
          line(1000, "alpha beta", [
            { text: "alpha", time_ms: 1000, end_ms: 1500 },
            { text: "beta", time_ms: 1500, end_ms: 2000 },
          ]),
          line(2000, "plain"),
        ],
      }),
    );

    harness.session.syncActiveLine(1000);
    harness.session.syncActiveWord(1000);
    expect(harness.session.getState().activeWordIndex).toBe(0);

    harness.session.syncActiveWord(1600);
    expect(harness.session.getState().activeWordIndex).toBe(1);

    harness.session.syncActiveLine(2000);
    harness.session.syncActiveWord(2000);
    expect(harness.session.getState().activeLineIndex).toBe(1);
    expect(harness.session.getState().activeWordIndex).toBe(-1);
  });

  test("re-derives the same index for a new song instead of holding the latch", async () => {
    await load(harness, payload({ songId: "song-A", lines: TIMED_LINES }));
    harness.session.syncActiveLine(2000);
    expect(harness.session.getState().activeLineIndex).toBe(2);

    await load(
      harness,
      payload({
        songId: "song-B",
        lines: [line(0, "b0"), line(500, "b1"), line(900, "b2")],
      }),
    );
    expect(harness.session.getState().activeLineIndex).toBe(-1);

    harness.session.syncActiveLine(2000);
    expect(harness.session.getState().activeLineIndex).toBe(2);
  });
});

describe("LyricsSession romanization", () => {
  let harness: Harness;
  beforeEach(() => {
    harness = setup();
  });

  async function loadChinese(): Promise<void> {
    await load(
      harness,
      payload({ lines: [line(0, "你好"), line(1000, "世界")] }),
    );
  }

  test("transcribes the current lines and caches the result", async () => {
    await loadChinese();
    harness.romanization.respondWith((text) =>
      text === "你好" ? "ni hao" : "shi jie",
    );

    await harness.session.romanizeCurrentLyrics();

    expect(harness.romanization.calls).toEqual([
      { lines: ["你好", "世界"], language: null },
    ]);
    expect(harness.session.getState().romanizedLines).toEqual([
      "ni hao",
      "shi jie",
    ]);
    expect(harness.session.getState().isRomanizing).toBe(false);
  });

  test("passes the catalog language the song is tagged with", async () => {
    const backend = createMockBackend({
      overrides: {
        lyrics: {
          fetchLyrics: vi
            .fn()
            .mockResolvedValue(payload({ lines: [line(0, "你好")] })),
        },
      },
    });
    const tagged = createTestLyricsSession({
      backend,
      songLanguage: { read: (songId) => (songId ? "mandarin" : null) },
    });

    await tagged.session.load("song-1");
    await tagged.session.romanizeCurrentLyrics();

    expect(tagged.romanization.calls[0]?.language).toBe("mandarin");
  });

  test("empties the transcription when romanization throws", async () => {
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    await loadChinese();
    harness.romanization.failWith(new Error("romanize failed"));

    await harness.session.romanizeCurrentLyrics();

    expect(harness.session.getState().romanizedLines).toEqual([]);
    expect(harness.session.getState().romanizedLinesIdentity).toBeNull();
    expect(harness.session.getState().isRomanizing).toBe(false);
    consoleError.mockRestore();
  });

  test("runs one transcription at a time", async () => {
    await loadChinese();

    const first = harness.session.romanizeCurrentLyrics();
    await harness.session.romanizeCurrentLyrics();
    await first;

    expect(harness.romanization.calls).toHaveLength(1);
  });

  test("does nothing without lyrics", async () => {
    await load(harness, payload({ lines: [] }));

    await harness.session.romanizeCurrentLyrics();

    expect(harness.romanization.calls).toEqual([]);
  });

  test("discards a transcription whose song changed under it", async () => {
    await loadChinese();

    const inFlight = harness.session.romanizeCurrentLyrics();
    harness.session.clear();
    await inFlight;

    expect(harness.session.getState().romanizedLines).toEqual([]);
  });

  test("keeps a transcription that never left the caller's turn", async () => {
    await loadChinese();
    harness.romanization.answerWithoutYielding();

    const inFlight = harness.session.romanizeCurrentLyrics();
    harness.session.clear();
    await inFlight;

    expect(harness.session.getState().romanizedLines).toEqual([
      "roman(你好)",
      "roman(世界)",
    ]);
  });

  test("refreshRomanization only re-runs while the overlay is on screen", async () => {
    await loadChinese();

    harness.session.refreshRomanization();
    expect(harness.romanization.calls).toEqual([]);

    harness.session.setRomanizedVisibility(true);
    await vi.waitFor(() => expect(harness.romanization.calls).toHaveLength(1));

    harness.session.refreshRomanization();
    await vi.waitFor(() => expect(harness.romanization.calls).toHaveLength(2));
  });
});

describe("LyricsSession romanization visibility", () => {
  let harness: Harness;
  beforeEach(() => {
    harness = setup();
  });

  async function loadChinese(): Promise<void> {
    await load(harness, payload({ lines: [line(0, "你好")] }));
  }

  test("turning it on starts exactly one transcription", async () => {
    await loadChinese();

    harness.session.setRomanizedVisibility(true);

    expect(harness.session.getState().showRomanized).toBe(true);
    await vi.waitFor(() => expect(harness.romanization.calls).toHaveLength(1));

    harness.session.setRomanizedVisibility(true);
    expect(harness.romanization.calls).toHaveLength(1);
  });

  test("turning it off keeps the cached transcription", async () => {
    await loadChinese();
    harness.session.setRomanizedVisibility(true);
    await vi.waitFor(() =>
      expect(harness.session.getState().romanizedLines).toEqual([
        "roman(你好)",
      ]),
    );

    harness.session.setRomanizedVisibility(false);

    expect(harness.session.getState().showRomanized).toBe(false);
    expect(harness.session.getState().romanizedLines).toEqual(["roman(你好)"]);
  });

  test("turning it back on reuses a transcription that still matches", async () => {
    await loadChinese();
    harness.session.setRomanizedVisibility(true);
    await vi.waitFor(() => expect(harness.romanization.calls).toHaveLength(1));
    harness.session.setRomanizedVisibility(false);

    harness.session.setRomanizedVisibility(true);

    expect(harness.romanization.calls).toHaveLength(1);
  });

  test("turning it back on recomputes once the cache no longer matches", async () => {
    await loadChinese();
    harness.session.setRomanizedVisibility(true);
    await vi.waitFor(() => expect(harness.romanization.calls).toHaveLength(1));

    harness.session.applyRemoteRomanizeState({
      revision: 7,
      songId: "song-1",
      lyricsIdentity: "an identity from other lyrics",
      showRomanized: false,
      isRomanizing: false,
      romanizedLines: ["stale"],
    });

    harness.session.setRomanizedVisibility(true);

    await vi.waitFor(() => expect(harness.romanization.calls).toHaveLength(2));
  });

  test("does nothing without lyrics", () => {
    harness.session.setRomanizedVisibility(true);

    expect(harness.session.getState().showRomanized).toBe(false);
    expect(harness.romanization.calls).toEqual([]);
  });

  test("leaves the song, the lyrics, and the playhead alone", async () => {
    await load(
      harness,
      payload({ lines: [line(0, "你好")], offsetMs: 50, source: "lrc_lib" }),
    );
    harness.session.syncActiveLine(0);

    harness.session.setRomanizedVisibility(true);
    harness.session.setRomanizedVisibility(false);

    const state = harness.session.getState();
    expect(state.songId).toBe("song-1");
    expect(state.lines).toHaveLength(1);
    expect(state.offsetMs).toBe(50);
    expect(state.activeLineIndex).toBe(0);
  });

  test("toggleRomanized flips visibility and is inert without lyrics", async () => {
    harness.session.toggleRomanized();
    expect(harness.session.getState().showRomanized).toBe(false);

    await loadChinese();

    harness.session.toggleRomanized();
    expect(harness.session.getState().showRomanized).toBe(true);
    await vi.waitFor(() => expect(harness.romanization.calls).toHaveLength(1));

    harness.session.toggleRomanized();
    expect(harness.session.getState().showRomanized).toBe(false);
    expect(harness.session.getState().romanizedLines).toEqual(["roman(你好)"]);
  });
});

describe("LyricsSession.applyRemoteRomanizeState", () => {
  let harness: Harness;
  beforeEach(() => {
    harness = setup();
  });

  test("adopts the projected overlay without touching the lyrics", async () => {
    await load(
      harness,
      payload({ lines: [line(0, "你好")], offsetMs: 75, source: "lrc_lib" }),
    );
    harness.session.syncActiveLine(0);

    harness.session.applyRemoteRomanizeState({
      revision: 5,
      songId: "song-1",
      lyricsIdentity: "id",
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["ni hao"],
    });

    const state = harness.session.getState();
    expect(state.showRomanized).toBe(true);
    expect(state.isRomanizing).toBe(false);
    expect(state.romanizedLines).toEqual(["ni hao"]);
    expect(state.songId).toBe("song-1");
    expect(state.lines).toHaveLength(1);
    expect(state.offsetMs).toBe(75);
    expect(state.activeLineIndex).toBe(0);
    expect(state.source).toBe("lrc_lib");
    expect(harness.romanization.calls).toEqual([]);
  });

  test("copies the projected lines so later remote mutation cannot leak in", () => {
    const remote = ["ni hao", "shi jie"];

    harness.session.applyRemoteRomanizeState({
      revision: 5,
      songId: "song-1",
      lyricsIdentity: "id",
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: remote,
    });
    remote.push("mutated");

    expect(harness.session.getState().romanizedLines).toEqual([
      "ni hao",
      "shi jie",
    ]);
  });
});

describe("LyricsSession.saveManualLyrics", () => {
  let harness: Harness;
  beforeEach(() => {
    harness = setup();
  });

  test("publishes the saved lyrics and reports success", async () => {
    harness.lyrics.saveManualLyrics.mockResolvedValue(
      payload({
        lines: [line(0, "Updated")],
        source: "manual",
        offsetMs: 120,
        rawLrc: "[00:00.00]Updated",
      }),
    );

    const saved = await harness.session.saveManualLyrics(
      "song-1",
      "[00:00.00]Updated",
    );

    expect(saved).toBe(true);
    expect(harness.session.getState()).toMatchObject({
      songId: "song-1",
      source: "manual",
      offsetMs: 120,
      rawLrc: "[00:00.00]Updated",
    });
    expect(harness.errors).toEqual([]);
  });

  test("keeps the current lyrics and reports failure when the save is rejected", async () => {
    await load(
      harness,
      payload({ lines: [line(0, "Original")], rawLrc: "[00:00.00]Original" }),
    );
    const error = new Error("Lyrics save failed");
    harness.lyrics.saveManualLyrics.mockRejectedValue(error);

    const saved = await harness.session.saveManualLyrics(
      "song-1",
      "[00:00.00]Updated",
    );

    expect(saved).toBe(false);
    expect(harness.session.getState().rawLrc).toBe("[00:00.00]Original");
    expect(harness.errors).toEqual([error]);
  });
});

describe("LyricsSession.clear", () => {
  test("returns the session to the no-song state", async () => {
    const harness = setup();
    await load(
      harness,
      payload({
        lines: [line(1000, "Hello")],
        source: "lrc_lib",
        offsetMs: 50,
        rawLrc: "[00:01.00]Hello",
      }),
    );
    harness.session.syncActiveLine(1000);
    harness.session.setRomanizedVisibility(true);

    harness.session.clear();

    expect(harness.session.getState()).toMatchObject({
      songId: null,
      lines: [],
      source: null,
      offsetMs: 0,
      rawLrc: "",
      activeLineIndex: -1,
      activeWordIndex: -1,
      romanizedLines: [],
      romanizedLinesIdentity: null,
      isRomanizing: false,
      showRomanized: false,
    });
  });
});

describe("LyricsSession alignment", () => {
  test("switches between left and centered", () => {
    const { session } = setup();
    expect(session.getState().lyricsAlignment).toBe("left");

    session.toggleLyricsAlignment();
    expect(session.getState().lyricsAlignment).toBe("center");

    session.toggleLyricsAlignment();
    expect(session.getState().lyricsAlignment).toBe("left");

    session.setLyricsAlignment("center");
    expect(session.getState().lyricsAlignment).toBe("center");
  });
});

describe("LyricsSession scroll control", () => {
  test("each session owns its own resume generation", () => {
    const first = setup().session;
    const second = setup().session;

    first.scroll.requestResume();
    first.scroll.requestResume();

    expect(first.scroll.peekResumeGeneration()).toBe(2);
    expect(second.scroll.peekResumeGeneration()).toBe(0);
  });

  test("a resume request suppresses unlock until the frame writes scrollTop", () => {
    const { session } = setup();
    expect(session.scroll.isUnlockSuppressed()).toBe(false);

    session.scroll.requestResume();
    expect(session.scroll.isUnlockSuppressed()).toBe(true);

    session.scroll.endUnlockSuppress();
    expect(session.scroll.isUnlockSuppressed()).toBe(false);
  });
});
