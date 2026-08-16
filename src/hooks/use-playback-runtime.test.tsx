// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { useLibraryStore } from "@/stores/library-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import { useQueueStore } from "@/stores/queue-store";
import { useRemotePlaybackStore } from "@/stores/remote-playback-store";
import { useSettingsStore } from "@/stores/settings-store";
import { BackendProvider } from "@/lib/backend";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { createRecordingRuntimeEventSource } from "@/runtime/event-source";
import type {
  CommandError,
  PlaybackPositionEvent,
  PlaybackStateSnapshot,
  SeparationProgressEvent,
  SeparationStatusSnapshot,
} from "@/types/ipc";
import { useEventSubscriptions } from "./use-event-subscription";

function commandError(message: string): CommandError {
  return {
    code: "internal",
    message,
    retryable: false,
    fallback: "show_empty_state",
  };
}

function playbackSnapshot(songId: string): PlaybackStateSnapshot {
  return {
    song_id: songId,
    transport_generation: 1,
    state: "idle",
    is_playing: false,
    position_ms: 0,
    duration_ms: 10_000,
    buffered_ms: 10_000,
    volume: 1,
    stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
    has_stems: false,
    stem_mode: null,
  };
}

function completedSeparationStatus(
  songId: string,
  cacheHit: boolean,
): SeparationStatusSnapshot {
  return {
    song_id: songId,
    state: "completed",
    percent: 100,
    cache_hit: cacheHit,
    vocals_path: null,
    accomp_path: null,
    drums_path: null,
    bass_path: null,
    other_path: null,
    model_variant: null,
    error: null,
  };
}

const { mockNotifyError, mockNotifySuccess, mockNotifyWhenUnfocused } =
  vi.hoisted(() => ({
    mockNotifyError: vi.fn(),
    mockNotifySuccess: vi.fn(),
    mockNotifyWhenUnfocused: vi.fn(() => Promise.resolve()),
  }));

const mockGetPlaybackState = vi.fn();
const mockGetSettings = vi.fn();
const mockSetPreloadCandidate = vi.fn();

const backend = createMockBackend({
  overrides: {
    playback: {
      getPlaybackState: mockGetPlaybackState,
      setPreloadCandidate: mockSetPreloadCandidate,
    },
    settings: { getSettings: mockGetSettings },
  },
});

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
  notifySuccess: mockNotifySuccess,
}));

vi.mock("@/lib/notifications", () => ({
  notifyWhenUnfocused: mockNotifyWhenUnfocused,
}));

const initialPlayerActions = usePlayerStore.getState();
const initialLibraryActions = useLibraryStore.getState();
const initialLyricsActions = useLyricsStore.getState();
const initialSettingsActions = useSettingsStore.getState();
type RecordingRuntimeEventSource = ReturnType<
  typeof createRecordingRuntimeEventSource
>;
let runtimeSource: RecordingRuntimeEventSource =
  createRecordingRuntimeEventSource();
const unmountFns: Array<() => void> = [];

beforeEach(() => {
  runtimeSource = createRecordingRuntimeEventSource();
  mockGetPlaybackState.mockReset();
  mockGetSettings.mockReset();
  mockSetPreloadCandidate.mockReset();
  mockSetPreloadCandidate.mockResolvedValue(undefined);
  mockNotifyError.mockReset();
  mockNotifySuccess.mockReset();
  mockNotifyWhenUnfocused.mockReset();
  mockNotifyWhenUnfocused.mockResolvedValue(undefined);

  usePlayerStore.setState({
    snapshot: null,
    positionMs: 0,
    loadStems: initialPlayerActions.loadStems,
    applyPlaybackPositionEvent: initialPlayerActions.applyPlaybackPositionEvent,
    onTrackTransitioned: initialPlayerActions.onTrackTransitioned,
  });
  useLibraryStore.setState({
    songs: [],
    batchSeparation: null,
    updateSeparationStatus: initialLibraryActions.updateSeparationStatus,
    updateUploadStatus: initialLibraryActions.updateUploadStatus,
    clearUploadStatus: initialLibraryActions.clearUploadStatus,
  });
  useLyricsStore.setState({
    songId: null,
    fetchLyrics: initialLyricsActions.fetchLyrics,
  });
  useSettingsStore.setState({
    hydrateAppSettings: initialSettingsActions.hydrateAppSettings,
  });
  useQueueStore.setState({ queue: [], playHistory: [], isOpen: false });
  useRemotePlaybackStore.getState().reset();
});

afterEach(() => {
  while (unmountFns.length > 0) {
    const unmount = unmountFns.pop();
    unmount?.();
  }
});

