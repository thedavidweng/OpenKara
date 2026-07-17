import { create } from "zustand";
import * as api from "@/lib/tauri";
import { notifyError } from "@/lib/errors";
import {
  createPlaybackSession,
  selectCurrentPositionMs as selectSessionPositionMs,
  selectSyncDisplayPositionMs as selectSessionSyncDisplayPositionMs,
  type PlaybackSession,
  type PlaybackTransport,
  type PositionClockState,
} from "@/playback";
import {
  createWebviewSyncChannel,
  type WebviewSyncChannel,
} from "@/runtime/webview-sync";
import { useLibraryStore } from "@/stores/library-store";
import { useQueueStore } from "@/stores/queue-store";
import type {
  AirPlayOutputStateEvent,
  PlaybackPositionEvent,
  PlaybackStateSnapshot,
  StemName,
} from "@/types/ipc";

export const DEFAULT_AIRPLAY_OUTPUT_STATE: AirPlayOutputStateEvent = {
  active: false,
  audioActive: false,
  routeName: null,
  mode: "idle",
  phase: "idle",
  detail: null,
  displayedPositionMs: null,
  streamGeneration: 0,
  latencyMs: null,
};

interface PlayerState {
  snapshot: PlaybackStateSnapshot | null;
  positionMs: number;
  /** monotonic-ms timestamp of the last authoritative position update;
   * null when playback is paused/stopped so extrapolation halts. */
  playingSinceMs: number | null;
  /**
   * Monotonic host-owned seek edge. Lyrics consume this only after the
   * authoritative Tauri seek response has been applied to the playback clock.
   */
  seekRevision: number;
  airPlayOutput: AirPlayOutputStateEvent;
  localAudienceOutputActive: boolean;
  airPlayPlainTextPagePending: boolean;
  airPlayPlainTextPagePendingDirection: "prev" | "next" | null;

  playSong: (songId: string) => Promise<void>;
  playNow: (songId: string) => Promise<void>;
  resume: () => Promise<void>;
  pause: () => Promise<void>;
  seek: (ms: number) => Promise<void>;
  setVolume: (level: number) => Promise<void>;
  setStemVolume: (stem: StemName, level: number) => Promise<void>;
  loadStems: () => Promise<void>;
  applyPlaybackPositionEvent: (event: PlaybackPositionEvent) => void;
  updateSnapshot: (snapshot: PlaybackStateSnapshot) => void;
  loadState: () => Promise<void>;
  playNextFromQueue: (endedSongId: string) => Promise<void>;
  skipForward: () => Promise<void>;
  skipBack: () => Promise<void>;
  updateAirPlayOutput: (airPlayOutput: AirPlayOutputStateEvent) => void;
  updateLocalAudienceOutputActive: (active: boolean) => void;
  startAirPlayPlainTextPagePending: (
    direction: "prev" | "next",
    lockMs: number,
  ) => void;
  clearAirPlayPlainTextPagePending: () => void;
}

export interface PlayerSyncSnapshot {
  snapshot: PlaybackStateSnapshot | null;
  positionMs: number;
  playingSinceMs: number | null;
  seekRevision: number;
  airPlayOutput: AirPlayOutputStateEvent;
  localAudienceOutputActive: boolean;
  airPlayPlainTextPagePending: boolean;
  airPlayPlainTextPagePendingDirection: "prev" | "next" | null;
}

function createPlayerSyncSnapshot(state: PlayerState): PlayerSyncSnapshot {
  return {
    snapshot: state.snapshot,
    positionMs: state.positionMs,
    playingSinceMs: state.playingSinceMs,
    seekRevision: state.seekRevision,
    airPlayOutput: state.airPlayOutput,
    localAudienceOutputActive: state.localAudienceOutputActive,
    airPlayPlainTextPagePending: state.airPlayPlainTextPagePending,
    airPlayPlainTextPagePendingDirection:
      state.airPlayPlainTextPagePendingDirection,
  };
}

