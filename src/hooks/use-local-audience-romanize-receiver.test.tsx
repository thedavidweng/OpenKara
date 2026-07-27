// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, test, vi } from "vitest";
import {
  MAIN_WINDOW_LABEL,
  LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT,
} from "@/lib/local-audience-romanize";
import { useLyricsStore } from "@/stores/lyrics-store";
import { useLocalAudienceRomanizeReceiver } from "./use-local-audience-romanize-receiver";

const { mockListen, mockEmit, mockEmitTo, mockRomanizeLyricsLines } =
  vi.hoisted(() => ({
    mockListen: vi.fn(),
    mockEmit: vi.fn(),
    mockEmitTo: vi.fn(),
    mockRomanizeLyricsLines: vi.fn(),
  }));

vi.mock("@tauri-apps/api/event", () => ({
  listen: mockListen,
  emit: mockEmit,
  emitTo: mockEmitTo,
}));

vi.mock("@/lib/lyrics-romanizer", () => ({
  romanizeLyricsLines: mockRomanizeLyricsLines,
}));

const line = (time_ms: number, text: string) => ({
  time_ms,
  text,
  words: null,
  bg_words: null,
  section: null,
});

function Harness() {
  useLocalAudienceRomanizeReceiver();
  return null;
}

function resetStore() {
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
}

function identity(lines: { time_ms: number; text: string }[]): string {
  return JSON.stringify(lines.map((l) => [l.time_ms, l.text]));
}