async function renderHook(fn: () => void) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <BackendProvider backend={backend}>
        <HookHarness hookFn={fn} />
      </BackendProvider>,
    );
    await Promise.resolve();
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
  test("routes upload events into the library store", async () => {
    const updateUploadStatus = vi.fn();
    const clearUploadStatus = vi.fn();
    useLibraryStore.setState({ updateUploadStatus, clearUploadStatus });

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));

    await act(async () => {
      runtimeSource.emit("upload-progress", { song_id: "song-a", percent: 40 });
      runtimeSource.emit("upload-error", {
        song_id: "song-a",
        error: commandError("upload failed"),
      });
    });

    expect(updateUploadStatus).toHaveBeenLastCalledWith(
      expect.objectContaining({ song_id: "song-a", state: "failed" }),
    );
    expect(mockNotifyError).toHaveBeenCalledWith(
      expect.objectContaining({ message: "upload failed" }),
    );
  });
});

describe("runtime event subscription cleanup", () => {
  test("unlistens when registration resolves after unmount", async () => {
    let resolveSubscription: ((unlisten: () => void) => void) | undefined;
    const unlisten = vi.fn();
    const subscription = {
      subscribe: () =>
        new Promise<() => void>((resolve) => {
          resolveSubscription = resolve;
        }),
    };
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <HookHarness
          hookFn={() => useEventSubscriptions([subscription], true)}
        />,
      );
    });
    await act(async () => {
      root.unmount();
      resolveSubscription?.(unlisten);
      await Promise.resolve();
    });

    expect(unlisten).toHaveBeenCalledOnce();
    container.remove();
  });
});

describe("lyrics auto-fetch", () => {
  test("fetches lyrics when the current song changes", async () => {
    const fetchLyrics = vi.fn().mockResolvedValue(undefined);
    useLyricsStore.setState({ fetchLyrics });
    usePlayerStore.setState({ snapshot: playbackSnapshot("song-a") });

    const { useLyricsAutoFetch } = await import("./use-playback-runtime");
    await renderHook(() => useLyricsAutoFetch());

    expect(fetchLyrics).toHaveBeenCalledWith("song-a");

    await act(async () => {
      usePlayerStore.setState({ snapshot: playbackSnapshot("song-b") });
      await Promise.resolve();
    });

    expect(fetchLyrics).toHaveBeenNthCalledWith(2, "song-b");
  });

  test("does not fetch lyrics while disabled", async () => {
    const fetchLyrics = vi.fn().mockResolvedValue(undefined);
    useLyricsStore.setState({ fetchLyrics });
    usePlayerStore.setState({ snapshot: playbackSnapshot("song-a") });

    const { useLyricsAutoFetch } = await import("./use-playback-runtime");
    await renderHook(() => useLyricsAutoFetch(false));

    expect(fetchLyrics).not.toHaveBeenCalled();
  });
});

describe("usePreloadCandidateEffect", () => {
  test("selects the next queue item for preload", async () => {
    useQueueStore.setState({ queue: ["song-a", "song-b"] });
    usePlayerStore.setState({ snapshot: playbackSnapshot("song-a") });

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));

    expect(mockSetPreloadCandidate).toHaveBeenCalledWith("song-b");
  });

  test("selects the queue head when no song is playing", async () => {
    useQueueStore.setState({ queue: ["song-a", "song-b"] });

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));

    expect(mockSetPreloadCandidate).toHaveBeenCalledWith("song-a");
  });

  test("uses null when the queue is empty", async () => {
    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));

    expect(mockSetPreloadCandidate).toHaveBeenCalledWith(null);
  });

  test("does not run when the runtime is disabled", async () => {
    useQueueStore.setState({ queue: ["song-a"] });
    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(false, runtimeSource));

    expect(mockSetPreloadCandidate).not.toHaveBeenCalled();
  });
});

describe("playback event modules", () => {
  test("routes track transitions to the player store", async () => {
    const onTrackTransitioned = vi.fn();
    usePlayerStore.setState({ onTrackTransitioned });

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));
    runtimeSource.emit("track-transitioned", {
      transition_serial: 1,
      from_song_id: "song-a",
      to_song_id: "song-b",
    });

    expect(onTrackTransitioned).toHaveBeenCalledWith("song-a", "song-b");
  });

  test("routes playback positions to the player store", async () => {
    const applyPlaybackPositionEvent = vi.fn();
    usePlayerStore.setState({ applyPlaybackPositionEvent });
    const snapshot = playbackSnapshot("song-a");
    const event: PlaybackPositionEvent = {
      ms: 5_000,
      transport_generation: 1,
      snapshot: { ...snapshot, is_playing: true, position_ms: 5_000 },
    };

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));
    runtimeSource.emit("playback-position", event);

    expect(applyPlaybackPositionEvent).toHaveBeenCalledWith(event);
  });

  test("retries playback errors and advances ended tracks", async () => {
    const playSong = vi.fn().mockResolvedValue(undefined);
    const playNextFromQueue = vi.fn().mockResolvedValue(undefined);
    usePlayerStore.setState({ playSong, playNextFromQueue });

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));

    runtimeSource.emit("playback-error", {
      song_id: "song-a",
      error: commandError("decode failed"),
    });
    runtimeSource.emit("playback-ended", { song_id: "song-a" });

    const retryAction = mockNotifyError.mock.calls[0]?.[1];
    expect(retryAction).toEqual(expect.any(Function));
    if (typeof retryAction === "function") {
      retryAction();
    }

    expect(playSong).toHaveBeenCalledWith("song-a");
    expect(playNextFromQueue).toHaveBeenCalledWith("song-a");
  });
});

