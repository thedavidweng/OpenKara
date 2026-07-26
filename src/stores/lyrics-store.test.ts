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
  romanizedLinesIdentity: null,
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

  test("keeps interleaved romaji out of the lyric list and off screen until enabled", async () => {
    const jp = (timeMs: number, text: string) => ({
      time_ms: timeMs,
      text,
      words: [],
      bg_words: null,
      section: null,
    });
    mockFetchLyrics.mockResolvedValue({
      song_id: "song-1",
      lines: [
        jp(850, "どうでもいいような 夜だけど"),
        jp(850, "doudemoiiyouna yorudakedo"),
        jp(4850, "響めき 煌めきと君も"),
        jp(4850, "kyoumeki koumekitokunmo"),
        jp(25810, "まだ止まった 刻む針も"),
        jp(25810, "madatomatta kizamuharimo"),
      ],
      source: "embedded" as const,
      offset_ms: 0,
      raw_lrc: "irrelevant",
    });

    await useLyricsStore.getState().fetchLyrics("song-1");

    const state = useLyricsStore.getState();
    // Transcriptions are no longer peer lyric lines…
    expect(state.lines.map((l) => l.text)).toEqual([
      "どうでもいいような 夜だけど",
      "響めき 煌めきと君も",
      "まだ止まった 刻む針も",
    ]);
    // …they are the romanization overlay, which starts hidden.
    expect(state.romanizedLines).toEqual([
      "doudemoiiyouna yorudakedo",
      "kyoumeki koumekitokunmo",
      "madatomatta kizamuharimo",
    ]);
    expect(state.showRomanized).toBe(false);

    // Enabling the toggle shows the source transcription without paying for a
    // romanizer run, because the cache identity matches the fetched lines.
    useLyricsStore.getState().setRomanizedVisibility(true);
    expect(useLyricsStore.getState().showRomanized).toBe(true);
    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });

  test("recomputes romanization when the source transcribes only some lines", async () => {
    const jp = (timeMs: number, text: string) => ({
      time_ms: timeMs,
      text,
      words: [],
      bg_words: null,
      section: null,
    });
    mockFetchLyrics.mockResolvedValue({
      song_id: "song-1",
      lines: [
        jp(0, "ライン一"),
        jp(0, "rain ichi"),
        jp(1000, "ライン二"),
        jp(1000, "rain ni"),
        jp(2000, "ライン三"),
        jp(2000, "rain san"),
        jp(3000, "ライン四"),
      ],
      source: "embedded" as const,
      offset_ms: 0,
      raw_lrc: "irrelevant",
    });
    mockRomanizeLyricsLines.mockResolvedValue({
      result: ["rain ichi", "rain ni", "rain san", "rain yon"],
      requestId: 1,
    });

    await useLyricsStore.getState().fetchLyrics("song-1");
    expect(useLyricsStore.getState().romanizedLinesIdentity).toBeNull();

    useLyricsStore.getState().setRomanizedVisibility(true);
    await Promise.resolve();
    await Promise.resolve();

    expect(mockRomanizeLyricsLines).toHaveBeenCalled();
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

    expect(mockFetchLyricsOnline).toHaveBeenCalledWith("song-1", false);
    const state = useLyricsStore.getState();
    expect(state.lines).toEqual(synced.lines);
    expect(state.source).toBe("lrc_lib");
  });

  // Issue #203: user-authored/user-provided sources must never trigger the
  // silent auto-upgrade, which could overwrite them with a wrong online match.
  test.each([
    "manual",
    "manual_ttml",
    "manual_lys",
    "sidecar",
    "sidecar_ttml",
    "sidecar_lys",
  ] as const)("does not auto-upgrade when source is %s", async (source) => {
    const unsynced = {
      song_id: "song-1",
      lines: [
        {
          time_ms: 0,
          text: "Hand written",
          words: [],
          bg_words: null,
          section: null,
        },
      ],
      source,
      offset_ms: 0,
      raw_lrc: "Hand written",
    };
    mockFetchLyrics.mockResolvedValue(unsynced);

    await useLyricsStore.getState().fetchLyrics("song-1");

    expect(mockFetchLyricsOnline).not.toHaveBeenCalled();
    const state = useLyricsStore.getState();
    expect(state.lines).toEqual(unsynced.lines);
    expect(state.source).toBe(source);
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

describe("lyrics-store setOffset", () => {
  beforeEach(resetStore);

  test("calls api.setLyricsOffset and updates offsetMs", async () => {
    mockSetLyricsOffset.mockResolvedValue(undefined);

    await useLyricsStore.getState().setOffset("song-1", 200);

    expect(mockSetLyricsOffset).toHaveBeenCalledWith("song-1", 200);
    expect(useLyricsStore.getState().offsetMs).toBe(200);
  });
});

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
    expect(useLyricsStore.getState()).toBe(before);
  });
});

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

