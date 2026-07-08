import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createWebviewSyncChannel } from "@/runtime/webview-sync";
import type { PlaybackPositionEvent, PlaybackStateSnapshot } from "@/types/ipc";
import {
  createPlayerStore,
  DEFAULT_AIRPLAY_OUTPUT_STATE,
  selectCurrentPositionMs,
  selectSyncDisplayPositionMs,
  type PlayerSyncSnapshot,
  usePlayerStore,
} from "./player-store";

const {
  mockResume,
  mockPause,
  mockSeek,
  mockSetVolume,
  mockSetStemVolume,
  mockLoadStems,
  mockGetPlaybackState,
  mockNotifyError,
} = vi.hoisted(() => ({
  mockResume: vi.fn(),
  mockPause: vi.fn(),
  mockSeek: vi.fn(),
  mockSetVolume: vi.fn(),
  mockSetStemVolume: vi.fn(),
  mockLoadStems: vi.fn(),
  mockGetPlaybackState: vi.fn(),
  mockNotifyError: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  play: vi.fn(),
  resume: mockResume,
  pause: mockPause,
  seek: mockSeek,
  setVolume: mockSetVolume,
  setStemVolume: mockSetStemVolume,
  loadStems: mockLoadStems,
  getPlaybackState: mockGetPlaybackState,
}));

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: {
    getState: () => ({ separationStatuses: {} }),
  },
}));

vi.mock("@/stores/queue-store", () => ({
  useQueueStore: {
    getState: () => ({
      addToQueue: vi.fn(),
      dequeue: vi.fn(),
      pushToHistory: vi.fn(),
      popFromHistory: vi.fn(),
    }),
  },
}));

// Session is the real implementation under the player-store adapter.
// Transport is mocked via @/lib/tauri above; queue/library seams are stubbed.

interface FakeChannel {
  onmessage: ((event: { data: unknown }) => void) | null;
  postMessage: (data: unknown) => void;
  close: () => void;
}

function playbackSnapshot(
  overrides: Partial<PlaybackStateSnapshot> = {},
): PlaybackStateSnapshot {
  return {
    song_id: "song-1",
    transport_generation: 1,
    state: "playing",
    is_playing: true,
    position_ms: 1200,
    duration_ms: 3000,
    buffered_ms: 3000,
    volume: 1,
    stem_volumes: {
      vocals: 1,
      drums: 1,
      bass: 1,
      other: 1,
    },
    has_stems: false,
    stem_mode: null,
    ...overrides,
  };
}

function playbackPositionEvent(
  snapshot: PlaybackStateSnapshot,
): PlaybackPositionEvent {
  return {
    ms: snapshot.position_ms,
    transport_generation: snapshot.transport_generation,
    snapshot,
  };
}

