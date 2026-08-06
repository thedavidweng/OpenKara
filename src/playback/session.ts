import type {
  PlaybackPositionEvent,
  PlaybackStateSnapshot,
  SeparationStatusSnapshot,
  StemName,
} from "@/types/ipc";
import {
  type PositionClockState,
  reduceAuthoritativeSnapshot,
  reducePositionEvent,
} from "./position-clock";
import {
  shouldEnqueueInsteadOfReplacingCurrentSong,
  shouldLoadSeparatedStems,
} from "./session-policies";

export interface PlaybackSession {
  play(songId: string): Promise<void>;
  playNow(songId: string): Promise<void>;
  skipForward(): Promise<void>;
  skipBack(): Promise<void>;
  onEnded(endedSongId: string): Promise<void>;
  onTrackTransitioned(fromSongId: string, toSongId: string): Promise<void>;
  applyPosition(event: PlaybackPositionEvent): void;
  applySnapshot(snapshot: PlaybackStateSnapshot): void;
  getPositionClock(): PositionClockState;
  replaceClock(clock: PositionClockState): void;

  resume(): Promise<void>;
  pause(): Promise<void>;
  seek(ms: number): Promise<boolean>;
  setVolume(level: number): Promise<void>;
  setStemVolume(stem: StemName, level: number): Promise<void>;
  loadStems(): Promise<void>;
  loadState(): Promise<void>;
}

export interface PlaybackTransport {
  play: (songId: string) => Promise<PlaybackStateSnapshot>;
  resume: () => Promise<PlaybackStateSnapshot>;
  pause: () => Promise<PlaybackStateSnapshot>;
  seek: (ms: number) => Promise<PlaybackStateSnapshot>;
  setVolume: (level: number) => Promise<PlaybackStateSnapshot>;
  setStemVolume: (
    stem: StemName,
    level: number,
  ) => Promise<PlaybackStateSnapshot>;
  loadStems: () => Promise<PlaybackStateSnapshot>;
  getPlaybackState: () => Promise<PlaybackStateSnapshot>;
}

export interface PlaybackQueueOps {
  addToQueue: (songId: string) => void;
  dequeue: () => string | null;
  pushToHistory: (songId: string) => void;
  popFromHistory: () => string | null;
  removeSongIds: (songIds: string[]) => void;
}

export interface PlaybackSessionDeps {
  transport: PlaybackTransport;
  queue: PlaybackQueueOps;
  getSeparationStatus: (songId: string) => SeparationStatusSnapshot | undefined;
  nowMs?: () => number;
  onClockChange: (clock: PositionClockState) => void;
}