describe("lyrics-store setRomanizedVisibility", () => {
  beforeEach(resetStore);

  test("enabling with lyrics sets showRomanized=true and starts one romanization task", async () => {
    mockRomanizeLyricsLines.mockResolvedValue({
      result: ["ni hao"],
      requestId: 10,
    });
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      showRomanized: false,
    });

    useLyricsStore.getState().setRomanizedVisibility(true);

    expect(useLyricsStore.getState().showRomanized).toBe(true);
    await vi.waitFor(() =>
      expect(mockRomanizeLyricsLines).toHaveBeenCalledTimes(1),
    );
  });

  test("enabling when already enabled does not start a duplicate task", () => {
    mockRomanizeLyricsLines.mockResolvedValue({
      result: ["ni hao"],
      requestId: 11,
    });
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      showRomanized: true,
      romanizedLines: ["ni hao"],
    });

    useLyricsStore.getState().setRomanizedVisibility(true);

    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });

  test("enabling with cached romanizedLines does not re-run the Worker", () => {
    mockRomanizeLyricsLines.mockResolvedValue({
      result: ["ni hao"],
      requestId: 12,
    });
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      showRomanized: false,
      romanizedLines: ["ni hao"],
      romanizedLinesIdentity: JSON.stringify([[0, "你好"]]),
    });

    useLyricsStore.getState().setRomanizedVisibility(true);

    expect(useLyricsStore.getState().showRomanized).toBe(true);
    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });

  test("enabling while romanizing does not start a duplicate task", () => {
    mockRomanizeLyricsLines.mockResolvedValue({
      result: ["ni hao"],
      requestId: 13,
    });
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      showRomanized: true,
      isRomanizing: true,
    });

    useLyricsStore.getState().setRomanizedVisibility(true);

    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });

  test("disabling changes visibility without clearing cached romanizedLines", () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      showRomanized: true,
      romanizedLines: ["ni hao"],
    });

    useLyricsStore.getState().setRomanizedVisibility(false);

    expect(useLyricsStore.getState().showRomanized).toBe(false);
    expect(useLyricsStore.getState().romanizedLines).toEqual(["ni hao"]);
    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });

  test("no-op when lines are empty", () => {
    useLyricsStore.setState({ lines: [], showRomanized: false });

    useLyricsStore.getState().setRomanizedVisibility(true);

    expect(useLyricsStore.getState().showRomanized).toBe(false);
    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });

  test("does not alter song, lyric, or playback state", () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      offsetMs: 50,
      activeLineIndex: 2,
      showRomanized: true,
      romanizedLines: ["ni hao"],
    });

    useLyricsStore.getState().setRomanizedVisibility(false);

    const state = useLyricsStore.getState();
    expect(state.songId).toBe("song-1");
    expect(state.lines).toHaveLength(1);
    expect(state.offsetMs).toBe(50);
    expect(state.activeLineIndex).toBe(2);
  });

  test("re-enabling after lines changed recomputes instead of reusing stale cache", async () => {
    // Simulate: romanize on (cache computed for v1), romanize off,
    // edit/upgrade lyrics (lines become v2 without clearing cache),
    // romanize on → must recompute, not reuse v1's romanizedLines.
    mockRomanizeLyricsLines
      .mockResolvedValueOnce({ result: ["ni hao"], requestId: 40 })
      .mockResolvedValueOnce({ result: ["ni hao v2"], requestId: 41 });

    const linesV1 = [
      { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
    ];
    const linesV2 = [
      {
        time_ms: 0,
        text: "你好世界",
        words: [],
        bg_words: null,
        section: null,
      },
    ];

    useLyricsStore.setState({
      songId: "song-1",
      lines: linesV1,
      showRomanized: false,
    });

    // Romanize on → computes for v1.
    useLyricsStore.getState().setRomanizedVisibility(true);
    await vi.waitFor(() =>
      expect(mockRomanizeLyricsLines).toHaveBeenCalledTimes(1),
    );
    await vi.waitFor(() =>
      expect(useLyricsStore.getState().romanizedLines).toEqual(["ni hao"]),
    );

    // Romanize off → cache preserved.
    useLyricsStore.getState().setRomanizedVisibility(false);
    expect(useLyricsStore.getState().romanizedLines).toEqual(["ni hao"]);

    // Edit/upgrade lyrics: lines change to v2 without clearing romanizedLines.
    useLyricsStore.setState({ lines: linesV2 });

    // Romanize on → must recompute because identity no longer matches.
    useLyricsStore.getState().setRomanizedVisibility(true);
    await vi.waitFor(() =>
      expect(mockRomanizeLyricsLines).toHaveBeenCalledTimes(2),
    );
    await vi.waitFor(() =>
      expect(useLyricsStore.getState().romanizedLines).toEqual(["ni hao v2"]),
    );
  });

  test("re-enabling after lines replaced with identical content reuses cache", async () => {
    mockRomanizeLyricsLines.mockResolvedValue({
      result: ["ni hao"],
      requestId: 50,
    });

    const lines = [
      { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
    ];

    useLyricsStore.setState({
      songId: "song-1",
      lines,
      showRomanized: false,
    });

    useLyricsStore.getState().setRomanizedVisibility(true);
    await vi.waitFor(() =>
      expect(mockRomanizeLyricsLines).toHaveBeenCalledTimes(1),
    );

    useLyricsStore.getState().setRomanizedVisibility(false);

    // Replace lines with identical content (e.g. re-fetched same lyrics).
    useLyricsStore.setState({ lines: [...lines] });

    useLyricsStore.getState().setRomanizedVisibility(true);
    // Identity matches → no recompute.
    expect(mockRomanizeLyricsLines).toHaveBeenCalledTimes(1);
  });
});