describe("selectSyncDisplayPositionMs", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    usePlayerStore.setState({
      airPlayPlainTextPagePending: false,
      airPlayPlainTextPagePendingDirection: null,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  test("prefers the AirPlay displayed position when active", () => {
    expect(
      selectSyncDisplayPositionMs({
        positionMs: 1000,
        airPlayOutput: {
          active: true,
          audioActive: true,
          routeName: "Living Room TV",
          mode: "lyrics",
          phase: "playing",
          detail: null,
          displayedPositionMs: 1250,
          streamGeneration: 7,
          latencyMs: 200,
        },
      }),
    ).toBe(1250);
  });

  test("falls back to the local playback position when AirPlay is inactive", () => {
    expect(
      selectSyncDisplayPositionMs({
        positionMs: 1000,
        airPlayOutput: {
          active: false,
          audioActive: false,
          routeName: null,
          mode: "idle",
          phase: "idle",
          detail: null,
          displayedPositionMs: 1250,
          streamGeneration: 0,
          latencyMs: null,
        },
      }),
    ).toBe(1000);
  });

  test("clears AirPlay plain-text page feedback after the lock window elapses", () => {
    usePlayerStore.getState().startAirPlayPlainTextPagePending("prev", 900);

    expect(usePlayerStore.getState().airPlayPlainTextPagePending).toBe(true);
    expect(usePlayerStore.getState().airPlayPlainTextPagePendingDirection).toBe(
      "prev",
    );

    vi.advanceTimersByTime(900);

    expect(usePlayerStore.getState().airPlayPlainTextPagePending).toBe(false);
    expect(usePlayerStore.getState().airPlayPlainTextPagePendingDirection).toBe(
      null,
    );
  });

  describe("selectCurrentPositionMs", () => {
    test("returns positionMs when playback is paused", () => {
      expect(
        selectCurrentPositionMs(
          {
            snapshot: playbackSnapshot({ is_playing: false }),
            positionMs: 1500,
            playingSinceMs: null,
          },
          () => 2000,
        ),
      ).toBe(1500);
    });

    test("returns positionMs when no snapshot exists", () => {
      expect(
        selectCurrentPositionMs(
          {
            snapshot: null,
            positionMs: 0,
            playingSinceMs: null,
          },
          () => 5000,
        ),
      ).toBe(0);
    });

    test("does not extrapolate during buffer underrun", () => {
      expect(
        selectCurrentPositionMs(
          {
            snapshot: playbackSnapshot({
              is_playing: true,
              state: "buffering",
            }),
            positionMs: 1500,
            playingSinceMs: 1000,
          },
          () => 2000,
        ),
      ).toBe(1500);
    });

    test("extrapolates position from the last sync point when playing", () => {
      expect(
        selectCurrentPositionMs(
          {
            snapshot: playbackSnapshot({ position_ms: 1200, is_playing: true }),
            positionMs: 1200,
            playingSinceMs: 1000,
          },
          () => 1500,
        ),
      ).toBe(1700);
    });

    test("advances smoothly between position events", () => {
      // Initial play at position 0, synced at monotonic time 1000
      expect(
        selectCurrentPositionMs(
          {
            snapshot: playbackSnapshot({ position_ms: 0, is_playing: true }),
            positionMs: 0,
            playingSinceMs: 1000,
          },
          () => 1000,
        ),
      ).toBe(0);

      // 33 ms later, position event arrives
      expect(
        selectCurrentPositionMs(
          {
            snapshot: playbackSnapshot({ position_ms: 33, is_playing: true }),
            positionMs: 33,
            playingSinceMs: 1033,
          },
          () => 1050,
        ),
      ).toBe(50);

      // 33 ms later, next position event
      expect(
        selectCurrentPositionMs(
          {
            snapshot: playbackSnapshot({ position_ms: 66, is_playing: true }),
            positionMs: 66,
            playingSinceMs: 1066,
          },
          () => 1066,
        ),
      ).toBe(66);
    });

    test("continues advancing even without position events arriving", () => {
      // Play at position 0, synced at monotonic time 1000
      // 500 ms passes, no events arrive
      expect(
        selectCurrentPositionMs(
          {
            snapshot: playbackSnapshot({ position_ms: 0, is_playing: true }),
            positionMs: 0,
            playingSinceMs: 1000,
          },
          () => 1500,
        ),
      ).toBe(500);
    });
  });

  test("syncs playback snapshot and position across webview contexts", () => {
    const channelsByName = new Map<string, Set<FakeChannel>>();
    const channelFactory = (name: string) => {
      const peers = channelsByName.get(name) ?? new Set<FakeChannel>();
      channelsByName.set(name, peers);

      const channel: FakeChannel = {
        onmessage: null,
        postMessage(data: unknown) {
          for (const peer of peers) {
            if (peer === channel) {
              continue;
            }

            peer.onmessage?.({ data });
          }
        },
        close() {
          peers.delete(channel);
        },
      };

      peers.add(channel);
      return channel;
    };

    const primary = createPlayerStore(
      createWebviewSyncChannel<PlayerSyncSnapshot>("player", {
        channelFactory,
        originId: "primary",
      }),
    );
    const secondary = createPlayerStore(
      createWebviewSyncChannel<PlayerSyncSnapshot>("player", {
        channelFactory,
        originId: "secondary",
      }),
    );

    primary.store.getState().updateSnapshot(playbackSnapshot());
    primary.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(playbackSnapshot({ position_ms: 1500 })),
      );

    expect(secondary.store.getState().snapshot?.song_id).toBe("song-1");
    expect(secondary.store.getState().positionMs).toBe(1500);
    expect(secondary.store.getState().airPlayOutput).toEqual(
      DEFAULT_AIRPLAY_OUTPUT_STATE,
    );

    primary.dispose();
    secondary.dispose();
  });

  test("applies the authoritative playback snapshot when background loading starts playback", () => {
    const player = createPlayerStore();
    player.store.getState().updateSnapshot(
      playbackSnapshot({
        state: "loading",
        is_playing: false,
        position_ms: 0,
        duration_ms: null,
      }),
    );

    player.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(playbackSnapshot({ position_ms: 1200 })),
      );

    expect(player.store.getState().snapshot).toEqual(
      playbackSnapshot({ position_ms: 1200 }),
    );
    expect(player.store.getState().positionMs).toBe(1200);

    player.dispose();
  });

  test("ignores stale position events from an older transport generation", () => {
    const player = createPlayerStore();
    player.store.getState().updateSnapshot(
      playbackSnapshot({
        song_id: "song-2",
        transport_generation: 2,
        state: "loading",
        is_playing: false,
        position_ms: 0,
        duration_ms: null,
      }),
    );

    player.store.getState().applyPlaybackPositionEvent(
      playbackPositionEvent(
        playbackSnapshot({
          song_id: "song-1",
          transport_generation: 1,
          is_playing: true,
          position_ms: 2400,
        }),
      ),
    );

    expect(player.store.getState().snapshot).toEqual(
      playbackSnapshot({
        song_id: "song-2",
        transport_generation: 2,
        state: "loading",
        is_playing: false,
        position_ms: 0,
        duration_ms: null,
      }),
    );
    expect(player.store.getState().positionMs).toBe(0);

    player.dispose();
  });

  test("accepts the matching generation event that starts playback after loading", () => {
    const player = createPlayerStore();
    player.store.getState().updateSnapshot(
      playbackSnapshot({
        song_id: "song-2",
        transport_generation: 2,
        state: "loading",
        is_playing: false,
        position_ms: 0,
        duration_ms: null,
      }),
    );

    player.store.getState().applyPlaybackPositionEvent(
      playbackPositionEvent(
        playbackSnapshot({
          song_id: "song-2",
          transport_generation: 2,
          is_playing: true,
          position_ms: 120,
        }),
      ),
    );

    expect(player.store.getState().snapshot?.song_id).toBe("song-2");
    expect(player.store.getState().snapshot?.is_playing).toBe(true);
    expect(player.store.getState().positionMs).toBe(120);
    expect(player.store.getState().playingSinceMs).not.toBeNull();

    player.dispose();
  });

  test("syncs transport fields from position ticks without replacing snapshot", () => {
    const player = createPlayerStore();
    const currentSnapshot = playbackSnapshot({ is_playing: false });
    player.store.getState().updateSnapshot(currentSnapshot);

    player.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(
          playbackSnapshot({ position_ms: 1500, is_playing: true }),
        ),
      );

    expect(player.store.getState().snapshot).not.toBe(currentSnapshot);
    expect(player.store.getState().snapshot?.is_playing).toBe(true);
    expect(player.store.getState().positionMs).toBe(1500);

    player.dispose();
  });

  test("keeps the snapshot stable for ordinary position ticks", () => {
    const player = createPlayerStore();
    const currentSnapshot = playbackSnapshot();
    player.store.getState().updateSnapshot(currentSnapshot);

    player.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(playbackSnapshot({ position_ms: 1500 })),
      );

    expect(player.store.getState().snapshot).toBe(currentSnapshot);
    expect(player.store.getState().positionMs).toBe(1500);

    player.dispose();
  });

  test("advances positionMs across multiple ordinary position ticks", () => {
    const player = createPlayerStore();
    const currentSnapshot = playbackSnapshot();
    player.store.getState().updateSnapshot(currentSnapshot);

    player.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(playbackSnapshot({ position_ms: 1200 })),
      );
    expect(player.store.getState().positionMs).toBe(1200);

    player.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(playbackSnapshot({ position_ms: 1500 })),
      );
    expect(player.store.getState().positionMs).toBe(1500);

    player.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(playbackSnapshot({ position_ms: 1800 })),
      );
    expect(player.store.getState().positionMs).toBe(1800);

    player.dispose();
  });

  test("positions from the empty store via a series of position events", () => {
    const player = createPlayerStore();

    player.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(playbackSnapshot({ position_ms: 0 })),
      );
    expect(player.store.getState().positionMs).toBe(0);
    expect(player.store.getState().snapshot).toBeDefined();

    player.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(playbackSnapshot({ position_ms: 500 })),
      );
    expect(player.store.getState().positionMs).toBe(500);

    player.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(playbackSnapshot({ position_ms: 1500 })),
      );
    expect(player.store.getState().positionMs).toBe(1500);

    player.dispose();
  });

  test("applies position-only events while playing without monotonic guard", () => {
    const player = createPlayerStore();
    player.store
      .getState()
      .updateSnapshot(
        playbackSnapshot({ is_playing: true, position_ms: 2000 }),
      );

    player.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(playbackSnapshot({ position_ms: 1500 })),
      );

    expect(player.store.getState().positionMs).toBe(1500);

    player.dispose();
  });

  test("rebases playingSinceMs when applying cross-webview sync snapshot", () => {
    const nowSpy = vi.spyOn(performance, "now").mockReturnValue(5000);
    const channelsByName = new Map<string, Set<FakeChannel>>();
    const channelFactory = (name: string) => {
      const peers = channelsByName.get(name) ?? new Set<FakeChannel>();
      channelsByName.set(name, peers);

      const channel: FakeChannel = {
        onmessage: null,
        postMessage(data: unknown) {
          for (const peer of peers) {
            if (peer === channel) continue;
            peer.onmessage?.({ data });
          }
        },
        close() {
          peers.delete(channel);
        },
      };

      peers.add(channel);
      return channel;
    };

    const primary = createPlayerStore(
      createWebviewSyncChannel<PlayerSyncSnapshot>("player", {
        channelFactory,
        originId: "primary",
      }),
    );
    const secondary = createPlayerStore(
      createWebviewSyncChannel<PlayerSyncSnapshot>("player", {
        channelFactory,
        originId: "secondary",
      }),
    );

    primary.store
      .getState()
      .updateSnapshot(
        playbackSnapshot({ is_playing: true, position_ms: 1200 }),
      );

    expect(secondary.store.getState().playingSinceMs).toBe(5000);

    nowSpy.mockRestore();
    primary.dispose();
    secondary.dispose();
  });
});

