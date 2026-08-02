import { useEffect, useRef } from "react";
import { usePlayerStore } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";

export function useLyricsAutoFetch(enabled = true) {
  const songId =
    usePlayerStore((state) => state.snapshot?.song_id) ?? undefined;
  const fetchLyrics = useLyricsStore((state) => state.fetchLyrics);
  const previousSongId = useRef<string | undefined>(undefined);

  useEffect(() => {
    if (!enabled) {
      previousSongId.current = undefined;
      return;
    }
    if (songId && songId !== previousSongId.current) {
      fetchLyrics(songId);
    }
    previousSongId.current = songId;
  }, [enabled, fetchLyrics, songId]);
}
