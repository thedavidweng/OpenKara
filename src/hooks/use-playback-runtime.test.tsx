// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { usePlayerStore } from "@/stores/player-store";
import { useQueueStore } from "@/stores/queue-store";
import { useLibraryStore } from "@/stores/library-store";
import type {
  PlaybackPositionEvent,
  PlaybackStateSnapshot,
  SeparationCompleteEvent,
  SeparationErrorEvent,
  SeparationProgressEvent,
  TrackTransitionedEvent,
} from "@/types/ipc";

const {
  mockListen,
  mockSetPreloadCandidate,
  mockNotifyError,
  mockNotifySuccess,
  mockNotifyWhenUnfocused,
} = vi.hoisted(() => ({
  mockListen: vi.fn(),
  mockSetPreloadCandidate: vi.fn(),
  mockNotifyError: vi.fn(),
  mockNotifySuccess: vi.fn(),
  mockNotifyWhenUnfocused: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: mockListen,
}));

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    setPreloadCandidate: mockSetPreloadCandidate,
  };
});

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
  notifySuccess: mockNotifySuccess,
}));

vi.mock("@/lib/notifications", () => ({
  notifyWhenUnfocused: mockNotifyWhenUnfocused,
}));

// Track every rendered root so afterEach can unmount them all. Without this,
// earlier tests' roots stay mounted and their hooks react to store mutations
// from later tests (e.g. a still-mounted enabled=true instance calls
// setPreloadCandidate when a later test sets the queue).
const unmountFns: Array<() => void> = [];

afterEach(() => {
  while (unmountFns.length > 0) {
    const unmount = unmountFns.pop()!;
    act(() => {
      unmount();
    });
  }
});

async function renderHook(fn: () => void) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<HookHarness hookFn={fn} />);
  });
  const unmount = () => {
    act(() => {
      root.unmount();
    });
    container.remove();
  };
  unmountFns.push(unmount);
  return unmount;
}

function HookHarness({ hookFn }: { hookFn: () => void }) {
  hookFn();
  return null;
}

describe("use-playback-runtime wiring", () => {
  test("registers upload progress listeners alongside separation listeners", async () => {
    const { default: src } = await import("./use-playback-runtime.ts?raw");

    expect(src).toContain("upload-progress");
    expect(src).toContain("upload-complete");
    expect(src).toContain("upload-error");
    expect(src).toContain("updateUploadStatus");
    expect(src).toContain("clearUploadStatus");
    expect(src).toContain("separation-progress");
  });

  test("applies playback-position snapshots directly without a state refresh fallback", async () => {
    const { default: src } = await import("./use-playback-runtime.ts?raw");

    expect(src).toContain("applyPlaybackPositionEvent");
    expect(src).not.toContain("getPlaybackState: api.getPlaybackState");
  });
});

describe("playback-position listener cleanup when unmount races listen()", () => {
  test("setup function checks cancelled flag after await listen (RED)", async () => {
    const { default: src } = await import("./use-playback-runtime.ts?raw");

    const setupMatch = src.match(
      /unlisten\s*=\s*await\s+listen[\s\S]*?\n\s*\}\s*\n/,
    );
    expect(setupMatch).not.toBeNull();

    const afterListen = src.slice(
      src.indexOf("unlisten = await listen"),
      src.indexOf("void setup()"),
    );
    expect(afterListen).toContain("if (cancelled)");
    expect(afterListen).toContain("unlisten()");
  });
});

