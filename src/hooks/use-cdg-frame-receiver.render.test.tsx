// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

// The coalescing painter uses setTimeout(flush, 0) to batch frames into the
// next macrotask. We need fake timers to control this deterministically.

const {
  mockDrawFrame,
  mockClearFrame,
  mockGetCdgSyncChannel,
  mockStartCdgSyncReceiver,
} = vi.hoisted(() => {
  let frameHandler:
    | ((payload: {
        rgba: Uint8Array;
        frameVersion: number;
        transportGeneration: number;
      }) => void)
    | null = null;
  let clearHandler: (() => void) | null = null;
  let statusHandler:
    | ((payload: { songId: string | null; hasCdg: boolean }) => void)
    | null = null;

  return {
    mockDrawFrame: vi.fn(),
    mockClearFrame: vi.fn(),
    mockGetCdgSyncChannel: vi.fn(() => ({})),
    mockStartCdgSyncReceiver: vi.fn(
      (opts: {
        onFrame: (payload: {
          rgba: Uint8Array;
          frameVersion: number;
          transportGeneration: number;
        }) => void;
        onClear: () => void;
        onStatus: (payload: { songId: string | null; hasCdg: boolean }) => void;
      }) => {
        frameHandler = opts.onFrame;
        clearHandler = opts.onClear;
        statusHandler = opts.onStatus;
        return () => {};
      },
    ),
    getFrameHandler: () => frameHandler,
    getClearHandler: () => clearHandler,
    getStatusHandler: () => statusHandler,
  };
});

vi.mock("@/lib/cdg-canvas-painter", () => ({
  drawFrame: mockDrawFrame,
  clearFrame: mockClearFrame,
}));

vi.mock("@/lib/cdg-sync-channel", () => ({
  getCdgSyncChannel: mockGetCdgSyncChannel,
  startCdgSyncReceiver: mockStartCdgSyncReceiver,
  startCdgSyncRequestListener: vi.fn(() => () => {}),
}));

import { useCdgFrameReceiver } from "./use-cdg-frame-receiver";
import { useCdgStore } from "@/stores/cdg-store";

function TestComponent() {
  useCdgFrameReceiver();
  return null;
}

describe("useCdgFrameReceiver — render coverage", () => {
  let root: Root | null = null;
  let container: HTMLElement | null = null;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mockGetCdgSyncChannel.mockReturnValue({});
    useCdgStore.setState({
      hasCdg: false,
      songId: null,
      availability: "none",
      errorCode: null,
      frameVersion: 0,
      transportGeneration: 0,
    });
    container = document.createElement("div");
    root = createRoot(container);
  });

  afterEach(() => {
    if (root) {
      act(() => {
        root!.unmount();
      });
      root = null;
    }
    container = null;
    vi.useRealTimers();
  });

  test("onFrame draws the frame and updates frame version in the store", async () => {
    await act(async () => {
      root!.render(<TestComponent />);
    });

    const calls = mockStartCdgSyncReceiver.mock.calls;
    expect(calls.length).toBeGreaterThan(0);
    const opts = calls[0][0] as {
      onFrame: (payload: {
        rgba: Uint8Array;
        frameVersion: number;
        transportGeneration: number;
      }) => void;
      onClear: () => void;
      onStatus: (payload: { songId: string | null; hasCdg: boolean }) => void;
    };

    const rgba = new Uint8Array(4);
    // Both setFrameVersion and drawFrame are deferred through the
    // coalescing painter's setTimeout(flush, 0).
    act(() => {
      opts.onFrame({ rgba, frameVersion: 5, transportGeneration: 2 });
    });

    act(() => {
      vi.runAllTimers();
    });

    expect(useCdgStore.getState().frameVersion).toBe(5);
    expect(useCdgStore.getState().transportGeneration).toBe(2);
    expect(mockDrawFrame).toHaveBeenCalledWith(rgba);
  });

  test("onClear clears the store and canvas", async () => {
    useCdgStore.setState({
      hasCdg: true,
      songId: "song-1",
      frameVersion: 3,
      transportGeneration: 1,
    });

    await act(async () => {
      root!.render(<TestComponent />);
    });

    const calls = mockStartCdgSyncReceiver.mock.calls;
    const opts = calls[0][0] as {
      onFrame: () => void;
      onClear: () => void;
      onStatus: () => void;
    };

    act(() => {
      opts.onClear();
    });

    expect(mockClearFrame).toHaveBeenCalled();
    expect(useCdgStore.getState().hasCdg).toBe(false);
    expect(useCdgStore.getState().songId).toBeNull();
  });

  test("onStatus with songId sets the song in the store", async () => {
    await act(async () => {
      root!.render(<TestComponent />);
    });

    const calls = mockStartCdgSyncReceiver.mock.calls;
    const opts = calls[0][0] as {
      onFrame: () => void;
      onClear: () => void;
      onStatus: (payload: { songId: string | null; hasCdg: boolean }) => void;
    };

    act(() => {
      opts.onStatus({ songId: "song-abc", hasCdg: true });
    });

    expect(useCdgStore.getState().songId).toBe("song-abc");
    expect(useCdgStore.getState().hasCdg).toBe(true);
  });

  test("onStatus with null songId clears the store", async () => {
    useCdgStore.setState({
      hasCdg: true,
      songId: "old-song",
    });

    await act(async () => {
      root!.render(<TestComponent />);
    });

    const calls = mockStartCdgSyncReceiver.mock.calls;
    const opts = calls[0][0] as {
      onFrame: () => void;
      onClear: () => void;
      onStatus: (payload: { songId: string | null; hasCdg: boolean }) => void;
    };

    act(() => {
      opts.onStatus({ songId: null, hasCdg: false });
    });

    expect(useCdgStore.getState().hasCdg).toBe(false);
    expect(useCdgStore.getState().songId).toBeNull();
  });
});
