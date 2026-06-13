import { beforeEach, describe, expect, test, vi } from "vitest";
import { useLyricsStore } from "./lyrics-store";

const {
  mockFetchLyrics,
  mockFetchLyricsOnline,
  mockSetLyricsOffset,
  mockSaveManualLyrics,
  mockNotifyError,
  mockRomanizeLyricsLines,
} = vi.hoisted(() => ({
  mockFetchLyrics: vi.fn(),
  mockFetchLyricsOnline: vi.fn(),
  mockSetLyricsOffset: vi.fn(),
  mockSaveManualLyrics: vi.fn(),
  mockNotifyError: vi.fn(),
  mockRomanizeLyricsLines: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  fetchLyrics: mockFetchLyrics,
  fetchLyricsOnline: mockFetchLyricsOnline,
  setLyricsOffset: mockSetLyricsOffset,
  saveManualLyrics: mockSaveManualLyrics,
}));

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
}));

vi.mock("@/lib/lyrics-romanizer", () => ({
  romanizeLyricsLines: mockRomanizeLyricsLines,
}));

vi.mock("@/components/Library/song-list-item-menu", () => ({
  SONG_LANGUAGES: ["en", "zh-CN"],
}));

const mockSongs = [
  { hash: "song-1", language: "zh-CN" },
  { hash: "song-2", language: null },
];
vi.mock("@/stores/library-store", () => ({
  useLibraryStore: {
    getState: () => ({ songs: mockSongs }),
    subscribe: vi.fn(),
  },
}));

const DEFAULT_STATE = {
  songId: null,
  lines: [],
  source: null,
  offsetMs: 0,
  rawLrc: "",
  activeLineIndex: -1,
  isLoading: false,
  romanizedLines: [],
  isRomanizing: false,
  showRomanized: false,
};

function resetStore() {
  mockFetchLyrics.mockReset();
  mockFetchLyricsOnline.mockReset();
  mockSetLyricsOffset.mockReset();
  mockSaveManualLyrics.mockReset();
  mockNotifyError.mockReset();
  mockRomanizeLyricsLines.mockReset();
  useLyricsStore.setState(DEFAULT_STATE);
}

// ─── fetchLyrics ────────────────────────────────────────────