describe("usePreloadCandidateEffect", () => {
  beforeEach(() => {
    mockListen.mockReset();
    // Default: listen resolves to a no-op unlisten so useEventSubscriptions
    // cleanup never calls a non-function. Tests needing specific behaviour
    // override this with their own mockImplementation.
    mockListen.mockImplementation(async () => () => {});
    mockSetPreloadCandidate.mockReset();
    mockSetPreloadCandidate.mockResolvedValue(undefined);
    mockNotifyError.mockReset();
    mockNotifySuccess.mockReset();
    mockNotifyWhenUnfocused.mockReset();
    mockNotifyWhenUnfocused.mockResolvedValue(undefined);
    usePlayerStore.setState({
      snapshot: null,
      positionMs: 0,
      localAudienceOutputActive: false,
      airPlayPlainTextPagePending: false,
      airPlayPlainTextPagePendingDirection: null,
      airPlayOutput: {
        active: false,
        audioActive: false,
        routeName: null,
        mode: "idle",
        phase: "idle",
        detail: null,
        displayedPositionMs: null,
        streamGeneration: 0,
        latencyMs: null,
      },
    });
    useQueueStore.setState({ queue: [], playHistory: [], isOpen: false });
  });

  test("calls setPreloadCandidate with the queue head when no song is playing", async () => {
    useQueueStore.setState({ queue: ["song-a", "song-b"] });
    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    expect(mockSetPreloadCandidate).toHaveBeenCalledWith("song-a");
  });

  test("skips the queue head when it is the currently playing song", async () => {
    usePlayerStore.setState({
      snapshot: { song_id: "song-a" } as never,
      positionMs: 0,
      localAudienceOutputActive: false,
      airPlayPlainTextPagePending: false,
      airPlayPlainTextPagePendingDirection: null,
      airPlayOutput: {
        active: false,
        audioActive: false,
        routeName: null,
        mode: "idle",
        phase: "idle",
        detail: null,
        displayedPositionMs: null,
        streamGeneration: 0,
        latencyMs: null,
      },
    });
    useQueueStore.setState({ queue: ["song-a", "song-b"] });
    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    expect(mockSetPreloadCandidate).toHaveBeenCalledWith("song-b");
  });

  test("calls setPreloadCandidate with null when the queue is empty", async () => {
    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    expect(mockSetPreloadCandidate).toHaveBeenCalledWith(null);
  });

  test("does not call setPreloadCandidate when disabled", async () => {
    useQueueStore.setState({ queue: ["song-a"] });
    // Ensure listen returns cleanup functions so other hooks don't throw
    mockListen.mockImplementation(async () => () => {});
    const { useEventListeners } = await import("./use-playback-runtime");
    const unmount = await renderHook(() => useEventListeners(false));

    await act(async () => {});

    expect(mockSetPreloadCandidate).not.toHaveBeenCalled();
    unmount();
  });
});

describe("useTrackTransitionedQueueReconcile", () => {
  beforeEach(() => {
    mockListen.mockReset();
    // Default: listen resolves to a no-op unlisten so useEventSubscriptions
    // cleanup never calls a non-function. Tests needing specific behaviour
    // override this with their own mockImplementation.
    mockListen.mockImplementation(async () => () => {});
    mockSetPreloadCandidate.mockReset();
    mockSetPreloadCandidate.mockResolvedValue(undefined);
    mockNotifyError.mockReset();
    mockNotifySuccess.mockReset();
    mockNotifyWhenUnfocused.mockReset();
    mockNotifyWhenUnfocused.mockResolvedValue(undefined);
    usePlayerStore.setState({
      snapshot: null,
      positionMs: 0,
      localAudienceOutputActive: false,
      airPlayPlainTextPagePending: false,
      airPlayPlainTextPagePendingDirection: null,
      airPlayOutput: {
        active: false,
        audioActive: false,
        routeName: null,
        mode: "idle",
        phase: "idle",
        detail: null,
        displayedPositionMs: null,
        streamGeneration: 0,
        latencyMs: null,
      },
    });
    useQueueStore.setState({ queue: [], playHistory: [], isOpen: false });
  });

  test("forwards track-transitioned events to onTrackTransitioned", async () => {
    const onTrackTransitioned = vi.fn();
    usePlayerStore.setState({
      snapshot: null,
      positionMs: 0,
      localAudienceOutputActive: false,
      airPlayPlainTextPagePending: false,
      airPlayPlainTextPagePendingDirection: null,
      airPlayOutput: {
        active: false,
        audioActive: false,
        routeName: null,
        mode: "idle",
        phase: "idle",
        detail: null,
        displayedPositionMs: null,
        streamGeneration: 0,
        latencyMs: null,
      },
      onTrackTransitioned,
    });

    // Stub listen to capture handlers for all events
    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    const handler = listeners.get("track-transitioned");
    expect(handler).not.toBeUndefined();

    const event: TrackTransitionedEvent = {
      transition_serial: 1,
      from_song_id: "song-a",
      to_song_id: "song-b",
    };
    handler!({ payload: event });

    expect(onTrackTransitioned).toHaveBeenCalledWith("song-a", "song-b");
  });
});