function applyPlayerSyncSnapshot(
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
  session: PlaybackSession,
  payload: PlayerSyncSnapshot,
) {
  // RATIONALE: playingSinceMs is tied to this webview's performance.now() clock.
  // Applying a peer window's timestamp would break selectCurrentPositionMs extrapolation.
  const playingSinceMs =
    payload.playingSinceMs === null
      ? null
      : payload.snapshot?.is_playing
        ? performance.now()
        : null;

  const clock: PositionClockState = {
    snapshot: payload.snapshot,
    positionMs: payload.positionMs,
    playingSinceMs,
  };
  // Keep session clock aligned with peer sync without re-publishing.
  session.replaceClock(clock);

  set({
    snapshot: payload.snapshot,
    positionMs: payload.positionMs,
    playingSinceMs,
    // Delayed BroadcastChannel messages must never replay an older seek edge.
    seekRevision: Math.max(get().seekRevision, payload.seekRevision ?? 0),
    airPlayOutput: payload.airPlayOutput,
    localAudienceOutputActive: payload.localAudienceOutputActive,
    airPlayPlainTextPagePending: payload.airPlayPlainTextPagePending,
    airPlayPlainTextPagePendingDirection:
      payload.airPlayPlainTextPagePendingDirection,
  });
}

export function selectSyncDisplayPositionMs(
  state: Pick<PlayerState, "positionMs" | "airPlayOutput">,
): number {
  return selectSessionSyncDisplayPositionMs(state);
}

export function selectCurrentPositionMs(
  state: Pick<PlayerState, "snapshot" | "positionMs" | "playingSinceMs">,
  nowMs = () => performance.now(),
): number {
  return selectSessionPositionMs(state, nowMs);
}

const sessionTransport: PlaybackTransport = {
  play: api.play,
  resume: api.resume,
  pause: api.pause,
  seek: api.seek,
  setVolume: api.setVolume,
  setStemVolume: api.setStemVolume,
  loadStems: api.loadStems,
  getPlaybackState: api.getPlaybackState,
};

