// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, test, vi } from "vitest";
import {
  LOCAL_AUDIENCE_ROMANIZE_SET_EVENT,
  LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT,
  LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
  FULLSCREEN_PLAYER_WINDOW_LABEL,
} from "@/lib/local-audience-romanize";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import { useLocalAudienceRomanizeRuntime } from "./local-audience-romanize-runtime";

const { mockListen, mockEmitTo } = vi.hoisted(() => ({
  mockListen: vi.fn(),
  mockEmitTo: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: mockListen,
  emitTo: mockEmitTo,
}));

const line = (time_ms: number, text: string) => ({
  time_ms,
  text,
  words: null,
  bg_words: null,
  section: null,
});

function Harness({ enabled = true }: { enabled?: boolean }) {
  useLocalAudienceRomanizeRuntime(enabled);
  return null;
}

function resetStores() {
  useLyricsStore.setState({
    songId: null,
    lines: [],
    source: null,
    offsetMs: 0,
    rawLrc: "",
    activeLineIndex: -1,
    activeWordIndex: -1,
    isLoading: false,
    romanizedLines: [],
    isRomanizing: false,
    showRomanized: false,
  });
  usePlayerStore.setState({
    localAudienceOutputActive: false,
  });
}

describe("useLocalAudienceRomanizeRuntime", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    mockListen.mockReset();
    mockEmitTo.mockReset();
    mockEmitTo.mockResolvedValue(undefined);
    mockListen.mockImplementation(async () => vi.fn());
    resetStores();
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  function renderHarness(enabled = true) {
    return act(async () => {
      root.render(<Harness enabled={enabled} />);
    });
  }

  function collectListeners() {
    const calls = mockListen.mock.calls as unknown as [
      string,
      (event: { payload: unknown }) => void,
    ][];
    return calls;
  }

  function getListener(eventName: string) {
    const call = collectListeners().find(([name]) => name === eventName);
    return call?.[1] ?? null;
  }

  test("emits a full state snapshot when local audience output is active", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
      showRomanized: true,
      romanizedLines: ["ni hao"],
    });
    usePlayerStore.setState({ localAudienceOutputActive: true });

    await renderHarness();

    expect(mockEmitTo).toHaveBeenCalledWith(
      FULLSCREEN_PLAYER_WINDOW_LABEL,
      LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
      expect.objectContaining({
        songId: "song-1",
        showRomanized: true,
        romanizedLines: ["ni hao"],
      }),
    );
  });

  test("does not emit when local audience output is inactive but keeps the latest snapshot for sync requests", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
      showRomanized: true,
      romanizedLines: ["ni hao"],
    });
    usePlayerStore.setState({ localAudienceOutputActive: false });

    await renderHarness();

    // No state emission yet.
    expect(mockEmitTo).not.toHaveBeenCalledWith(
      FULLSCREEN_PLAYER_WINDOW_LABEL,
      LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
      expect.anything(),
    );

    // Sync request listener should answer with the latest snapshot.
    const syncListener = getListener(
      LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT,
    );
    expect(syncListener).not.toBeNull();

    await act(async () => {
      syncListener!({ payload: {} });
    });

    expect(mockEmitTo).toHaveBeenCalledWith(
      FULLSCREEN_PLAYER_WINDOW_LABEL,
      LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
      expect.objectContaining({
        songId: "song-1",
        showRomanized: true,
        romanizedLines: ["ni hao"],
      }),
    );
  });

  test("every emitted snapshot has a strictly increasing revision", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
      showRomanized: false,
    });
    usePlayerStore.setState({ localAudienceOutputActive: true });

    await renderHarness();

    const lastCall = () =>
      mockEmitTo.mock.calls[mockEmitTo.mock.calls.length - 1]?.[2] as {
        revision: number;
      };

    const rev1 = lastCall();

    await act(async () => {
      useLyricsStore.setState({ showRomanized: true });
    });
    const rev2 = lastCall();

    await act(async () => {
      useLyricsStore.setState({ romanizedLines: ["ni hao"] });
    });
    const rev3 = lastCall();

    expect(rev2.revision).toBeGreaterThan(rev1.revision);
    expect(rev3.revision).toBeGreaterThan(rev2.revision);
  });

  test("changes to source lyrics, visibility, loading, or romanized lines each emit a new snapshot", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
    });
    usePlayerStore.setState({ localAudienceOutputActive: true });

    await renderHarness();
    const baseline = mockEmitTo.mock.calls.length;

    await act(async () => {
      useLyricsStore.setState({ lines: [line(0, "你好"), line(1000, "世界")] });
    });
    expect(mockEmitTo.mock.calls.length).toBeGreaterThan(baseline);

    const afterLines = mockEmitTo.mock.calls.length;
    await act(async () => {
      useLyricsStore.setState({ showRomanized: true });
    });
    expect(mockEmitTo.mock.calls.length).toBeGreaterThan(afterLines);

    const afterVisibility = mockEmitTo.mock.calls.length;
    await act(async () => {
      useLyricsStore.setState({ isRomanizing: true });
    });
    expect(mockEmitTo.mock.calls.length).toBeGreaterThan(afterVisibility);

    const afterLoading = mockEmitTo.mock.calls.length;
    await act(async () => {
      useLyricsStore.setState({ romanizedLines: ["ni hao", "shi jie"] });
    });
    expect(mockEmitTo.mock.calls.length).toBeGreaterThan(afterLoading);
  });

  test("a valid explicit set request updates the authoritative store once", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
      showRomanized: false,
    });
    usePlayerStore.setState({ localAudienceOutputActive: true });

    await renderHarness();

    const setListener = getListener(LOCAL_AUDIENCE_ROMANIZE_SET_EVENT);
    expect(setListener).not.toBeNull();

    await act(async () => {
      setListener!({ payload: { songId: "song-1", showRomanized: true } });
    });

    expect(useLyricsStore.getState().showRomanized).toBe(true);
  });

  test("a set request for another song is ignored", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
      showRomanized: false,
    });

    await renderHarness();

    const setListener = getListener(LOCAL_AUDIENCE_ROMANIZE_SET_EVENT);
    await act(async () => {
      setListener!({ payload: { songId: "song-2", showRomanized: true } });
    });

    expect(useLyricsStore.getState().showRomanized).toBe(false);
  });

  test("duplicate set requests for the already requested state do not start duplicate Worker work", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
      showRomanized: true,
      romanizedLines: ["ni hao"],
    });

    await renderHarness();

    const setListener = getListener(LOCAL_AUDIENCE_ROMANIZE_SET_EVENT);
    const before = mockEmitTo.mock.calls.length;
    await act(async () => {
      setListener!({ payload: { songId: "song-1", showRomanized: true } });
    });
    // No state change, no new emission.
    expect(mockEmitTo.mock.calls.length).toBe(before);
  });

  test("emit rejection does not throw into playback or lyrics rendering", async () => {
    mockEmitTo.mockRejectedValueOnce(new Error("fullscreen window closed"));
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
      showRomanized: true,
    });
    usePlayerStore.setState({ localAudienceOutputActive: true });

    await expect(renderHarness()).resolves.toBeUndefined();
  });

  test("cleans up all listeners on unmount", async () => {
    const unlistenA = vi.fn();
    const unlistenB = vi.fn();
    const queue = [unlistenA, unlistenB];
    mockListen.mockImplementation(async () => queue.shift()!);

    await renderHarness();

    await act(async () => {
      root.unmount();
    });

    expect(unlistenA).toHaveBeenCalled();
    expect(unlistenB).toHaveBeenCalled();
    container.remove();
  });

  test("cleans up listeners when unmount races listen() resolution", async () => {
    let resolveListen: (unlisten: () => void) => void;
    const pending = new Promise<() => void>((resolve) => {
      resolveListen = resolve;
    });
    mockListen.mockImplementation(async () => pending);

    await renderHarness();

    // Unmount before listen() resolves.
    await act(async () => {
      root.unmount();
    });

    // Now resolve listen(); the runtime must call the unlisten function
    // immediately so no listener is leaked.
    const unlisten = vi.fn();
    resolveListen!(unlisten);
    await act(async () => {
      await Promise.resolve();
    });

    expect(unlisten).toHaveBeenCalled();
    container.remove();
  });

  test("does not register listeners or emit when disabled", async () => {
    await renderHarness(false);

    expect(mockListen).not.toHaveBeenCalled();
    expect(mockEmitTo).not.toHaveBeenCalled();
  });
});
