import type {
  PlaybackStateSnapshot,
  SeparationStatusSnapshot,
} from "@/types/ipc";

export function shouldEnqueueInsteadOfReplacingCurrentSong(
  currentSnapshot: PlaybackStateSnapshot | null,
  requestedSongId: string,
): boolean {
  return Boolean(
    currentSnapshot?.song_id && currentSnapshot.song_id !== requestedSongId,
  );
}

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