describe("lyrics-store fetchLyrics", () => {
  beforeEach(resetStore);

  test("sets songId, lines, source, offsetMs, rawLrc on success", async () => {
    const payload = {
      song_id: "song-1",
      lines: [
        {
          time_ms: 1000,
          text: "Hello",
          words: [],
          bg_words: null,
          section: null,
        },
        {
          time_ms: 2000,
          text: "World",
          words: [],
          bg_words: null,
          section: null,
        },
      ],
      source: "lrc_lib" as const,
      offset_ms: 50,
      raw_lrc: "[00:01.00]Hello\n[00:02.00]World",
    };
    mockFetchLyrics.mockResolvedValue(payload);

    await useLyricsStore.getState().fetchLyrics("song-1");

    const state = useLyricsStore.getState();
    expect(state.songId).toBe("song-1");
    expect(state.lines).toEqual(payload.lines);
    expect(state.source).toBe("lrc_lib");
    expect(state.offsetMs).toBe(50);
    expect(state.rawLrc).toBe(payload.raw_lrc);
    expect(state.isLoading).toBe(false);
    expect(mockNotifyError).not.toHaveBeenCalled();
  });

  test("calls notifyError and clears state on error", async () => {
    const error = new Error("fetch failed");
    mockFetchLyrics.mockRejectedValue(error);

    useLyricsStore.setState({
      lines: [
        { time_ms: 0, text: "old", words: [], bg_words: null, section: null },
      ],
    });

    await useLyricsStore.getState().fetchLyrics("song-1");

    expect(mockNotifyError).toHaveBeenCalledWith(error);
    const state = useLyricsStore.getState();
    expect(state.lines).toEqual([]);
    expect(state.source).toBeNull();
    expect(state.rawLrc).toBe("");
    expect(state.isLoading).toBe(false);
  });

  test("auto-upgrades unsynced lyrics by calling fetchLyricsOnline", async () => {
    const unsynced = {
      song_id: "song-1",
      lines: [
        {
          time_ms: 0,
          text: "Line A",
          words: [],
          bg_words: null,
          section: null,
        },
        {
          time_ms: 0,
          text: "Line B",
          words: [],
          bg_words: null,
          section: null,
        },
      ],
      source: "embedded" as const,
      offset_ms: 0,
      raw_lrc: "Line A\nLine B",
    };
    const synced = {
      song_id: "song-1",
      lines: [
        {
          time_ms: 500,
          text: "Line A",
          words: [],
          bg_words: null,
          section: null,
        },
        {
          time_ms: 1500,
          text: "Line B",
          words: [],
          bg_words: null,
          section: null,
        },
      ],
      source: "lrc_lib" as const,
      offset_ms: 0,
      raw_lrc: "[00:00.50]Line A\n[00:01.50]Line B",
    };
    mockFetchLyrics.mockResolvedValue(unsynced);
    mockFetchLyricsOnline.mockResolvedValue(synced);

    await useLyricsStore.getState().fetchLyrics("song-1");

    expect(mockFetchLyricsOnline).toHaveBeenCalledWith("song-1");
    const state = useLyricsStore.getState();
    expect(state.lines).toEqual(synced.lines);
    expect(state.source).toBe("lrc_lib");
  });

  test("does not auto-upgrade when source is lrc_lib", async () => {
    const synced = {
      song_id: "song-1",
      lines: [
        { time_ms: 0, text: "Solo", words: [], bg_words: null, section: null },
      ],
      source: "lrc_lib" as const,
      offset_ms: 0,
      raw_lrc: "Solo",
    };
    mockFetchLyrics.mockResolvedValue(synced);

    await useLyricsStore.getState().fetchLyrics("song-1");

    expect(mockFetchLyricsOnline).not.toHaveBeenCalled();
  });

  test("does not auto-upgrade when any line has time_ms > 0", async () => {
    const mixed = {
      song_id: "song-1",
      lines: [
        { time_ms: 0, text: "A", words: [], bg_words: null, section: null },
        { time_ms: 1000, text: "B", words: [], bg_words: null, section: null },
      ],
      source: "embedded" as const,
      offset_ms: 0,
      raw_lrc: "A\n[00:01.00]B",
    };
    mockFetchLyrics.mockResolvedValue(mixed);

    await useLyricsStore.getState().fetchLyrics("song-1");

    expect(mockFetchLyricsOnline).not.toHaveBeenCalled();
  });

  test("keeps local lyrics when fetchLyricsOnline fails", async () => {
    const unsynced = {
      song_id: "song-1",
      lines: [
        { time_ms: 0, text: "Local", words: [], bg_words: null, section: null },
      ],
      source: "embedded" as const,
      offset_ms: 0,
      raw_lrc: "Local",
    };
    mockFetchLyrics.mockResolvedValue(unsynced);
    mockFetchLyricsOnline.mockRejectedValue(new Error("offline"));

    await useLyricsStore.getState().fetchLyrics("song-1");

    expect(useLyricsStore.getState().lines).toEqual(unsynced.lines);
    expect(useLyricsStore.getState().source).toBe("embedded");
  });

  test("stale fetch result does not overwrite current lyrics (F1 race guard)", async () => {
    // Song A responds slowly; Song B responds immediately.
    // After both settle, the store must hold Song B's lyrics.
    let resolveA: (v: unknown) => void;
    const promiseA = new Promise((resolve) => {
      resolveA = resolve;
    });
    mockFetchLyrics.mockImplementation(async (id: string) => {
      if (id === "song-A") return promiseA;
      return {
        song_id: "song-B",
        lines: [
          {
            time_ms: 1000,
            text: "B line",
            words: [],
            bg_words: null,
            section: null,
          },
        ],
        source: "lrc_lib" as const,
        offset_ms: 0,
        raw_lrc: "[00:01.00]B line",
      };
    });

    // Start fetching song A (will hang)
    const fetchAPromise = useLyricsStore.getState().fetchLyrics("song-A");

    // Start fetching song B (resolves immediately)
    await useLyricsStore.getState().fetchLyrics("song-B");

    // Store should now hold song B's lyrics
    expect(useLyricsStore.getState().songId).toBe("song-B");
    expect(useLyricsStore.getState().lines[0]?.text).toBe("B line");

    // Now let song A's response arrive late
    resolveA!({
      song_id: "song-A",
      lines: [
        {
          time_ms: 500,
          text: "A stale line",
          words: [],
          bg_words: null,
          section: null,
        },
      ],
      source: "embedded" as const,
      offset_ms: 0,
      raw_lrc: "[00:00.50]A stale line",
    });
    await fetchAPromise;

    // Song A's stale result must NOT overwrite song B
    expect(useLyricsStore.getState().songId).toBe("song-B");
    expect(useLyricsStore.getState().lines[0]?.text).toBe("B line");
  });

  test("does not auto-upgrade if songId changed during fetch", async () => {
    const unsynced = {
      song_id: "song-1",
      lines: [
        { time_ms: 0, text: "A", words: [], bg_words: null, section: null },
      ],
      source: "embedded" as const,
      offset_ms: 0,
      raw_lrc: "A",
    };
    mockFetchLyrics.mockResolvedValue(unsynced);
    mockFetchLyricsOnline.mockImplementation(async () => {
      // Simulate another fetch changing the active song while online resolves
      useLyricsStore.setState({ songId: "song-2" });
      return {
        song_id: "song-1",
        lines: [
          { time_ms: 500, text: "A", words: [], bg_words: null, section: null },
        ],
        source: "lrc_lib" as const,
        offset_ms: 0,
        raw_lrc: "[00:00.50]A",
      };
    });

    await useLyricsStore.getState().fetchLyrics("song-1");

    // The online result should NOT be applied because songId no longer matches
    expect(useLyricsStore.getState().lines).toEqual(unsynced.lines);
  });
});

