import type {
  AirPlayOutputStateEvent,
  PlaybackPositionEvent,
  PlaybackStateSnapshot,
} from "@/types/ipc";

/**
 * Authoritative local playback clock.
 *
 * RATIONALE: IPC position events are asynchronous and can be delayed, dropped,
 * or reordered under focus changes / event-loop pressure. UI extrapolates from
 * the last authoritative position using a local monotonic clock so lyrics and
 * seek UI stay smooth without polling the backend.
 */
export interface PositionClockState {
  snapshot: PlaybackStateSnapshot | null;
  positionMs: number;
  /** Monotonic-ms of the last authoritative position update; null when paused/stopped. */
  playingSinceMs: number | null;
}

export function shouldAnchorPlayingSinceMs(
  snapshot: PlaybackStateSnapshot,
): boolean {
  return snapshot.is_playing && snapshot.state !== "buffering";
}

/**
 * RATIONALE: (positionMs, playingSinceMs) must be a consistent pair —
 * positionMs as measured at monotonic time playingSinceMs. Every reducer that
 * replaces positionMs with a fresh backend position must re-anchor to nowMs.
 * Keeping a stale anchor while adopting a fresh position makes
 * selectCurrentPositionMs double-count elapsed time: the displayed clock runs
 * at ~2× real speed and races past the last lyric line, which froze lyric
 * auto-scroll mid-song and shortly after every click-to-seek.
 */
export function resolvePlayingSinceMs(
  nextSnapshot: PlaybackStateSnapshot,
  nowMs: number,
): number | null {
  return shouldAnchorPlayingSinceMs(nextSnapshot) ? nowMs : null;
}

export function isStaleTransportSnapshot(
  current: PlaybackStateSnapshot | null,
  next: PlaybackStateSnapshot,
): boolean {
  return (
    current !== null && next.transport_generation < current.transport_generation
  );
}

export function shouldReplaceSnapshotFromPositionEvent(
  current: PlaybackStateSnapshot | null,
  next: PlaybackStateSnapshot,
): boolean {
  // RATIONALE: transport_generation is part of the snapshot identity. Seek /
  // resume / pause bump it on the backend. If we only patch positionMs and
  // keep an older generation on the stored snapshot, delayed pre-seek
  // position events fail the stale check (their gen is not *less* than the
  // still-stale stored gen) and yank the clock back before the seek.
  return (
    current?.song_id !== next.song_id ||
    current.transport_generation !== next.transport_generation ||
    current.state !== next.state ||
    current.is_playing !== next.is_playing ||
    current.duration_ms !== next.duration_ms ||
    current.volume !== next.volume ||
    current.has_stems !== next.has_stems ||
    current.stem_mode !== next.stem_mode ||
    current.stem_volumes.vocals !== next.stem_volumes.vocals ||
    current.stem_volumes.drums !== next.stem_volumes.drums ||
    current.stem_volumes.bass !== next.stem_volumes.bass ||
    current.stem_volumes.other !== next.stem_volumes.other
  );
}

export function selectCurrentPositionMs(
  state: Pick<PositionClockState, "snapshot" | "positionMs" | "playingSinceMs">,
  nowMs: () => number = () => performance.now(),
): number {
  const { snapshot, positionMs, playingSinceMs } = state;
  // Do not extrapolate during buffer underrun — backend position is frozen even
  // though is_playing still reflects transport intent.
  if (
    snapshot?.is_playing &&
    snapshot.state !== "buffering" &&
    playingSinceMs !== null
  ) {
    return positionMs + (nowMs() - playingSinceMs);
  }
  return positionMs;
}

// RATIONALE: Once AirPlay is active, the audience surface must follow the TV's
// displayed clock rather than the local playback clock. That keeps the
// standard UI synchronized with the remote audience surface without changing
// which window is allowed to render audience styling.
export function selectSyncDisplayPositionMs(
  state: Pick<PositionClockState, "positionMs"> & {
    airPlayOutput: AirPlayOutputStateEvent;
  },
): number {
  return state.airPlayOutput.active &&
    state.airPlayOutput.displayedPositionMs !== null
    ? state.airPlayOutput.displayedPositionMs
    : state.positionMs;
}

/** Apply a command-response or hydrate snapshot. Returns null if stale. */
export function reduceAuthoritativeSnapshot(
  prev: PositionClockState,
  nextSnapshot: PlaybackStateSnapshot,
  nowMs: number,
): PositionClockState | null {
  if (isStaleTransportSnapshot(prev.snapshot, nextSnapshot)) {
    return null;
  }

  return {
    snapshot: nextSnapshot,
    positionMs: nextSnapshot.position_ms,
    playingSinceMs: resolvePlayingSinceMs(nextSnapshot, nowMs),
  };
}

/** Apply a playback-position IPC event. Returns null if ignored/stale. */
export function reducePositionEvent(
  prev: PositionClockState,
  event: PlaybackPositionEvent,
  nowMs: number,
): PositionClockState | null {
  const currentSnapshot = prev.snapshot;
  const nextSnapshot = event.snapshot;
  if (event.transport_generation !== nextSnapshot.transport_generation) {
    return null;
  }
  if (isStaleTransportSnapshot(currentSnapshot, nextSnapshot)) {
    return null;
  }

  if (shouldReplaceSnapshotFromPositionEvent(currentSnapshot, nextSnapshot)) {
    return {
      snapshot: nextSnapshot,
      positionMs: nextSnapshot.position_ms,
      playingSinceMs: resolvePlayingSinceMs(nextSnapshot, nowMs),
    };
  }

  return {
    positionMs: nextSnapshot.position_ms,
    playingSinceMs: resolvePlayingSinceMs(nextSnapshot, nowMs),
    snapshot:
      currentSnapshot &&
      (nextSnapshot.is_playing !== currentSnapshot.is_playing ||
        nextSnapshot.state !== currentSnapshot.state ||
        nextSnapshot.buffered_ms !== currentSnapshot.buffered_ms)
        ? {
            ...currentSnapshot,
            is_playing: nextSnapshot.is_playing,
            state: nextSnapshot.state,
            buffered_ms: nextSnapshot.buffered_ms,
          }
        : (currentSnapshot ?? nextSnapshot),
  };
}