export function createPlayerStore(
  syncChannel: WebviewSyncChannel<PlayerSyncSnapshot> = createWebviewSyncChannel<PlayerSyncSnapshot>(
    "openkara.player",
  ),
) {
  let airPlayPlainTextPagePendingTimer: ReturnType<typeof setTimeout> | null =
    null;
  // Assigned synchronously inside create() before any subscriber can run.
  let sessionRef!: PlaybackSession;

  const store = create<PlayerState>((set, get) => {
    const syncPatch = (patch: Partial<PlayerState>) => {
      set(patch);
      syncChannel.publish(createPlayerSyncSnapshot(get()));
    };

    // Reactive adapter: session owns lifecycle + clock; store mirrors for UI.
    const session = createPlaybackSession({
      transport: sessionTransport,
      queue: {
        addToQueue: (id) => useQueueStore.getState().addToQueue(id),
        dequeue: () => useQueueStore.getState().dequeue() ?? null,
        pushToHistory: (id) => useQueueStore.getState().pushToHistory(id),
        popFromHistory: () => useQueueStore.getState().popFromHistory() ?? null,
      },
      getSeparationStatus: (songId) =>
        useLibraryStore.getState().separationStatuses[songId],
      onClockChange: (clock) => {
        syncPatch({
          snapshot: clock.snapshot,
          positionMs: clock.positionMs,
          playingSinceMs: clock.playingSinceMs,
        });
      },
    });
    sessionRef = session;

    return {
      snapshot: null,
      positionMs: 0,
      playingSinceMs: null,
      seekRevision: 0,
      airPlayOutput: DEFAULT_AIRPLAY_OUTPUT_STATE,
      localAudienceOutputActive: false,
      airPlayPlainTextPagePending: false,
      airPlayPlainTextPagePendingDirection: null,

      playSong: async (songId) => {
        try {
          await session.play(songId);
        } catch (e) {
          notifyError(e, () => get().playSong(songId));
        }
      },

      playNow: async (songId) => {
        try {
          await session.playNow(songId);
        } catch (e) {
          notifyError(e, () => get().playNow(songId));
        }
      },

      resume: async () => {
        try {
          await session.resume();
        } catch (e) {
          notifyError(e);
        }
      },

      pause: async () => {
        try {
          await session.pause();
        } catch (e) {
          notifyError(e);
        }
      },

      seek: async (ms) => {
        const beforeSeek = get().snapshot;
        if (!beforeSeek?.song_id) {
          return;
        }

        try {
          const applied = await session.seek(ms);
          if (!applied) {
            return;
          }

          const snapshot = get().snapshot;
          if (
            snapshot?.song_id === beforeSeek.song_id &&
            snapshot.transport_generation > beforeSeek.transport_generation
          ) {
            // RATIONALE: Tauri seek is asynchronous. Publishing a seek edge
            // from the click/mouseup handler lets the lyrics rAF consume
            // resetScroll against the old playhead. Publish only after the
            // authoritative seek snapshot is installed, so the same lyrics
            // frame sees both the target time and isSeek=true.
            syncPatch({
              seekRevision: Math.max(
                get().seekRevision + 1,
                snapshot.transport_generation,
              ),
            });
          }
        } catch (e) {
          notifyError(e);
        }
      },

      setVolume: async (level) => {
        try {
          await session.setVolume(level);
        } catch (e) {
          notifyError(e);
        }
      },

      setStemVolume: async (stem, level) => {
        try {
          await session.setStemVolume(stem, level);
        } catch (e) {
          notifyError(e);
        }
      },

      loadStems: async () => {
        try {
          await session.loadStems();
        } catch (e) {
          notifyError(e, () => get().loadStems());
        }
      },

      applyPlaybackPositionEvent: (event) => {
        session.applyPosition(event);
      },

      updateSnapshot: (snapshot) => {
        session.applySnapshot(snapshot);
      },

      loadState: async () => {
        try {
          await session.loadState();
        } catch (e) {
          notifyError(e);
        }
      },

      playNextFromQueue: async (endedSongId) => {
        try {
          await session.onEnded(endedSongId);
        } catch (e) {
          notifyError(e);
        }
      },

      onTrackTransitioned: (fromSongId, toSongId) => {
        session.onTrackTransitioned(fromSongId, toSongId).catch((e) => {
          notifyError(e);
        });
      },

      skipForward: async () => {
        try {
          await session.skipForward();
        } catch (e) {
          notifyError(e);
        }
      },

      skipBack: async () => {
        try {
          await session.skipBack();
        } catch (e) {
          notifyError(e);
        }
      },

      updateAirPlayOutput: (airPlayOutput) => {
        syncPatch({ airPlayOutput });
      },

      updateLocalAudienceOutputActive: (active) => {
        syncPatch({ localAudienceOutputActive: active });
      },

      startAirPlayPlainTextPagePending: (direction, lockMs) => {
        if (airPlayPlainTextPagePendingTimer !== null) {
          clearTimeout(airPlayPlainTextPagePendingTimer);
        }

        syncPatch({
          airPlayPlainTextPagePending: true,
          airPlayPlainTextPagePendingDirection: direction,
        });

        airPlayPlainTextPagePendingTimer = setTimeout(() => {
          airPlayPlainTextPagePendingTimer = null;
          get().clearAirPlayPlainTextPagePending();
        }, lockMs);
      },

      clearAirPlayPlainTextPagePending: () => {
        if (airPlayPlainTextPagePendingTimer !== null) {
          clearTimeout(airPlayPlainTextPagePendingTimer);
          airPlayPlainTextPagePendingTimer = null;
        }

        syncPatch({
          airPlayPlainTextPagePending: false,
          airPlayPlainTextPagePendingDirection: null,
        });
      },
    };
  });

  const unsubscribe = syncChannel.subscribe((payload) => {
    applyPlayerSyncSnapshot(
      store.setState,
      store.getState,
      sessionRef,
      payload,
    );
  });

  return {
    store,
    dispose() {
      if (airPlayPlainTextPagePendingTimer !== null) {
        clearTimeout(airPlayPlainTextPagePendingTimer);
        airPlayPlainTextPagePendingTimer = null;
      }
      unsubscribe();
      syncChannel.close();
    },
  };
}

const defaultPlayerStore = createPlayerStore();

export const usePlayerStore = defaultPlayerStore.store;
