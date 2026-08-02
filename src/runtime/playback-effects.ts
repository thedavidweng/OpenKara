import { useEffect } from "react";
import { usePlayerStore } from "@/stores/player-store";
import { useQueueStore } from "@/stores/queue-store";
import { notifyError } from "@/lib/errors";
import * as api from "@/lib/tauri";

export function usePreloadCandidateEffect(enabled: boolean) {
  const currentSongId =
    usePlayerStore((state) => state.snapshot?.song_id) ?? null;
  const queue = useQueueStore((state) => state.queue);
  const nextCandidate = (() => {
    if (queue.length === 0) return null;
    if (queue[0] === currentSongId) {
      return queue.length > 1 ? queue[1] : null;
    }
    return queue[0];
  })();

  useEffect(() => {
    if (!enabled) return;
    api.setPreloadCandidate(nextCandidate).catch(notifyError);
  }, [enabled, nextCandidate]);
}
