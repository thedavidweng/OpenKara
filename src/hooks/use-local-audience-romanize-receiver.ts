import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useLyricsStore } from "@/stores/lyrics-store";
import {
  LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
  type LocalAudienceRomanizeState,
  buildLyricsIdentity,
  emitLocalAudienceRomanizeSyncRequest,
} from "@/lib/local-audience-romanize";

export function useLocalAudienceRomanizeReceiver(): void {
  const songId = useLyricsStore((s) => s.songId);
  const lines = useLyricsStore((s) => s.lines);
  const applyRemoteRomanizeState = useLyricsStore(
    (s) => s.applyRemoteRomanizeState,
  );

  const lastRevisionRef = useRef<number>(0);
  const pendingRef = useRef<LocalAudienceRomanizeState | null>(null);

  const tryApply = (
    payload: LocalAudienceRomanizeState,
  ): "applied" | "retained" | "dropped" => {
    const currentSongId = useLyricsStore.getState().songId;
    const currentLines = useLyricsStore.getState().lines;

    if (currentSongId === null) {
      return "retained";
    }

    if (payload.songId !== currentSongId) {
      return "dropped";
    }

    const localIdentity = buildLyricsIdentity(currentLines);
    if (payload.lyricsIdentity === localIdentity) {
      applyRemoteRomanizeState(payload);
      return "applied";
    }

    return "retained";
  };

  useEffect(() => {
    const pending = pendingRef.current;
    if (pending === null) return;
    const result = tryApply(pending);
    if (result === "applied" || result === "dropped") {
      pendingRef.current = null;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [songId, lines]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    const setup = async () => {
      unlisten = await listen<LocalAudienceRomanizeState>(
        LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
        (event) => {
          if (cancelled) return;
          const payload = event.payload;
          if (payload.revision <= lastRevisionRef.current) {
            // Older or duplicate revision; ignore. This guards against a
            // delayed event from a previous song overwriting newer state.
            return;
          }
          lastRevisionRef.current = payload.revision;

          const result = tryApply(payload);
          if (result === "applied" || result === "dropped") {
            pendingRef.current = null;
          } else {
            pendingRef.current = payload;
          }
        },
      );

      if (cancelled) {
        unlisten();
        unlisten = null;
        return;
      }

      void emitLocalAudienceRomanizeSyncRequest().catch(() => {});
    };

    void setup();

    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}