describe("resume", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    vi.useFakeTimers();
    player = createPlayerStore();
    mockResume.mockReset();
    mockNotifyError.mockReset();
  });

  afterEach(() => {
    player.dispose();
    vi.useRealTimers();
  });

  test("updates snapshot, positionMs, and sets playingSinceMs", async () => {
    const snap = playbackSnapshot({ is_playing: true, position_ms: 500 });
    mockResume.mockResolvedValue(snap);

    await player.store.getState().resume();

    expect(mockResume).toHaveBeenCalled();
    expect(player.store.getState().snapshot).toEqual(snap);
    expect(player.store.getState().positionMs).toBe(500);
    expect(player.store.getState().playingSinceMs).not.toBeNull();
  });

  test("calls notifyError when api.resume rejects", async () => {
    const error = new Error("resume failed");
    mockResume.mockRejectedValue(error);

    await player.store.getState().resume();

    expect(mockNotifyError).toHaveBeenCalledWith(error);
    expect(player.store.getState().snapshot).toBeNull();
  });
});

describe("pause", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    vi.useFakeTimers();
    player = createPlayerStore();
    mockPause.mockReset();
    mockNotifyError.mockReset();
  });

  afterEach(() => {
    player.dispose();
    vi.useRealTimers();
  });

  test("forces is_playing false when the pause API still reports playing", async () => {
    const snap = playbackSnapshot({ is_playing: true, position_ms: 1200 });
    mockPause.mockResolvedValue(snap);

    await player.store.getState().pause();

    expect(player.store.getState().snapshot?.is_playing).toBe(false);
    expect(player.store.getState().playingSinceMs).toBeNull();
  });

  test("ignores stale is_playing position ticks briefly after pause", async () => {
    player.store
      .getState()
      .updateSnapshot(playbackSnapshot({ is_playing: true }));
    mockPause.mockResolvedValue(
      playbackSnapshot({
        transport_generation: 2,
        is_playing: true,
        position_ms: 1200,
      }),
    );

    await player.store.getState().pause();
    expect(player.store.getState().snapshot?.is_playing).toBe(false);

    player.store
      .getState()
      .applyPlaybackPositionEvent(
        playbackPositionEvent(
          playbackSnapshot({ is_playing: true, position_ms: 1200 }),
        ),
      );
    expect(player.store.getState().snapshot?.is_playing).toBe(false);
  });

  test("updates snapshot, positionMs, and clears playingSinceMs", async () => {
    const snap = playbackSnapshot({ is_playing: false, position_ms: 1200 });
    mockPause.mockResolvedValue(snap);

    await player.store.getState().pause();

    expect(mockPause).toHaveBeenCalled();
    expect(player.store.getState().snapshot).toEqual(snap);
    expect(player.store.getState().positionMs).toBe(1200);
    expect(player.store.getState().playingSinceMs).toBeNull();
  });

  test("calls notifyError when api.pause rejects", async () => {
    const error = new Error("pause failed");
    mockPause.mockRejectedValue(error);

    await player.store.getState().pause();

    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });
});

