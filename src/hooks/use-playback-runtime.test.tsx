// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { usePlayerStore } from "@/stores/player-store";
import { useQueueStore } from "@/stores/queue-store";
import { useLibraryStore } from "@/stores/library-store";
import type {
  PlaybackPositionEvent,
  SeparationCompleteEvent,
  SeparationErrorEvent,
  SeparationProgressEvent,
  TrackTransitionedEvent,
} from "@/types/ipc";

const { mockListen, mockSetPreloadCandidate, mockNotifyError } = vi.hoisted(
  () => ({
    mockListen: vi.fn(),
    mockSetPreloadCandidate: vi.fn(),
    mockNotifyError: vi.fn(),
  }),
);

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
}));

// ─── Helpers ────────────────────────────────────────────────

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

// ─── Existing wiring tests ───────────────────────────────────

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

// ─── F3: Playback-position listener leak ────────────────────

describe("F3: playback-position listener cleanup when unmount races listen()", () => {
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

// ─── Behavioural tests for gapless preload (#88) ─────────────

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

    // Wait for any pending effects to settle
    await act(async () => {});

    expect(mockSetPreloadCandidate).not.toHaveBeenCalled();
    unmount();
  });
});

// ─── Behavioural tests for track-transitioned (#88) ──────────

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
      from_song_id: "song-a",
      to_song_id: "song-b",
    };
    handler!({ payload: event });

    expect(onTrackTransitioned).toHaveBeenCalledWith("song-a", "song-b");
  });
});

// ─── Behavioural tests for playback-position subscription ───

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

    const event: PlaybackPositionEvent = {
      song_id: "song-a",
      position_ms: 5000,
      transport_generation: 1,
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

    // Now resolve listen — the cleanup path should call unlisten()
    await act(async () => {
      resolveListen!(unlisten);
    });

    // The unlisten function should have been called because cancelled was true
    expect(unlisten).toHaveBeenCalled();
  });
});

// ─── Behavioural tests for separation events ────────────────

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
});
