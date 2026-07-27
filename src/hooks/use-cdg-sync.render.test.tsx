// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const {
  mockGetCdgFrame,
  mockGetCdgStatus,
  mockDrawFrame,
  mockClearFrame,
  mockPostCdgFrame,
  mockPostCdgClear,
  mockPostCdgStatus,
  mockGetCdgSyncChannel,
  mockStartCdgSyncRequestListener,
} = vi.hoisted(() => {
  return {
    mockGetCdgFrame: vi.fn(),
    mockGetCdgStatus: vi.fn().mockResolvedValue({
      availability: "none",
      songId: null,
      transportGeneration: null,
      packetCount: null,
      errorCode: null,
    }),
    mockDrawFrame: vi.fn(),
    mockClearFrame: vi.fn(),
    mockPostCdgFrame: vi.fn(),
    mockPostCdgClear: vi.fn(),
    mockPostCdgStatus: vi.fn(),
    mockGetCdgSyncChannel: vi.fn(() => ({})),
    mockStartCdgSyncRequestListener: vi.fn(() => () => {}),
  };
});

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    getCdgFrame: mockGetCdgFrame,
    getCdgStatus: mockGetCdgStatus,
  };
});

vi.mock("@/lib/cdg-canvas-painter", () => ({
  drawFrame: mockDrawFrame,
  clearFrame: mockClearFrame,
}));

vi.mock("@/lib/cdg-sync-channel", () => ({
  getCdgSyncChannel: mockGetCdgSyncChannel,
  postCdgFrame: mockPostCdgFrame,
  postCdgClear: mockPostCdgClear,
  postCdgStatus: mockPostCdgStatus,
  startCdgSyncRequestListener: mockStartCdgSyncRequestListener,
  startCdgSyncReceiver: vi.fn(() => () => {}),
}));

vi.mock("@/lib/song-media", () => ({
  songHasCdgMedia: vi.fn(() => true),
}));

import { useCdgSync } from "./use-cdg-sync";
import {
  usePlayerStore,
  DEFAULT_AIRPLAY_OUTPUT_STATE,
} from "@/stores/player-store";
import { useCdgStore } from "@/stores/cdg-store";
import { useLibraryStore } from "@/stores/library-store";
import type { PlaybackStateSnapshot } from "@/types/ipc";
import { CDG_PROTOCOL_HEADER_SIZE, CDG_RGBA_SIZE } from "@/lib/cdg-protocol";

function buildBinaryFrame(
  transportGeneration: number,
  frameVersion: number,
): ArrayBuffer {
  const buf = new ArrayBuffer(CDG_PROTOCOL_HEADER_SIZE + CDG_RGBA_SIZE);
  const view = new DataView(buf);
  // Magic "OKCG"
  view.setUint8(0, 0x4f);
  view.setUint8(1, 0x4b);
  view.setUint8(2, 0x43);
  view.setUint8(3, 0x47);
  view.setUint16(4, 1, true);
  // Flags: RGBA present (bit 0)
  view.setUint16(6, 0x01, true);
  view.setBigUint64(8, BigInt(transportGeneration), true);
  view.setBigUint64(16, BigInt(frameVersion), true);
  view.setBigUint64(24, 0n, true);
  return buf;
}

function makeSnapshot(
  overrides: Partial<PlaybackStateSnapshot> = {},
): PlaybackStateSnapshot {
  return {
    song_id: "song-1",
    is_playing: true,
    position_ms: 0,
    duration_ms: 100000,
    volume: 1,
    stem_volumes: {},
    has_stems: false,
    stem_mode: null,
    transport_generation: 1,
    ...overrides,
  } as unknown as PlaybackStateSnapshot;
}

function TestComponent({ enabled }: { enabled: boolean }) {
  useCdgSync(enabled);
  return null;
}