// ─── setOffset ──────────────────────────────────────────────

describe("lyrics-store setOffset", () => {
  beforeEach(resetStore);

  test("calls api.setLyricsOffset and updates offsetMs", async () => {
    mockSetLyricsOffset.mockResolvedValue(undefined);

    await useLyricsStore.getState().setOffset("song-1", 200);

    expect(mockSetLyricsOffset).toHaveBeenCalledWith("song-1", 200);
    expect(useLyricsStore.getState().offsetMs).toBe(200);
  });
});

// ─── adjustOffset ───────────────────────────────────────────

describe("lyrics-store adjustOffset", () => {
  beforeEach(resetStore);

  test("adds delta to current offset and calls api.setLyricsOffset", async () => {
    useLyricsStore.setState({ offsetMs: 100 });
    mockSetLyricsOffset.mockResolvedValue(undefined);

    await useLyricsStore.getState().adjustOffset("song-1", 50);

    expect(mockSetLyricsOffset).toHaveBeenCalledWith("song-1", 150);
    expect(useLyricsStore.getState().offsetMs).toBe(150);
  });

  test("works with negative delta", async () => {
    useLyricsStore.setState({ offsetMs: 100 });
    mockSetLyricsOffset.mockResolvedValue(undefined);

    await useLyricsStore.getState().adjustOffset("song-1", -30);

    expect(mockSetLyricsOffset).toHaveBeenCalledWith("song-1", 70);
    expect(useLyricsStore.getState().offsetMs).toBe(70);
  });
});

// ─── setActiveLineIndex ─────────────────────────────────────

describe("lyrics-store setActiveLineIndex", () => {
  beforeEach(resetStore);

  test("updates when index is different", () => {
    useLyricsStore.getState().setActiveLineIndex(3);
    expect(useLyricsStore.getState().activeLineIndex).toBe(3);
  });

  test("no-op when index is the same", () => {
    useLyricsStore.setState({ activeLineIndex: 3 });
    const before = useLyricsStore.getState();

    useLyricsStore.getState().setActiveLineIndex(3);

    expect(useLyricsStore.getState().activeLineIndex).toBe(3);
    // State reference should be unchanged (no new set call)
    expect(useLyricsStore.getState()).toBe(before);
  });
});

// ─── clear ──────────────────────────────────────────────────

describe("lyrics-store clear", () => {
  beforeEach(resetStore);

  test("resets all fields to defaults", () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        {
          time_ms: 1000,
          text: "Hello",
          words: [],
          bg_words: null,
          section: null,
        },
      ],
      source: "lrc_lib",
      offsetMs: 50,
      rawLrc: "[00:01.00]Hello",
      activeLineIndex: 2,
      romanizedLines: ["ni hao"],
      isRomanizing: true,
      showRomanized: true,
    });

    useLyricsStore.getState().clear();

    const state = useLyricsStore.getState();
    expect(state.songId).toBeNull();
    expect(state.lines).toEqual([]);
    expect(state.source).toBeNull();
    expect(state.offsetMs).toBe(0);
    expect(state.rawLrc).toBe("");
    expect(state.activeLineIndex).toBe(-1);
    expect(state.romanizedLines).toEqual([]);
    expect(state.isRomanizing).toBe(false);
    expect(state.showRomanized).toBe(false);
  });
});

// ─── romanizeCurrentLyrics ──────────────────────────────────

