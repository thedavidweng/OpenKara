import { create } from "zustand";
import type {
  RemotePlaybackFailedEvent,
  RemotePlaybackReconnectEvent,
  RemotePlaybackResyncEvent,
} from "@/types/ipc";

/**
 * Reconnect state for the currently-playing remote song.
 *
 * The backend reconnect coordinator (PR #7) emits `remote-playback-reconnect`
 * before each re-resolve attempt, `remote-playback-resync` when the new source
 * snaps to a preceding boundary, and `remote-playback-failed` when the attempt
 * budget is exhausted or a permanent error occurs. This store holds the
 * latest state so the playback bar (PR #8) can render a "reconnecting…"
 * indicator and a transient resync notice.
 */
export type RemoteReconnectState =
  | "idle"
  | "reconnecting"
  | "resync"
  | "failed";

export interface RemotePlaybackState {
  /** Current reconnect state for the active song. */
  reconnectState: RemoteReconnectState;
  /** The song ID this state applies to. When the user switches songs, the
   * state resets to `idle`. */
  songId: string | null;
  /** 1-based attempt number from the latest `remote-playback-reconnect`
   * event. */
  attempt: number;
  /** Maximum reconnect attempts configured by the backend. */
  maxAttempts: number;
  /** Human-readable reason from the latest reconnect/failed event. */
  reason: string | null;
  /** The resync delta (requested − actual) in ms, or null when no resync
   * occurred. The UI shows a transient notice when this is non-null. */
  resyncDeltaMs: number | null;

  /** Apply a `remote-playback-reconnect` event. */
  applyReconnectEvent: (event: RemotePlaybackReconnectEvent) => void;
  /** Apply a `remote-playback-resync` event. */
  applyResyncEvent: (event: RemotePlaybackResyncEvent) => void;
  /** Apply a `remote-playback-failed` event. */
  applyFailedEvent: (event: RemotePlaybackFailedEvent) => void;
  /** Reset to idle (called when the user switches songs or playback stops). */
  reset: () => void;
}

export const useRemotePlaybackStore = create<RemotePlaybackState>((set) => ({
  reconnectState: "idle",
  songId: null,
  attempt: 0,
  maxAttempts: 0,
  reason: null,
  resyncDeltaMs: null,

  applyReconnectEvent: (event) =>
    set({
      reconnectState: "reconnecting",
      songId: event.song_id,
      attempt: event.attempt,
      maxAttempts: event.max_attempts,
      reason: event.reason,
      resyncDeltaMs: null,
    }),

  applyResyncEvent: (event) =>
    set({
      reconnectState: "resync",
      songId: event.song_id,
      resyncDeltaMs: event.requested_position_ms - event.actual_position_ms,
    }),

  applyFailedEvent: (event) =>
    set({
      reconnectState: "failed",
      songId: event.song_id,
      reason: event.reason,
    }),

  reset: () =>
    set({
      reconnectState: "idle",
      songId: null,
      attempt: 0,
      maxAttempts: 0,
      reason: null,
      resyncDeltaMs: null,
    }),
}));