describe("separation event modules", () => {
  test("loads stems for the current song after separation", async () => {
    const loadStems = vi.fn().mockResolvedValue(undefined);
    const updateSeparationStatus = vi.fn();
    usePlayerStore.setState({
      snapshot: playbackSnapshot("song-a"),
      loadStems,
    });
    useLibraryStore.setState({ updateSeparationStatus });

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));
    runtimeSource.emit("separation-complete", {
      song_id: "song-a",
      status: completedSeparationStatus("song-a", true),
    });

    expect(updateSeparationStatus).toHaveBeenCalled();
    expect(loadStems).toHaveBeenCalled();
    expect(mockNotifySuccess).toHaveBeenCalledOnce();
  });

  test("does not load stems for a different song", async () => {
    const loadStems = vi.fn().mockResolvedValue(undefined);
    const updateSeparationStatus = vi.fn();
    usePlayerStore.setState({
      snapshot: playbackSnapshot("song-a"),
      loadStems,
    });
    useLibraryStore.setState({ updateSeparationStatus });

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));
    runtimeSource.emit("separation-complete", {
      song_id: "song-b",
      status: completedSeparationStatus("song-b", false),
    });

    expect(updateSeparationStatus).toHaveBeenCalled();
    expect(loadStems).not.toHaveBeenCalled();
  });

  test("reports a stem loading failure for the current song", async () => {
    const loadError = new Error("stem load failed");
    const loadStems = vi.fn().mockRejectedValue(loadError);
    const updateSeparationStatus = vi.fn();
    usePlayerStore.setState({
      snapshot: playbackSnapshot("song-a"),
      loadStems,
    });
    useLibraryStore.setState({
      songs: [
        {
          hash: "song-a",
          file_path: "/music/song-a.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          title: "Song A",
          artist: null,
          album: null,
          duration_ms: 0,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        },
      ],
      updateSeparationStatus,
    });

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));
    await act(async () => {
      runtimeSource.emit("separation-complete", {
        song_id: "song-a",
        status: completedSeparationStatus("song-a", false),
      });
      await Promise.resolve();
    });

    expect(mockNotifyWhenUnfocused).toHaveBeenCalledWith(
      expect.any(String),
      "Song A",
    );
    expect(mockNotifyError).toHaveBeenCalledWith(loadError);
  });

  test("notifies on separation errors and records cancellation", async () => {
    const updateSeparationStatus = vi.fn();
    useLibraryStore.setState({ updateSeparationStatus });

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));
    runtimeSource.emit("separation-error", {
      song_id: "song-a",
      error: commandError("decode failed"),
    });
    runtimeSource.emit("separation-cancelled", { song_id: "song-a" });

    expect(updateSeparationStatus).toHaveBeenLastCalledWith(
      expect.objectContaining({ song_id: "song-a", state: "idle" }),
    );
    expect(mockNotifyError).toHaveBeenCalledWith(
      expect.objectContaining({ message: "decode failed" }),
    );
  });

  test("updates separation progress without changing other domains", async () => {
    const updateSeparationStatus = vi.fn();
    useLibraryStore.setState({ updateSeparationStatus });
    const progress: SeparationProgressEvent = {
      song_id: "song-a",
      percent: 50,
    };

    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));
    runtimeSource.emit("separation-progress", progress);

    expect(updateSeparationStatus).toHaveBeenCalledWith(
      expect.objectContaining({ song_id: "song-a", state: "running" }),
    );
  });

  test("routes batch separation progress and terminal events", async () => {
    vi.useFakeTimers();
    try {
      const updateBatchProgress = vi.fn();
      const clearBatchSeparation = vi.fn();
      useLibraryStore.setState({ updateBatchProgress, clearBatchSeparation });

      const { useEventListeners } = await import("./use-playback-runtime");
      await renderHook(() => useEventListeners(true, runtimeSource));

      const progress = {
        total: 2,
        completed: 1,
        skipped: 0,
        failed: 0,
        current_song_id: "song-a",
        current_percent: 50,
      };
      runtimeSource.emit("batch-separation-progress", progress);
      runtimeSource.emit("batch-separation-complete", {
        ...progress,
        completed: 2,
        current_song_id: null,
        current_percent: 100,
      });
      runtimeSource.emit("batch-separation-cancelled", {
        ...progress,
        failed: 1,
        current_song_id: null,
        current_percent: 0,
      });

      expect(updateBatchProgress).toHaveBeenCalledTimes(3);
      expect(mockNotifyWhenUnfocused).toHaveBeenCalledOnce();

      vi.advanceTimersByTime(3_000);
      expect(clearBatchSeparation).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("fullscreen playback runtime", () => {
  test("hydrates state and subscribes to playback positions", async () => {
    const snapshot = playbackSnapshot("song-a");
    const settings = {
      stem_mode: "four_stem" as const,
      model_variant: "htdemucs" as const,
      language: "en",
      hide_batch_separate: false,
      cover_art_backdrop: true,
      lyrics_blur_inactive: false,
      hide_upgrade_all: false,
      lyrics_font_step: 0,
      execution_provider: "cpu" as const,
      available_execution_providers: ["cpu" as const],
      eq_enabled: false,
      eq_gains_db: [0, 0, 0, 0, 0] as [number, number, number, number, number],
      crossfade_enabled: false,
      crossfade_duration_ms: 3_000,
      library_sort_mode: "recently_imported" as const,
      theme_preference: "dark" as const,
      update_policy: "notify" as const,
    };
    const updateSnapshot = vi.fn();
    const applyPlaybackPositionEvent = vi.fn();
    const hydrateAppSettings = vi.fn();
    usePlayerStore.setState({ updateSnapshot, applyPlaybackPositionEvent });
    useSettingsStore.setState({ hydrateAppSettings });
    mockGetPlaybackState.mockResolvedValue(snapshot);
    mockGetSettings.mockResolvedValue(settings);

    const { useFullscreenPlaybackRuntime } =
      await import("./use-playback-runtime");
    await renderHook(() => useFullscreenPlaybackRuntime(runtimeSource));
    await vi.waitFor(() => {
      expect(updateSnapshot).toHaveBeenCalledWith(snapshot);
      expect(hydrateAppSettings).toHaveBeenCalledWith(settings);
    });

    const event: PlaybackPositionEvent = {
      ms: 2_000,
      transport_generation: 1,
      snapshot: { ...snapshot, position_ms: 2_000 },
    };
    runtimeSource.emit("playback-position", event);

    expect(applyPlaybackPositionEvent).toHaveBeenCalledWith(event);
  });
});

describe("upload event module", () => {
  test("clears an upload status after completion", async () => {
    vi.useFakeTimers();
    try {
      const updateUploadStatus = vi.fn();
      const clearUploadStatus = vi.fn();
      useLibraryStore.setState({ updateUploadStatus, clearUploadStatus });

      const { useEventListeners } = await import("./use-playback-runtime");
      await renderHook(() => useEventListeners(true, runtimeSource));
      runtimeSource.emit("upload-complete", {
        song_id: "song-a",
        remote_library_id: "remote-1",
      });

      expect(updateUploadStatus).toHaveBeenCalledWith(
        expect.objectContaining({ song_id: "song-a", state: "completed" }),
      );
      vi.advanceTimersByTime(3_000);
      expect(clearUploadStatus).toHaveBeenCalledWith("song-a");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("remote playback event module", () => {
  test("forwards reconnect, resync, and failure events", async () => {
    const { useEventListeners } = await import("./use-playback-runtime");
    await renderHook(() => useEventListeners(true, runtimeSource));

    runtimeSource.emit("remote-playback-reconnect", {
      song_id: "song-x",
      request_id: 1,
      attempt: 1,
      max_attempts: 3,
      reason: "503",
    });
    expect(useRemotePlaybackStore.getState().reconnectState).toBe(
      "reconnecting",
    );
    expect(useRemotePlaybackStore.getState().songId).toBe("song-x");

    runtimeSource.emit("remote-playback-resync", {
      song_id: "song-y",
      requested_position_ms: 5_000,
      actual_position_ms: 4_000,
    });
    expect(useRemotePlaybackStore.getState().reconnectState).toBe("resync");
    expect(useRemotePlaybackStore.getState().resyncDeltaMs).toBe(1_000);

    runtimeSource.emit("remote-playback-failed", {
      song_id: "song-z",
      request_id: 2,
      reason: "permanent",
    });
    expect(useRemotePlaybackStore.getState().reconnectState).toBe("failed");
    expect(useRemotePlaybackStore.getState().reason).toBe("permanent");
  });
});
