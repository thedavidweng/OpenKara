import type {
  PlaybackPositionEvent,
  PlaybackStateSnapshot,
  SeparationStatusSnapshot,
  StemName,
  TrackTransitionedEvent,
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
   * matches the backend's current track. The event includes an authoritative
   * post-transition snapshot and a monotonic serial for idempotent handling.
   * Also restores separated stems for the new song — the gapless swap
   * creates a plain track, so vocal-removal / karaoke stem mode must be
   * reloaded. */
  onTrackTransitioned(event: TrackTransitionedEvent): Promise<void>;
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
  /** #88: Set or clear the gapless preload candidate. Called by the
   * session after a gapless transition reconciles the queue so the
   * backend starts decoding the new queue head. */
  setPreloadCandidate: (songId: string | null) => Promise<void>;
}

/** Queue/history seam — not part of the public session API. */
export interface PlaybackQueueOps {
  addToQueue: (songId: string) => void;
  dequeue: () => string | null;
  pushToHistory: (songId: string) => void;
  popFromHistory: () => string | null;
  /** #88: Reconcile queue and history after a gapless transition. */
  reconcileGaplessTransition: (fromSongId: string, toSongId: string) => void;
  /** #88: Peek at the current queue head without removing it. Returns
   * `null` when the queue is empty. Used by `onTrackTransitioned` to
   * update the preload candidate after reconciliation. */
  peekHead: () => string | null;
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
  // #88: Monotonic serial of the last applied gapless transition. Used for
  // idempotent dedup — the backend may emit duplicate events and the
  // frontend must not advance the queue twice. Resets on process restart.
  let lastAppliedTransitionSerial = 0;

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
    // F6: Skip applying stems if the song changed during loadStems().
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

    onTrackTransitioned: async (event) => {
      // #88: Idempotent serial dedup — ignore transitions we have already
      // applied. The serial resets on process restart, so this only needs
      // to persist for the WebView lifetime.
      if (event.transitionSerial <= lastAppliedTransitionSerial) {
        return;
      }
      lastAppliedTransitionSerial = event.transitionSerial;

      const { fromSongId, toSongId, state } = event;

      // #88: Apply the authoritative post-transition snapshot before
      // subsequent position events. This ensures the clock holds
      // `toSongId` immediately, even if a position event arrives later
      // with stale data.
      //
      // #89: Gate queue/history reconciliation on the snapshot being
      // accepted. If the user manually started a different track during
      // the ~33ms window between the backend's gapless swap and the
      // position emitter draining the transition, the clock will hold a
      // newer transport_generation and `tryApplyAuthoritative` rejects
      // the stale transition snapshot. In that race, the queue must NOT
      // be reconciled — the user has already moved to an unrelated track
      // and removing `toSongId` / pushing `fromSongId` to history would
      // corrupt the queue. The gapless swap does not bump
      // transport_generation, so a position event that arrived first
      // with the same generation will not trigger this rejection.
      const accepted = tryApplyAuthoritative(state);
      if (!accepted) {
        return;
      }

      // #88: Reconcile queue and history via the named store action.
      // This removes the first queue entry matching `toSongId` and pushes
      // `fromSongId` to history exactly once. A missing `toSongId` still
      // applies player state and history; it is not an error.
      deps.queue.reconcileGaplessTransition(fromSongId, toSongId);

      // #88: Update the preload candidate from the resulting queue head.
      // The backend already swapped to `toSongId`; the new queue head is
      // the next song to preload for a future gapless/crossfade transition.
      // Errors are swallowed — a failed preload must not surface as a
      // playback error; the normal `playback-ended` fallback still applies.
      const nextCandidate = deps.queue.peekHead();
      deps.transport.setPreloadCandidate(nextCandidate).catch(() => {
        /* best-effort: preload failure is silent per #88 */
      });

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
      // F6-style guard: skip applying stems if the song changed again
      // before loadStems() resolved.
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

      // NOTE: applySnapshot also updates playingSinceMs from the seek response,
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