describe("usePlaybackPositionSubscription", () => {
  beforeEach(() => {
    mockListen.mockReset();
    // Default: listen resolves to a no-op unlisten so useEventSubscriptions
    // cleanup never calls a non-function. Tests needing specific behaviour
    // override this with their own mockImplementation.
    mockListen.mockImplementation(async () => () => {});
    mockSetPreloadCandidate.mockReset();
    mockSetPreloadCandidate.mockResolvedValue(undefined);
    mockNotifyError.mockReset();
    mockNotifySuccess.mockReset();
    mockNotifyWhenUnfocused.mockReset();
    mockNotifyWhenUnfocused.mockResolvedValue(undefined);
    usePlayerStore.setState({
      snapshot: null,
      positionMs: 0,
      localAudienceOutputActive: false,
      airPlayPlainTextPagePending: false,
      airPlayPlainTextPagePendingDirection: null,
      airPlayOutput: {
        active: false,
        audioActive: false,
        routeName: null,
        mode: "idle",
        phase: "idle",
        detail: null,
        displayedPositionMs: null,
        streamGeneration: 0,
        latencyMs: null,
      },
    });
    useQueueStore.setState({ queue: [], playHistory: [], isOpen: false });
  });

  test("forwards playback-position events to applyPlaybackPositionEvent", async () => {
    const applyPlaybackPositionEvent = vi.fn();
    usePlayerStore.setState({
      snapshot: null,
      positionMs: 0,
      localAudienceOutputActive: false,
      airPlayPlainTextPagePending: false,
      airPlayPlainTextPagePendingDirection: null,
      airPlayOutput: {
        active: false,
        audioActive: false,
        routeName: null,
        mode: "idle",
        phase: "idle",
        detail: null,
        displayedPositionMs: null,
        streamGeneration: 0,
        latencyMs: null,
      },
      applyPlaybackPositionEvent,
    });

    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    const handler = listeners.get("playback-position");
    expect(handler).not.toBeUndefined();

    const snapshot: PlaybackStateSnapshot = {
      song_id: "song-a",
      transport_generation: 1,
      state: "playing",
      is_playing: true,
      position_ms: 5000,
      duration_ms: 10000,
      buffered_ms: 8000,
      volume: 1,
      stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
      has_stems: false,
      stem_mode: null,
    };
    const event: PlaybackPositionEvent = {
      ms: 5000,
      transport_generation: 1,
      snapshot,
    };
    handler!({ payload: event });

    expect(applyPlaybackPositionEvent).toHaveBeenCalledWith(event);
  });

  test("cleans up the listener when cancelled before listen resolves", async () => {
    const unlisten = vi.fn();
    let resolveListen: ((un: () => void) => void) | null = null;
    // Only the "playback-position" event should hang; all other listen
    // calls should resolve immediately with a cleanup function so that
    // useEventSubscriptions doesn't throw during unmount.
    mockListen.mockImplementation(async (eventName: string) => {
      if (eventName === "playback-position") {
        return new Promise((resolve) => {
          resolveListen = resolve;
        });
      }
      return () => {};
    });

    const { useEventListeners } = await import("./use-playback-runtime");
    const unmount = await renderHook(() => useEventListeners(true));

    // Unmount before listen resolves — the cancelled flag should be set
    unmount();

    await act(async () => {
      resolveListen!(unlisten);
    });

    expect(unlisten).toHaveBeenCalled();
  });
});