describe("useLocalAudienceRomanizeReceiver", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let stateListener:
    | ((event: {
        payload: {
          revision: number;
          songId: string | null;
          lyricsIdentity: string | null;
          showRomanized: boolean;
          isRomanizing: boolean;
          romanizedLines: string[];
        };
      }) => void)
    | null = null;

  beforeEach(() => {
    mockListen.mockReset();
    mockEmit.mockReset();
    mockEmitTo.mockReset();
    mockRomanizeLyricsLines.mockReset();
    mockEmit.mockResolvedValue(undefined);
    mockEmitTo.mockResolvedValue(undefined);
    mockListen.mockImplementation(async (_eventName, listener) => {
      stateListener = listener;
      return vi.fn();
    });
    resetStore();
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    stateListener = null;
  });

  async function renderHarness() {
    await act(async () => {
      root.render(<Harness />);
    });
  }

  function emitState(
    payload: Parameters<NonNullable<typeof stateListener>>[0]["payload"],
  ) {
    if (!stateListener) throw new Error("listener not registered");
    act(() => {
      stateListener!({ payload });
    });
  }

  test("listener registration completes before the initial sync request is emitted", async () => {
    const order: string[] = [];
    mockListen.mockImplementation(async () => {
      order.push("listen-resolved");
      return vi.fn();
    });
    mockEmit.mockImplementation(async () => {
      order.push("sync-emitted");
      return undefined;
    });

    await renderHarness();

    expect(order).toEqual(["listen-resolved", "sync-emitted"]);
  });

  test("matching state applies immediately", () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
    });

    return renderHarness().then(() => {
      emitState({
        revision: 1,
        songId: "song-1",
        lyricsIdentity: identity([line(0, "你好")]),
        showRomanized: true,
        isRomanizing: false,
        romanizedLines: ["ni hao"],
      });

      const state = useLyricsStore.getState();
      expect(state.showRomanized).toBe(true);
      expect(state.romanizedLines).toEqual(["ni hao"]);
    });
  });

  test("state received before local lyrics load is retained and applied after identities match", async () => {
    // No local lyrics yet.
    await renderHarness();

    emitState({
      revision: 1,
      songId: "song-1",
      lyricsIdentity: identity([line(0, "你好")]),
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["ni hao"],
    });

    // Not applied yet.
    expect(useLyricsStore.getState().showRomanized).toBe(false);

    await act(async () => {
      useLyricsStore.setState({
        songId: "song-1",
        lines: [line(0, "你好")],
      });
    });

    expect(useLyricsStore.getState().showRomanized).toBe(true);
    expect(useLyricsStore.getState().romanizedLines).toEqual(["ni hao"]);
  });

  test("a matching song with different lyric content stays pending and is not rendered against the wrong lines", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "本地版本")], // local lyrics differ from the remote
    });

    await renderHarness();

    emitState({
      revision: 1,
      songId: "song-1",
      lyricsIdentity: identity([line(0, "在线版本")]),
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["zai xian ban ben"],
    });

    expect(useLyricsStore.getState().showRomanized).toBe(false);
    expect(useLyricsStore.getState().romanizedLines).toEqual([]);

    // When local lyrics catch up to the remote identity, apply.
    await act(async () => {
      useLyricsStore.setState({
        lines: [line(0, "在线版本")],
      });
    });

    expect(useLyricsStore.getState().showRomanized).toBe(true);
    expect(useLyricsStore.getState().romanizedLines).toEqual([
      "zai xian ban ben",
    ]);
  });

  test("a newer revision replaces an older pending revision", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "本地")],
    });

    await renderHarness();

    // Old pending payload targeting a different identity.
    emitState({
      revision: 1,
      songId: "song-1",
      lyricsIdentity: identity([line(0, "旧在线")]),
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["jiu zai xian"],
    });
    expect(useLyricsStore.getState().showRomanized).toBe(false);

    // Newer revision matches the local identity.
    emitState({
      revision: 2,
      songId: "song-1",
      lyricsIdentity: identity([line(0, "本地")]),
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["ben di"],
    });

    expect(useLyricsStore.getState().showRomanized).toBe(true);
    expect(useLyricsStore.getState().romanizedLines).toEqual(["ben di"]);
  });

  test("an older delayed revision is ignored after a newer state applies", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
    });

    await renderHarness();

    emitState({
      revision: 2,
      songId: "song-1",
      lyricsIdentity: identity([line(0, "你好")]),
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["ni hao v2"],
    });
    expect(useLyricsStore.getState().romanizedLines).toEqual(["ni hao v2"]);

    // Older revision arrives late; must be ignored.
    emitState({
      revision: 1,
      songId: "song-1",
      lyricsIdentity: identity([line(0, "你好")]),
      showRomanized: false,
      isRomanizing: false,
      romanizedLines: ["ni hao v1"],
    });

    expect(useLyricsStore.getState().romanizedLines).toEqual(["ni hao v2"]);
    expect(useLyricsStore.getState().showRomanized).toBe(true);
  });

  test("close/reopen requests and restores the current state without a new button click", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
      showRomanized: true,
      romanizedLines: ["ni hao"],
    });

    await renderHarness();

    // Simulate close: unmount.
    await act(async () => {
      root.unmount();
    });

    // Reopen: fresh receiver instance. The main window answers the sync
    // request with the latest authoritative snapshot.
    stateListener = null;
    mockListen.mockImplementation(async (_n, listener) => {
      stateListener = listener;
      return vi.fn();
    });
    const container2 = document.createElement("div");
    document.body.appendChild(container2);
    const root2 = createRoot(container2);
    await act(async () => {
      root2.render(<Harness />);
    });

    emitState({
      revision: 100,
      songId: "song-1",
      lyricsIdentity: identity([line(0, "你好")]),
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["ni hao"],
    });

    expect(useLyricsStore.getState().showRomanized).toBe(true);
    expect(useLyricsStore.getState().romanizedLines).toEqual(["ni hao"]);

    await act(async () => {
      root2.unmount();
    });
    container2.remove();
  });

  test("the receiver never calls romanizeLyricsLines()", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
    });

    await renderHarness();

    emitState({
      revision: 1,
      songId: "song-1",
      lyricsIdentity: identity([line(0, "你好")]),
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["ni hao"],
    });

    expect(mockRomanizeLyricsLines).not.toHaveBeenCalled();
  });

  test("a payload targeting another song is dropped, not retained", async () => {
    useLyricsStore.setState({
      songId: "song-1",
      lines: [line(0, "你好")],
    });

    await renderHarness();

    emitState({
      revision: 1,
      songId: "song-2",
      lyricsIdentity: identity([line(0, "other")]),
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["other"],
    });

    expect(useLyricsStore.getState().showRomanized).toBe(false);

    await act(async () => {
      useLyricsStore.setState({
        songId: "song-2",
        lines: [line(0, "other")],
      });
    });

    expect(useLyricsStore.getState().showRomanized).toBe(false);
  });

  test("cleanup handles unmount before asynchronous listen() resolution", async () => {
    let resolveListen: (unlisten: () => void) => void;
    const pending = new Promise<() => void>((resolve) => {
      resolveListen = resolve;
    });
    mockListen.mockImplementation(async () => pending);

    await renderHarness();

    await act(async () => {
      root.unmount();
    });

    const unlisten = vi.fn();
    resolveListen!(unlisten);
    await act(async () => {
      await Promise.resolve();
    });

    expect(unlisten).toHaveBeenCalled();
  });

  test("the initial sync request targets the main window via emit", async () => {
    await renderHarness();

    expect(mockEmit).toHaveBeenCalledWith(
      LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT,
      {},
    );
    // emitTo is not used by the receiver's sync request.
    expect(mockEmitTo).not.toHaveBeenCalledWith(
      MAIN_WINDOW_LABEL,
      LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT,
      expect.anything(),
    );
  });
});
