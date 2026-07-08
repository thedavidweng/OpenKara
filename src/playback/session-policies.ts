import type {
  PlaybackStateSnapshot,
  SeparationStatusSnapshot,
} from "@/types/ipc";

/** Enqueue when a different song is already loaded (karaoke host flow). */
export function shouldEnqueueInsteadOfReplacingCurrentSong(
  currentSnapshot: PlaybackStateSnapshot | null,
  requestedSongId: string,
): boolean {
  return Boolean(
    currentSnapshot?.song_id && currentSnapshot.song_id !== requestedSongId,
  );
}

/** Auto-load stems once separation finished and transport is ready. */
export function shouldLoadSeparatedStems(
  snapshot: PlaybackStateSnapshot,
  separationStatus: SeparationStatusSnapshot | undefined,
): boolean {
  return (
    snapshot.state !== "loading" &&
    separationStatus?.state === "completed" &&
    !snapshot.has_stems
  );
}
