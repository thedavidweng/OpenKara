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

/**
 * Small public surface for song lifecycle + position clock.
 * Queue/history and stem-load policy live behind this interface so
 * "what happens when a song ends?" is one module, not a scatter across stores.
 */
export interface PlaybackSession {
  play(songId: string): Promise<void>;
  playNow(songId: string): Promise<void>;
  skipForward(): Promise<void>;
  skipBack(): Promise<void>;
  /** Queue advance on backend `playback-ended` (song_id guarded). */
  onEnded(endedSongId: string): Promise<void>;
  /** #88: Reconcile queue head after a gapless `track-transitioned` event.
   * The backend already swapped to the new song; the frontend must remove
   * the old song from the queue and push it to history so the queue head
   * matches the backend's current track. Also restores separated stems for
   * the new song — the gapless swap creates a plain track, so vocal-removal
   * / karaoke stem mode must be reloaded. */
  onTrackTransitioned(fromSongId: string, toSongId: string): Promise<void>;
  applyPosition(event: PlaybackPositionEvent): void;
  applySnapshot(snapshot: PlaybackStateSnapshot): void;
  getPositionClock(): PositionClockState;
  /** Replace clock from peer webview sync (adapter rebases playingSinceMs). */
  replaceClock(clock: PositionClockState): void;

  resume(): Promise<void>;
  pause(): Promise<void>;
  /** Returns true when the authoritative seek snapshot was accepted. */
  seek(ms: number): Promise<boolean>;
  setVolume(level: number): Promise<void>;
  setStemVolume(stem: StemName, level: number): Promise<void>;
  loadStems(): Promise<void>;
  loadState(): Promise<void>;
}

/** Transport seam — backend IPC only. */
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

/** Queue/history seam — not part of the public session API. */
export interface PlaybackQueueOps {
  addToQueue: (songId: string) => void;
  dequeue: () => string | null;
  pushToHistory: (songId: string) => void;
  popFromHistory: () => string | null;
  /** #88: Remove songs from the queue by ID (used by gapless reconciliation). */
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
    // Skip applying stems if the song changed during loadStems().
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
      // #88: The backend already swapped to `toSongId` via a gapless
      // transition. Reconcile the queue: remove `fromSongId` if it is
      // still the queue head (it was the song that just finished), push
      // it to history, and remove `toSongId` from the queue if present
      // (it was the prepared candidate and is now playing).
      //
      // The backend emits `track-transitioned` BEFORE the next
      // `playback-position` event, so the clock may still hold
      // `fromSongId` (the old song). Accept both `fromSongId` and
      // `toSongId` as valid current states; only skip if the clock holds
      // a completely different song (stale transition after the user
      // manually started another track).
      const snapshot = clock.snapshot;
      if (snapshot?.song_id !== fromSongId && snapshot?.song_id !== toSongId) {
        return;
      }

      deps.queue.pushToHistory(fromSongId);
      // Remove the from-song from the queue if it's still there (it may
      // have already been dequeued by the preload scheduler's caller).
      deps.queue.removeSongIds([fromSongId, toSongId]);

      // #88: The gapless swap creates a plain track (stems are not
      // preloaded), so vocal-removal / karaoke stem mode is lost. Mirror
      // `playSongWithOptionalStems`: fetch the current state, check whether
      // stems should be loaded for the new song, and call `loadStems()`.
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
      // Skip applying stems if the song changed again before loadStems()
      // resolved.
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

      // applySnapshot also updates playingSinceMs from the seek response,
      // keeping position extrapolation consistent after restart-from-beginning.
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

      // The Rust seek command emits playback-position before returning its
      // snapshot. The audio thread can leave seek buffering quickly, so a
      // newer same-generation `playing` event may already be installed by the
      // time the older command response reaches JavaScript. Treat the current
      // clock as authoritative once it has adopted this seek generation;
      // replaying the response would regress state back to `buffering`, clear
      // playingSinceMs, and freeze lyrics until another command re-anchors it.
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
      // Volume changes do not re-anchor the playhead clock.
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

// Re-export pure helpers used by tests / adapters without pulling reducers.
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