describe("seek", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    vi.useFakeTimers();
    player = createPlayerStore();
    mockSeek.mockReset();
    mockNotifyError.mockReset();
  });

  afterEach(() => {
    player.dispose();
    vi.useRealTimers();
  });

  test("is a no-op when no snapshot exists", async () => {
    await player.store.getState().seek(1000);

    expect(mockSeek).not.toHaveBeenCalled();
  });

  test("clamps negative ms to 0 and calls api.seek", async () => {
    player.store.getState().updateSnapshot(playbackSnapshot());
    const snap = playbackSnapshot({ position_ms: 0, is_playing: true });
    mockSeek.mockResolvedValue(snap);

    await player.store.getState().seek(-500);

    expect(mockSeek).toHaveBeenCalledWith(0);
    expect(player.store.getState().snapshot).toEqual(snap);
    expect(player.store.getState().positionMs).toBe(0);
  });

  test("passes through positive values to api.seek", async () => {
    player.store.getState().updateSnapshot(playbackSnapshot());
    const snap = playbackSnapshot({ position_ms: 1500, is_playing: true });
    mockSeek.mockResolvedValue(snap);

    await player.store.getState().seek(1500);

    expect(mockSeek).toHaveBeenCalledWith(1500);
    expect(player.store.getState().snapshot).toEqual(snap);
    expect(player.store.getState().positionMs).toBe(1500);
  });

  test("sets playingSinceMs to null when seek returns paused snapshot", async () => {
    player.store.getState().updateSnapshot(playbackSnapshot());
    const snap = playbackSnapshot({ is_playing: false, position_ms: 800 });
    mockSeek.mockResolvedValue(snap);

    await player.store.getState().seek(800);

    expect(player.store.getState().playingSinceMs).toBeNull();
  });
});

