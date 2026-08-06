import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { usePlayerStore } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import {
  LOCAL_AUDIENCE_ROMANIZE_SET_EVENT,
  LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT,
  type LocalAudienceRomanizeSetRequest,
  type LocalAudienceRomanizeState,
  buildLyricsIdentity,
  emitLocalAudienceRomanizeState,
} from "@/lib/local-audience-romanize";

let revisionCounter = 0;

function nextRevision(): number {
  return ++revisionCounter;
}

export function useLocalAudienceRomanizeRuntime(enabled: boolean): void {
  const songId = useLyricsStore((s) => s.songId);
  const lines = useLyricsStore((s) => s.lines);
  const showRomanized = useLyricsStore((s) => s.showRomanized);
  const isRomanizing = useLyricsStore((s) => s.isRomanizing);
  const romanizedLines = useLyricsStore((s) => s.romanizedLines);
  const setRomanizedVisibility = useLyricsStore(
    (s) => s.setRomanizedVisibility,
  );
  const localAudienceOutputActive = usePlayerStore(
    (s) => s.localAudienceOutputActive,
  );

  const latestSnapshotRef = useRef<LocalAudienceRomanizeState | null>(null);
  const listenersReadyRef = useRef(false);

  useEffect(() => {
    if (!enabled) {
      latestSnapshotRef.current = null;
      return;
    }

    const snapshot: LocalAudienceRomanizeState = {
      revision: nextRevision(),
      songId,
      lyricsIdentity: buildLyricsIdentity(lines),
      showRomanized,
      isRomanizing,
      romanizedLines,
    };
    latestSnapshotRef.current = snapshot;

    if (!localAudienceOutputActive) {
      return;
    }

    void emitLocalAudienceRomanizeState(snapshot).catch(() => {});
  }, [
    enabled,
    songId,
    lines,
    showRomanized,
    isRomanizing,
    romanizedLines,
    localAudienceOutputActive,
  ]);

  useEffect(() => {
    if (!enabled) {
      listenersReadyRef.current = false;
      return;
    }

    let cancelled = false;
    const unlisteners: (() => void)[] = [];

    const setup = async () => {
      const syncUnlisten = await listen(
        LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT,
        () => {
          if (cancelled) return;
          const snapshot = latestSnapshotRef.current;
          if (!snapshot) return;
          void emitLocalAudienceRomanizeState(snapshot).catch(() => {});
        },
      );

      const setUnlisten = await listen<LocalAudienceRomanizeSetRequest>(
        LOCAL_AUDIENCE_ROMANIZE_SET_EVENT,
        (event) => {
          if (cancelled) return;
          const request = event.payload;
          if (request.songId !== useLyricsStore.getState().songId) {
            return;
          }
          setRomanizedVisibility(request.showRomanized);
        },
      );

      if (cancelled) {
        syncUnlisten();
        setUnlisten();
        return;
      }

      unlisteners.push(syncUnlisten, setUnlisten);
      listenersReadyRef.current = true;
    };

    void setup();

    return () => {
      cancelled = true;
      listenersReadyRef.current = false;
      unlisteners.forEach((fn) => fn());
    };
  }, [enabled, setRomanizedVisibility]);
}