describe("useCdgSync — render coverage", () => {
  let root: Root | null = null;
  let container: HTMLElement | null = null;

  beforeEach(() => {
    vi.clearAllMocks();
    mockGetCdgSyncChannel.mockReturnValue({});
    mockStartCdgSyncRequestListener.mockReturnValue(() => {});
    usePlayerStore.setState({
      snapshot: makeSnapshot(),
      positionMs: 0,
      airPlayOutput: DEFAULT_AIRPLAY_OUTPUT_STATE,
    });
    useCdgStore.setState({
      hasCdg: false,
      songId: null,
      availability: "none",
      errorCode: null,
      frameVersion: 0,
      transportGeneration: 0,
    });
    useLibraryStore.setState({
      songs: [
        {
          hash: "song-1",
          title: "Test",
          artist: "Test",
          cdg_path: "/path/to/cdg",
          file_path: null,
          audio_source_kind: "original",
          media_g_container: null,
          instrumental: false,
          language: null,
          album: null,
          duration_ms: 100000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: null,
        },
      ],
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
  });

  test("probe effect fetches first frame and draws it", async () => {
    const frame = buildBinaryFrame(1, 1);
    mockGetCdgFrame.mockResolvedValue(frame);

    await act(async () => {
      root!.render(<TestComponent enabled={true} />);
    });

    // Wait for the probe promise to resolve
    await act(async () => {
      await vi.waitFor(() => {
        expect(mockDrawFrame).toHaveBeenCalled();
      });
    });

    expect(mockGetCdgFrame).toHaveBeenCalledWith(
      "song-1",
      1,
      expect.any(Number),
      0,
    );
    expect(mockDrawFrame).toHaveBeenCalled();
    expect(mockPostCdgFrame).toHaveBeenCalled();
    expect(useCdgStore.getState().hasCdg).toBe(true);
    expect(useCdgStore.getState().songId).toBe("song-1");
    expect(useCdgStore.getState().frameVersion).toBe(1);
  });

  test("probe effect clears when songId is null", async () => {
    usePlayerStore.setState({
      snapshot: makeSnapshot({ song_id: "" }),
    });

    await act(async () => {
      root!.render(<TestComponent enabled={true} />);
    });

    await act(async () => {
      await vi.waitFor(() => {
        expect(mockPostCdgClear).toHaveBeenCalled();
      });
    });

    expect(mockClearFrame).toHaveBeenCalled();
    // postCdgStatus is called with (channel, payload)
    expect(mockPostCdgStatus).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ songId: null, hasCdg: false }),
    );
  });

  test("hot frame path draws and broadcasts on position sync tick", async () => {
    // Set up: song has CDG, store already has hasCdg=true
    useCdgStore.setState({
      hasCdg: true,
      songId: "song-1",
      frameVersion: 1,
      transportGeneration: 1,
    });

    const frame = buildBinaryFrame(1, 2);
    mockGetCdgFrame.mockResolvedValue(frame);

    await act(async () => {
      root!.render(<TestComponent enabled={true} />);
    });

    await act(async () => {
      await vi.waitFor(() => {
        expect(mockGetCdgFrame).toHaveBeenCalled();
      });
    });

    // Simulate a position change that crosses a CDG sync bucket (0 -> 3)
    // selectSyncDisplayPositionMs reads state.positionMs, not snapshot.position_ms
    await act(async () => {
      usePlayerStore.setState({
        positionMs: 100,
        snapshot: makeSnapshot({ position_ms: 100, is_playing: true }),
      });
    });

    // The hot frame path should fire and call getCdgFrame a second time
    await act(async () => {
      await vi.waitFor(
        () => {
          expect(mockGetCdgFrame.mock.calls.length).toBeGreaterThanOrEqual(2);
        },
        { timeout: 2000 },
      );
    });

    expect(mockDrawFrame).toHaveBeenCalled();
  });

  test("disabled hook does nothing", async () => {
    await act(async () => {
      root!.render(<TestComponent enabled={false} />);
    });

    expect(mockGetCdgFrame).not.toHaveBeenCalled();
    expect(mockDrawFrame).not.toHaveBeenCalled();
  });

  test("loading availability keeps hasCdg optimistic and retries until frame arrives", async () => {
    const loadingFrame = new ArrayBuffer(0);
    const realFrame = buildBinaryFrame(1, 1);
    let frameCallCount = 0;
    mockGetCdgFrame.mockImplementation(() => {
      frameCallCount += 1;
      return Promise.resolve(frameCallCount <= 1 ? loadingFrame : realFrame);
    });
    mockGetCdgStatus.mockResolvedValue({
      availability: "loading",
      songId: "song-1",
      transportGeneration: 1,
      packetCount: null,
      errorCode: null,
    });

    await act(async () => {
      root!.render(<TestComponent enabled={true} />);
    });

    await act(async () => {
      await vi.waitFor(() => {
        expect(mockGetCdgFrame).toHaveBeenCalledTimes(1);
      });
    });

    expect(useCdgStore.getState().hasCdg).toBe(true);
    expect(useCdgStore.getState().availability).toBe("loading");
    const clearFrameCallsAfterProbe = mockClearFrame.mock.calls.length;
    const postCdgClearCallsAfterProbe = mockPostCdgClear.mock.calls.length;

    // Simulate the track starting to play and position advancing, which
    // triggers the hot loop to re-probe.
    await act(async () => {
      usePlayerStore.setState({
        positionMs: 100,
        snapshot: makeSnapshot({ position_ms: 100, is_playing: true }),
      });
    });

    // The hot loop re-probes; the second getCdgFrame returns a real frame.
    await act(async () => {
      await vi.waitFor(
        () => {
          expect(mockDrawFrame).toHaveBeenCalled();
        },
        { timeout: 2000 },
      );
    });

    expect(useCdgStore.getState().hasCdg).toBe(true);
    expect(useCdgStore.getState().availability).toBe("ready");
    expect(mockClearFrame.mock.calls.length).toBe(clearFrameCallsAfterProbe);
    expect(mockPostCdgClear.mock.calls.length).toBe(
      postCdgClearCallsAfterProbe,
    );
  });
});