describe("setVolume", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    player = createPlayerStore();
    mockSetVolume.mockReset();
    mockNotifyError.mockReset();
  });

  afterEach(() => {
    player.dispose();
  });

  test("calls api.setVolume with the given level", async () => {
    const snap = playbackSnapshot({ volume: 0.5 });
    mockSetVolume.mockResolvedValue(snap);

    await player.store.getState().setVolume(0.5);

    expect(mockSetVolume).toHaveBeenCalledWith(0.5);
    expect(player.store.getState().snapshot).toEqual(snap);
  });

  test("clamps volume below 0 to 0", async () => {
    mockSetVolume.mockResolvedValue(playbackSnapshot());

    await player.store.getState().setVolume(-0.5);

    expect(mockSetVolume).toHaveBeenCalledWith(0);
  });

  test("clamps volume above 1 to 1", async () => {
    mockSetVolume.mockResolvedValue(playbackSnapshot());

    await player.store.getState().setVolume(1.5);

    expect(mockSetVolume).toHaveBeenCalledWith(1);
  });
});

describe("setStemVolume", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    player = createPlayerStore();
    mockSetStemVolume.mockReset();
    mockNotifyError.mockReset();
  });

  afterEach(() => {
    player.dispose();
  });

  test("calls api.setStemVolume with the given stem and level", async () => {
    const snap = playbackSnapshot({
      stem_volumes: { vocals: 0.8, drums: 1, bass: 1, other: 1 },
    });
    mockSetStemVolume.mockResolvedValue(snap);

    await player.store.getState().setStemVolume("vocals", 0.8);

    expect(mockSetStemVolume).toHaveBeenCalledWith("vocals", 0.8);
    expect(player.store.getState().snapshot).toEqual(snap);
  });

  test("clamps stem volume below 0 to 0", async () => {
    mockSetStemVolume.mockResolvedValue(playbackSnapshot());

    await player.store.getState().setStemVolume("drums", -1);

    expect(mockSetStemVolume).toHaveBeenCalledWith("drums", 0);
  });

  test("clamps stem volume above 1 to 1", async () => {
    mockSetStemVolume.mockResolvedValue(playbackSnapshot());

    await player.store.getState().setStemVolume("bass", 2);

    expect(mockSetStemVolume).toHaveBeenCalledWith("bass", 1);
  });
});