describe("lyrics-store romanizeCurrentLyrics", () => {
  beforeEach(resetStore);

  test("sets romanizedLines on success", async () => {
    mockRomanizeLyricsLines.mockResolvedValue({
      result: ["ni hao", "shi jie"],
      requestId: 1,
    });
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
        {
          time_ms: 1000,
          text: "世界",
          words: [],
          bg_words: null,
          section: null,
        },
      ],
    });

    await useLyricsStore.getState().romanizeCurrentLyrics();

    expect(mockRomanizeLyricsLines).toHaveBeenCalledWith(
      ["你好", "世界"],
      "zh-CN",
    );
    expect(useLyricsStore.getState().romanizedLines).toEqual([
      "ni hao",
      "shi jie",
    ]);
    expect(useLyricsStore.getState().isRomanizing).toBe(false);
  });

  test("sets romanizedLines to [] on error", async () => {
    mockRomanizeLyricsLines.mockRejectedValue(new Error("romanize failed"));
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
    });

    await useLyricsStore.getState().romanizeCurrentLyrics();

    expect(useLyricsStore.getState().romanizedLines).toEqual([]);
    expect(useLyricsStore.getState().isRomanizing).toBe(false);
  });

  test("no-op when already romanizing", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      isRomanizing: true,
    });

    await useLyricsStore.getState().romanizeCurrentLyrics();

    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });

  test("no-op when lines are empty", async () => {
    useLyricsStore.setState({ songId: "song-1", lines: [] });

    await useLyricsStore.getState().romanizeCurrentLyrics();

    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });

  test("passes null language when song has no matching language", async () => {
    mockRomanizeLyricsLines.mockResolvedValue({ result: ["yo"], requestId: 2 });
    useLyricsStore.setState({
      songId: "song-2",
      lines: [
        { time_ms: 0, text: "Hey", words: [], bg_words: null, section: null },
      ],
    });

    await useLyricsStore.getState().romanizeCurrentLyrics();

    expect(mockRomanizeLyricsLines).toHaveBeenCalledWith(["Hey"], null);
  });
});

// ─── toggleRomanized ────────────────────────────────────────

describe("lyrics-store toggleRomanized", () => {
  beforeEach(resetStore);

  test("toggles on and triggers romanization", async () => {
    mockRomanizeLyricsLines.mockResolvedValue({
      result: ["ni hao"],
      requestId: 3,
    });
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      showRomanized: false,
    });

    useLyricsStore.getState().toggleRomanized();

    expect(useLyricsStore.getState().showRomanized).toBe(true);
    await vi.waitFor(() =>
      expect(useLyricsStore.getState().romanizedLines).toEqual(["ni hao"]),
    );
  });

  test("toggles off when showRomanized is true", () => {
    useLyricsStore.setState({
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      showRomanized: true,
      romanizedLines: ["ni hao"],
    });

    useLyricsStore.getState().toggleRomanized();

    expect(useLyricsStore.getState().showRomanized).toBe(false);
    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });

  test("no-op when lines are empty", () => {
    useLyricsStore.setState({ lines: [], showRomanized: false });

    useLyricsStore.getState().toggleRomanized();

    expect(useLyricsStore.getState().showRomanized).toBe(false);
    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });
});

// ─── saveManualLyrics ───────────────────────────────────────

describe("lyrics-store saveManualLyrics", () => {
  beforeEach(resetStore);

  test("returns true and updates lyrics on save success", async () => {
    mockSaveManualLyrics.mockResolvedValue({
      song_id: "song-1",
      lines: [{ time_ms: 0, text: "Updated" }],
      source: "manual",
      offset_ms: 120,
      raw_lrc: "[00:00.00]Updated",
    });

    const result = await useLyricsStore
      .getState()
      .saveManualLyrics("song-1", "[00:00.00]Updated");

    expect(result).toBe(true);
    expect(useLyricsStore.getState()).toMatchObject({
      songId: "song-1",
      source: "manual",
      offsetMs: 120,
      rawLrc: "[00:00.00]Updated",
    });
    expect(mockNotifyError).not.toHaveBeenCalled();
  });

  test("returns false and keeps current lyrics when save fails", async () => {
    useLyricsStore.setState({ songId: "song-1", rawLrc: "[00:00.00]Original" });
    const error = new Error("Lyrics save failed");
    mockSaveManualLyrics.mockRejectedValue(error);

    const result = await useLyricsStore
      .getState()
      .saveManualLyrics("song-1", "[00:00.00]Updated");

    expect(result).toBe(false);
    expect(useLyricsStore.getState()).toMatchObject({
      songId: "song-1",
      rawLrc: "[00:00.00]Original",
    });
    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });
});