export function createPlaybackSession(
  deps: PlaybackSessionDeps,
): PlaybackSession {
  const nowMs = deps.nowMs ?? (() => performance.now());
  let clock: PositionClockState = {
    snapshot: null,
    positionMs: 0,
    playingSinceMs: null,
  };

  const publish = (next: PositionClockState) => {
    clock = next;
    deps.onClockChange(clock);
  };

  const tryApplyAuthoritative = (
    nextSnapshot: PlaybackStateSnapshot,
  ): boolean => {
    const reduced = reduceAuthoritativeSnapshot(clock, nextSnapshot, nowMs());
    if (!reduced) {
      return false;
    }
    publish(reduced);
    return true;
  };

  async function playSongWithOptionalStems(songId: string): Promise<void> {
    const snapshot = await deps.transport.play(songId);
    tryApplyAuthoritative(snapshot);

    if (!shouldLoadSeparatedStems(snapshot, deps.getSeparationStatus(songId))) {
      return;
    }

    const snapshotWithStems = await deps.transport.loadStems();
    if (clock.snapshot?.song_id !== songId) {
      return;
    }
    tryApplyAuthoritative(snapshotWithStems);
  }

  return {
    getPositionClock: () => clock,

    replaceClock: (next) => {
      clock = next;
    },

    applySnapshot: (snapshot) => {
      tryApplyAuthoritative(snapshot);
    },

    applyPosition: (event) => {
      const reduced = reducePositionEvent(clock, event, nowMs());
      if (reduced) {
        publish(reduced);
      }
    },

    play: async (songId) => {
      const snapshot = clock.snapshot;
      if (shouldEnqueueInsteadOfReplacingCurrentSong(snapshot, songId)) {
        deps.queue.addToQueue(songId);
        return;
      }

      if (snapshot?.song_id) {
        deps.queue.pushToHistory(snapshot.song_id);
      }

      await playSongWithOptionalStems(songId);
    },

    playNow: async (songId) => {
      const snapshot = clock.snapshot;
      if (snapshot?.song_id) {
        deps.queue.pushToHistory(snapshot.song_id);
      }

      await playSongWithOptionalStems(songId);
    },

    onEnded: async (endedSongId) => {
      const snapshot = clock.snapshot;
      if (snapshot?.song_id !== endedSongId) return;

      const nextId = deps.queue.dequeue();
      if (!nextId) return;

      deps.queue.pushToHistory(endedSongId);
      await playSongWithOptionalStems(nextId);
    },

    onTrackTransitioned: async (fromSongId, toSongId) => {
      const snapshot = clock.snapshot;
      if (snapshot?.song_id !== fromSongId && snapshot?.song_id !== toSongId) {
        return;
      }

      deps.queue.pushToHistory(fromSongId);
      deps.queue.removeSongIds([fromSongId, toSongId]);

      const currentSnapshot = await deps.transport.getPlaybackState();
      tryApplyAuthoritative(currentSnapshot);

      if (
        !shouldLoadSeparatedStems(
          currentSnapshot,
          deps.getSeparationStatus(toSongId),
        )
      ) {
        return;
      }

      const snapshotWithStems = await deps.transport.loadStems();
      if (clock.snapshot?.song_id !== toSongId) {
        return;
      }
      tryApplyAuthoritative(snapshotWithStems);
    },

    skipForward: async () => {
      const snapshot = clock.snapshot;
      const nextId = deps.queue.dequeue();
      if (!nextId) return;

      if (snapshot?.song_id) {
        deps.queue.pushToHistory(snapshot.song_id);
      }

      await playSongWithOptionalStems(nextId);
    },

    skipBack: async () => {
      const snapshot = clock.snapshot;
      if (!snapshot?.song_id) return;

      const previousSongId = deps.queue.popFromHistory();
      if (previousSongId) {
        await playSongWithOptionalStems(previousSongId);
        return;
      }

      const newSnapshot = await deps.transport.seek(0);
      tryApplyAuthoritative(newSnapshot);
    },

    resume: async () => {
      const snapshot = await deps.transport.resume();
      const authoritative = {
        ...snapshot,
        is_playing: true,
      };
      tryApplyAuthoritative(authoritative);
    },

    pause: async () => {
      const snapshot = await deps.transport.pause();
      const authoritative = {
        ...snapshot,
        is_playing: false,
      };
      tryApplyAuthoritative(authoritative);
    },

    seek: async (ms) => {
      if (!clock.snapshot?.song_id) return false;
      const clamped = Math.max(0, ms);
      const snapshot = await deps.transport.seek(clamped);

      const current = clock.snapshot;
      if (
        current?.song_id === snapshot.song_id &&
        current.transport_generation === snapshot.transport_generation
      ) {
        return true;
      }

      return tryApplyAuthoritative(snapshot);
    },

    setVolume: async (level) => {
      const clamped = Math.max(0, Math.min(1, level));
      const snapshot = await deps.transport.setVolume(clamped);
      if (isStaleTransportSnapshotForClock(clock, snapshot)) {
        return;
      }
      publish({
        ...clock,
        snapshot,
      });
    },

    setStemVolume: async (stem, level) => {
      const clamped = Math.max(0, Math.min(1, level));
      const snapshot = await deps.transport.setStemVolume(stem, clamped);
      if (isStaleTransportSnapshotForClock(clock, snapshot)) {
        return;
      }
      publish({
        ...clock,
        snapshot,
      });
    },

    loadStems: async () => {
      const snapshot = await deps.transport.loadStems();
      tryApplyAuthoritative(snapshot);
    },

    loadState: async () => {
      const snapshot = await deps.transport.getPlaybackState();
      tryApplyAuthoritative(snapshot);
    },
  };
}

function isStaleTransportSnapshotForClock(
  clock: PositionClockState,
  next: PlaybackStateSnapshot,
): boolean {
  return (
    clock.snapshot !== null &&
    next.transport_generation < clock.snapshot.transport_generation
  );
}

export {
  shouldEnqueueInsteadOfReplacingCurrentSong,
  shouldLoadSeparatedStems,
} from "./session-policies";
export type { PositionClockState } from "./position-clock";
export {
  selectCurrentPositionMs,
  selectSyncDisplayPositionMs,
  shouldAnchorPlayingSinceMs,
} from "./position-clock";