describe("loadStems", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    player = createPlayerStore();
    mockLoadStems.mockReset();
    mockNotifyError.mockReset();
  });

  afterEach(() => {
    player.dispose();
  });

  test("calls api.loadStems and updates the snapshot", async () => {
    const snap = playbackSnapshot({ has_stems: true, stem_mode: "two_stem" });
    mockLoadStems.mockResolvedValue(snap);

    await player.store.getState().loadStems();

    expect(mockLoadStems).toHaveBeenCalled();
    expect(player.store.getState().snapshot).toEqual(snap);
  });

  test("does not let an older loadStems response replace the active transport", async () => {
    player.store
      .getState()
      .updateSnapshot(
        playbackSnapshot({ transport_generation: 2, is_playing: true }),
      );
    mockLoadStems.mockResolvedValue(
      playbackSnapshot({
        transport_generation: 1,
        is_playing: false,
        has_stems: true,
        stem_mode: "two_stem",
      }),
    );

    await player.store.getState().loadStems();

    expect(player.store.getState().snapshot).toEqual(
      playbackSnapshot({ transport_generation: 2, is_playing: true }),
    );
  });

  test("calls notifyError when api.loadStems rejects", async () => {
    const error = new Error("load stems failed");
    mockLoadStems.mockRejectedValue(error);

    await player.store.getState().loadStems();

    expect(mockNotifyError).toHaveBeenCalledWith(error, expect.any(Function));
  });
});

describe("loadState", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    vi.useFakeTimers();
    player = createPlayerStore();
    mockGetPlaybackState.mockReset();
    mockNotifyError.mockReset();
  });

  afterEach(() => {
    player.dispose();
    vi.useRealTimers();
  });

  test("calls api.getPlaybackState and updates the store", async () => {
    const snap = playbackSnapshot({ is_playing: true, position_ms: 2000 });
    mockGetPlaybackState.mockResolvedValue(snap);

    await player.store.getState().loadState();

    expect(mockGetPlaybackState).toHaveBeenCalled();
    expect(player.store.getState().snapshot).toEqual(snap);
    expect(player.store.getState().positionMs).toBe(2000);
    expect(player.store.getState().playingSinceMs).not.toBeNull();
  });

  test("sets playingSinceMs to null when paused", async () => {
    const snap = playbackSnapshot({ is_playing: false, position_ms: 1000 });
    mockGetPlaybackState.mockResolvedValue(snap);

    await player.store.getState().loadState();

    expect(player.store.getState().playingSinceMs).toBeNull();
  });

  test("calls notifyError when api.getPlaybackState rejects", async () => {
    const error = new Error("get state failed");
    mockGetPlaybackState.mockRejectedValue(error);

    await player.store.getState().loadState();

    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });
});

describe("updateSnapshot", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    vi.useFakeTimers();
    player = createPlayerStore();
  });

  afterEach(() => {
    player.dispose();
    vi.useRealTimers();
  });

  test("sets playingSinceMs when is_playing is true", () => {
    const snap = playbackSnapshot({ is_playing: true, position_ms: 500 });

    player.store.getState().updateSnapshot(snap);

    expect(player.store.getState().snapshot).toEqual(snap);
    expect(player.store.getState().positionMs).toBe(500);
    expect(player.store.getState().playingSinceMs).not.toBeNull();
  });

  test("sets playingSinceMs to null when is_playing is false", () => {
    const snap = playbackSnapshot({ is_playing: false, position_ms: 1000 });

    player.store.getState().updateSnapshot(snap);

    expect(player.store.getState().snapshot).toEqual(snap);
    expect(player.store.getState().positionMs).toBe(1000);
    expect(player.store.getState().playingSinceMs).toBeNull();
  });
});

