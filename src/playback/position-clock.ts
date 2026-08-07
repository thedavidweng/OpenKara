import type {
  AirPlayOutputStateEvent,
  PlaybackPositionEvent,
  PlaybackStateSnapshot,
} from "@/types/ipc";

export interface PositionClockState {
  snapshot: PlaybackStateSnapshot | null;
  positionMs: number;
  playingSinceMs: number | null;
}

export function shouldAnchorPlayingSinceMs(
  snapshot: PlaybackStateSnapshot,
): boolean {
  return snapshot.is_playing && snapshot.state !== "buffering";
}

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
  if (
    snapshot?.is_playing &&
    snapshot.state !== "buffering" &&
    playingSinceMs !== null
  ) {
    return positionMs + (nowMs() - playingSinceMs);
  }
  return positionMs;
}

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