describe("lyrics-store toggleRomanized delegates to setRomanizedVisibility", () => {
  beforeEach(resetStore);

  test("toggle on delegates to setRomanizedVisibility(true)", async () => {
    mockRomanizeLyricsLines.mockResolvedValue({
      result: ["ni hao"],
      requestId: 20,
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
      expect(mockRomanizeLyricsLines).toHaveBeenCalledTimes(1),
    );
  });

  test("toggle off delegates to setRomanizedVisibility(false) and keeps cache", () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      showRomanized: true,
      romanizedLines: ["ni hao"],
    });

    useLyricsStore.getState().toggleRomanized();

    expect(useLyricsStore.getState().showRomanized).toBe(false);
    expect(useLyricsStore.getState().romanizedLines).toEqual(["ni hao"]);
  });
});

describe("lyrics-store applyRemoteRomanizeState", () => {
  beforeEach(resetStore);

  test("copies projected state into the store", () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      showRomanized: false,
      isRomanizing: false,
      romanizedLines: [],
    });

    useLyricsStore.getState().applyRemoteRomanizeState({
      revision: 5,
      songId: "song-1",
      lyricsIdentity: "id",
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["ni hao", "shi jie"],
    });

    const state = useLyricsStore.getState();
    expect(state.showRomanized).toBe(true);
    expect(state.isRomanizing).toBe(false);
    expect(state.romanizedLines).toEqual(["ni hao", "shi jie"]);
  });

  test("never invokes the romanizer Worker", () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      romanizedLines: [],
    });

    useLyricsStore.getState().applyRemoteRomanizeState({
      revision: 5,
      songId: "song-1",
      lyricsIdentity: "id",
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["ni hao"],
    });

    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });

  test("does not mutate source lyrics, offset, or active indices", () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
      offsetMs: 75,
      activeLineIndex: 3,
      activeWordIndex: 2,
      source: "lrc_lib",
      rawLrc: "[00:00.00]你好",
    });

    useLyricsStore.getState().applyRemoteRomanizeState({
      revision: 5,
      songId: "song-1",
      lyricsIdentity: "id",
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["ni hao"],
    });

    const state = useLyricsStore.getState();
    expect(state.songId).toBe("song-1");
    expect(state.lines).toHaveLength(1);
    expect(state.offsetMs).toBe(75);
    expect(state.activeLineIndex).toBe(3);
    expect(state.activeWordIndex).toBe(2);
    expect(state.source).toBe("lrc_lib");
    expect(state.rawLrc).toBe("[00:00.00]你好");
  });

  test("copies the romanizedLines array so later remote mutation does not leak", () => {
    const remote = ["ni hao", "shi jie"];
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
    });

    useLyricsStore.getState().applyRemoteRomanizeState({
      revision: 5,
      songId: "song-1",
      lyricsIdentity: "id",
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: remote,
    });

    remote.push("mutated");
    expect(useLyricsStore.getState().romanizedLines).toEqual([
      "ni hao",
      "shi jie",
    ]);
  });
});

describe("lyrics-store stale Worker result after song change", () => {
  beforeEach(resetStore);

  test("romanizeCurrentLyrics rejects a stale Worker result when songId changed", async () => {
    mockRomanizeLyricsLines.mockResolvedValue({
      result: ["ni hao"],
      requestId: 30,
    });
    useLyricsStore.setState({
      songId: "song-1",
      lines: [
        { time_ms: 0, text: "你好", words: [], bg_words: null, section: null },
      ],
    });

    const promise = useLyricsStore.getState().romanizeCurrentLyrics();
    // Simulate a song change while the Worker is still running.
    useLyricsStore.setState({ songId: "song-2" });
    await promise;

    expect(useLyricsStore.getState().romanizedLines).toEqual([]);
  });
});