describe("useSeparationEvents", () => {
  beforeEach(() => {
    mockListen.mockReset();
    // Default: listen resolves to a no-op unlisten so useEventSubscriptions
    // cleanup never calls a non-function. Tests needing specific behaviour
    // override this with their own mockImplementation.
    mockListen.mockImplementation(async () => () => {});
    mockSetPreloadCandidate.mockReset();
    mockSetPreloadCandidate.mockResolvedValue(undefined);
    mockNotifyError.mockReset();
    mockNotifySuccess.mockReset();
    mockNotifyWhenUnfocused.mockReset();
    mockNotifyWhenUnfocused.mockResolvedValue(undefined);
    usePlayerStore.setState({
      snapshot: null,
      positionMs: 0,
      localAudienceOutputActive: false,
      airPlayPlainTextPagePending: false,
      airPlayPlainTextPagePendingDirection: null,
      airPlayOutput: {
        active: false,
        audioActive: false,
        routeName: null,
        mode: "idle",
        phase: "idle",
        detail: null,
        displayedPositionMs: null,
        streamGeneration: 0,
        latencyMs: null,
      },
    });
    useQueueStore.setState({ queue: [], playHistory: [], isOpen: false });
  });

  test("loads stems when separation-complete matches the current song", async () => {
    const loadStems = vi.fn().mockResolvedValue(undefined);
    const updateSeparationStatus = vi.fn();
    usePlayerStore.setState({
      snapshot: { song_id: "song-a" } as never,
      positionMs: 0,
      localAudienceOutputActive: false,
      airPlayPlainTextPagePending: false,
      airPlayPlainTextPagePendingDirection: null,
      airPlayOutput: {
        active: false,
        audioActive: false,
        routeName: null,
        mode: "idle",
        phase: "idle",
        detail: null,
        displayedPositionMs: null,
        streamGeneration: 0,
        latencyMs: null,
      },
      loadStems,
    });
    useLibraryStore.setState({ updateSeparationStatus } as never);

    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    const handler = listeners.get("separation-complete");
    expect(handler).not.toBeUndefined();

    const event: SeparationCompleteEvent = {
      song_id: "song-a",
      status: { state: "complete", progress: 1 },
    } as never;
    handler!({ payload: event });

    expect(updateSeparationStatus).toHaveBeenCalled();
    expect(loadStems).toHaveBeenCalled();
  });

  test("does not load stems when separation-complete is for a different song", async () => {
    const loadStems = vi.fn().mockResolvedValue(undefined);
    const updateSeparationStatus = vi.fn();
    usePlayerStore.setState({
      snapshot: { song_id: "song-a" } as never,
      positionMs: 0,
      localAudienceOutputActive: false,
      airPlayPlainTextPagePending: false,
      airPlayPlainTextPagePendingDirection: null,
      airPlayOutput: {
        active: false,
        audioActive: false,
        routeName: null,
        mode: "idle",
        phase: "idle",
        detail: null,
        displayedPositionMs: null,
        streamGeneration: 0,
        latencyMs: null,
      },
      loadStems,
    });
    useLibraryStore.setState({ updateSeparationStatus } as never);

    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    const handler = listeners.get("separation-complete");
    const event: SeparationCompleteEvent = {
      song_id: "song-b",
      status: { state: "complete", progress: 1 },
    } as never;
    handler!({ payload: event });

    expect(updateSeparationStatus).toHaveBeenCalled();
    expect(loadStems).not.toHaveBeenCalled();
  });

  test("notifies on separation-error events", async () => {
    const updateSeparationStatus = vi.fn();
    useLibraryStore.setState({ updateSeparationStatus } as never);

    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    const handler = listeners.get("separation-error");
    expect(handler).not.toBeUndefined();

    const event: SeparationErrorEvent = {
      song_id: "song-a",
      error: "decode failed",
    } as never;
    handler!({ payload: event });

    expect(updateSeparationStatus).toHaveBeenCalled();
    expect(mockNotifyError).toHaveBeenCalledWith("decode failed");
    expect(mockNotifyWhenUnfocused).toHaveBeenCalledWith(
      "Separation failed",
      expect.any(String),
    );
  });

  test("resets separation status to idle on separation-cancelled events", async () => {
    const updateSeparationStatus = vi.fn();
    useLibraryStore.setState({ updateSeparationStatus } as never);

    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    const handler = listeners.get("separation-cancelled");
    expect(handler).not.toBeUndefined();

    handler!({ payload: { song_id: "song-a" } });

    expect(updateSeparationStatus).toHaveBeenCalledWith(
      expect.objectContaining({ song_id: "song-a", state: "idle" }),
    );
    expect(mockNotifyError).not.toHaveBeenCalled();
  });

  test("surfaces a cache-hit toast when separation-complete reports cache_hit", async () => {
    const updateSeparationStatus = vi.fn();
    const loadStems = vi.fn().mockResolvedValue(undefined);
    usePlayerStore.setState({ loadStems } as never);
    useLibraryStore.setState({ updateSeparationStatus } as never);

    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    const handler = listeners.get("separation-complete");
    expect(handler).not.toBeUndefined();

    handler!({
      payload: {
        song_id: "song-a",
        status: { state: "completed", cache_hit: true },
      },
    });

    expect(mockNotifySuccess).toHaveBeenCalledOnce();
    // A cache hit returns before the user could switch away, so it must not
    // reach the native notification path.
    expect(mockNotifyWhenUnfocused).not.toHaveBeenCalled();
  });

  test("posts a native notification naming the song when a fresh separation completes", async () => {
    const updateSeparationStatus = vi.fn();
    const loadStems = vi.fn().mockResolvedValue(undefined);
    usePlayerStore.setState({ loadStems } as never);
    useLibraryStore.setState({
      updateSeparationStatus,
      songs: [{ hash: "song-a", title: "Bohemian Rhapsody" }],
    } as never);

    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    listeners.get("separation-complete")!({
      payload: {
        song_id: "song-a",
        status: { state: "completed", cache_hit: false },
      },
    });

    expect(mockNotifySuccess).not.toHaveBeenCalled();
    expect(mockNotifyWhenUnfocused).toHaveBeenCalledWith(
      "Separation complete",
      "Bohemian Rhapsody",
    );
  });

  test("updates separation status on separation-progress events", async () => {
    const updateSeparationStatus = vi.fn();
    useLibraryStore.setState({ updateSeparationStatus } as never);

    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    const handler = listeners.get("separation-progress");
    expect(handler).not.toBeUndefined();

    const event: SeparationProgressEvent = {
      song_id: "song-a",
      progress: 0.5,
    } as never;
    handler!({ payload: event });

    expect(updateSeparationStatus).toHaveBeenCalled();
  });

  test("forwards remote-playback-reconnect events to the remote playback store", async () => {
    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    const handler = listeners.get("remote-playback-reconnect");
    expect(handler).not.toBeUndefined();

    handler!({
      payload: {
        song_id: "song-x",
        request_id: 1,
        attempt: 1,
        max_attempts: 3,
        reason: "503",
      },
    });

    const { useRemotePlaybackStore } =
      await import("@/stores/remote-playback-store");
    expect(useRemotePlaybackStore.getState().reconnectState).toBe(
      "reconnecting",
    );
    expect(useRemotePlaybackStore.getState().songId).toBe("song-x");
    useRemotePlaybackStore.getState().reset();
  });

  test("forwards remote-playback-resync events to the remote playback store", async () => {
    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    const handler = listeners.get("remote-playback-resync");
    expect(handler).not.toBeUndefined();

    handler!({
      payload: {
        song_id: "song-y",
        requested_position_ms: 5000,
        actual_position_ms: 4000,
      },
    });

    const { useRemotePlaybackStore } =
      await import("@/stores/remote-playback-store");
    expect(useRemotePlaybackStore.getState().reconnectState).toBe("resync");
    expect(useRemotePlaybackStore.getState().resyncDeltaMs).toBe(1000);
    useRemotePlaybackStore.getState().reset();
  });

  test("forwards remote-playback-failed events to the remote playback store", async () => {
    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    mockListen.mockImplementation(
      async (eventName: string, handler: (e: { payload: unknown }) => void) => {
        listeners.set(eventName, handler);
        return () => {};
      },
    );

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true));

    const handler = listeners.get("remote-playback-failed");
    expect(handler).not.toBeUndefined();

    handler!({
      payload: {
        song_id: "song-z",
        request_id: 2,
        reason: "permanent",
      },
    });

    const { useRemotePlaybackStore } =
      await import("@/stores/remote-playback-store");
    expect(useRemotePlaybackStore.getState().reconnectState).toBe("failed");
    expect(useRemotePlaybackStore.getState().reason).toBe("permanent");
    useRemotePlaybackStore.getState().reset();
  });
});
