import type {
  PlaybackStateSnapshot,
  SeparationStatusSnapshot,
} from "@/types/ipc";

export interface PlaybackWorkflowDeps {
  getPlayerSnapshot: () => PlaybackStateSnapshot | null;
  play: (songId: string) => Promise<PlaybackStateSnapshot>;
  loadStems: () => Promise<PlaybackStateSnapshot>;
  getSeparationStatus: (songId: string) => SeparationStatusSnapshot | undefined;
  applySnapshot: (snapshot: PlaybackStateSnapshot) => void;
  seek: (ms: number) => Promise<PlaybackStateSnapshot>;
  addToQueue: (songId: string) => void;
  dequeue: () => string | null;
  pushToHistory: (songId: string) => void;
  popFromHistory: () => string | null;
}

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

async function playSongWithOptionalStems(
  songId: string,
  deps: PlaybackWorkflowDeps,
): Promise<void> {
  const snapshot = await deps.play(songId);
  deps.applySnapshot(snapshot);

  if (!shouldLoadSeparatedStems(snapshot, deps.getSeparationStatus(songId))) {
    return;
  }

  const snapshotWithStems = await deps.loadStems();
  // F6: Skip applying stems snapshot if the song changed during loadStems().
  if (deps.getPlayerSnapshot()?.song_id !== songId) {
    return;
  }
  deps.applySnapshot(snapshotWithStems);
}

export function createPlaybackWorkflow(deps: PlaybackWorkflowDeps) {
  return {
    async playSong(songId: string): Promise<void> {
      const snapshot = deps.getPlayerSnapshot();
      if (shouldEnqueueInsteadOfReplacingCurrentSong(snapshot, songId)) {
        deps.addToQueue(songId);
        return;
      }

      if (snapshot?.song_id) {
        deps.pushToHistory(snapshot.song_id);
      }

      await playSongWithOptionalStems(songId, deps);
    },

    async playNow(songId: string): Promise<void> {
      const snapshot = deps.getPlayerSnapshot();
      if (snapshot?.song_id) {
        deps.pushToHistory(snapshot.song_id);
      }

      await playSongWithOptionalStems(songId, deps);
    },

    async playNextFromQueue(endedSongId: string): Promise<void> {
      const snapshot = deps.getPlayerSnapshot();
      if (snapshot?.song_id !== endedSongId) return;

      const nextId = deps.dequeue();
      if (!nextId) return;

      deps.pushToHistory(endedSongId);
      await playSongWithOptionalStems(nextId, deps);
    },

    async skipForward(): Promise<void> {
      const snapshot = deps.getPlayerSnapshot();
      const nextId = deps.dequeue();
      if (!nextId) return;

      if (snapshot?.song_id) {
        deps.pushToHistory(snapshot.song_id);
      }

      await playSongWithOptionalStems(nextId, deps);
    },

    async skipBack(): Promise<void> {
      const snapshot = deps.getPlayerSnapshot();
      if (!snapshot?.song_id) return;

      const previousSongId = deps.popFromHistory();
      if (previousSongId) {
        await playSongWithOptionalStems(previousSongId, deps);
        return;
      }

      // NOTE: Unlike the old player-store code, applySnapshot here also updates
      // playingSinceMs from the seek response, keeping the position extrapolation
      // consistent after a restart-from-beginning.
      const newSnapshot = await deps.seek(0);
      deps.applySnapshot(newSnapshot);
    },
  };
}
