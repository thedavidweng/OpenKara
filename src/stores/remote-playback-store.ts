import { create } from "zustand";
import type {
  RemotePlaybackFailedEvent,
  RemotePlaybackReconnectEvent,
  RemotePlaybackResyncEvent,
} from "@/types/ipc";

export type RemoteReconnectState =
  | "idle"
  | "reconnecting"
  | "resync"
  | "failed";

export interface RemotePlaybackState {
  reconnectState: RemoteReconnectState;
  songId: string | null;
  attempt: number;
  maxAttempts: number;
  reason: string | null;
  resyncDeltaMs: number | null;

  applyReconnectEvent: (event: RemotePlaybackReconnectEvent) => void;
  applyResyncEvent: (event: RemotePlaybackResyncEvent) => void;
  applyFailedEvent: (event: RemotePlaybackFailedEvent) => void;
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