describe("updateAirPlayOutput", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    player = createPlayerStore();
  });

  afterEach(() => {
    player.dispose();
  });

  test("updates the airPlayOutput state", () => {
    const airPlayState = {
      ...DEFAULT_AIRPLAY_OUTPUT_STATE,
      active: true,
      audioActive: true,
      routeName: "Living Room",
      mode: "lyrics" as const,
      phase: "playing" as const,
    };

    player.store.getState().updateAirPlayOutput(airPlayState);

    expect(player.store.getState().airPlayOutput).toEqual(airPlayState);
  });

  test("resets to default when given the default state", () => {
    player.store.getState().updateAirPlayOutput({
      ...DEFAULT_AIRPLAY_OUTPUT_STATE,
      active: true,
      routeName: "TV",
    });

    player.store.getState().updateAirPlayOutput(DEFAULT_AIRPLAY_OUTPUT_STATE);

    expect(player.store.getState().airPlayOutput).toEqual(
      DEFAULT_AIRPLAY_OUTPUT_STATE,
    );
  });
});

describe("updateLocalAudienceOutputActive", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    player = createPlayerStore();
  });

  afterEach(() => {
    player.dispose();
  });

  test("toggles the localAudienceOutputActive flag", () => {
    player.store.getState().updateLocalAudienceOutputActive(true);
    expect(player.store.getState().localAudienceOutputActive).toBe(true);

    player.store.getState().updateLocalAudienceOutputActive(false);
    expect(player.store.getState().localAudienceOutputActive).toBe(false);
  });
});

describe("startAirPlayPlainTextPagePending", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    vi.useFakeTimers();
    player = createPlayerStore();
  });

  afterEach(() => {
    player.dispose();
    vi.useRealTimers();
  });

  test("sets pending flag and clears after timeout", () => {
    player.store.getState().startAirPlayPlainTextPagePending("next", 500);

    expect(player.store.getState().airPlayPlainTextPagePending).toBe(true);
    expect(player.store.getState().airPlayPlainTextPagePendingDirection).toBe(
      "next",
    );

    vi.advanceTimersByTime(500);

    expect(player.store.getState().airPlayPlainTextPagePending).toBe(false);
    expect(player.store.getState().airPlayPlainTextPagePendingDirection).toBe(
      null,
    );
  });

  test("replaces previous timer when called again", () => {
    player.store.getState().startAirPlayPlainTextPagePending("prev", 1000);
    vi.advanceTimersByTime(500);
    player.store.getState().startAirPlayPlainTextPagePending("next", 1000);

    expect(player.store.getState().airPlayPlainTextPagePendingDirection).toBe(
      "next",
    );

    // First timer has elapsed but second hasn't
    vi.advanceTimersByTime(500);
    expect(player.store.getState().airPlayPlainTextPagePending).toBe(true);

    // Second timer elapses
    vi.advanceTimersByTime(500);
    expect(player.store.getState().airPlayPlainTextPagePending).toBe(false);
    expect(player.store.getState().airPlayPlainTextPagePendingDirection).toBe(
      null,
    );
  });
});

describe("clearAirPlayPlainTextPagePending", () => {
  let player: ReturnType<typeof createPlayerStore>;

  beforeEach(() => {
    vi.useFakeTimers();
    player = createPlayerStore();
  });

  afterEach(() => {
    player.dispose();
    vi.useRealTimers();
  });

  test("clears pending flag and cancels timer", () => {
    player.store.getState().startAirPlayPlainTextPagePending("prev", 1000);
    expect(player.store.getState().airPlayPlainTextPagePending).toBe(true);

    player.store.getState().clearAirPlayPlainTextPagePending();

    expect(player.store.getState().airPlayPlainTextPagePending).toBe(false);
    expect(player.store.getState().airPlayPlainTextPagePendingDirection).toBe(
      null,
    );

    // Timer should be cancelled — advancing time shouldn't restore state
    vi.advanceTimersByTime(2000);
    expect(player.store.getState().airPlayPlainTextPagePending).toBe(false);
  });
});
