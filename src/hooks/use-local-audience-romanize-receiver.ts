import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useLyricsStore } from "@/stores/lyrics-store";
import {
  LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
  type LocalAudienceRomanizeState,
  buildLyricsIdentity,
  emitLocalAudienceRomanizeSyncRequest,
} from "@/lib/local-audience-romanize";

/**
 * Fullscreen WebView receiver for the authoritative romanization state.
 *
 * The receiver never invokes the romanization Worker and never calls the
 * main-window store action directly. It projects the authoritative state
 * onto the local lyrics store via `applyRemoteRomanizeState()` only after
 * the payload's `songId` matches the local lyrics-store `songId` AND the
 * payload's `lyricsIdentity` matches the local source lyrics. This guards
 * against the same song temporarily holding different lyric content in the
 * two WebViews (e.g. local lyrics vs an online-upgraded set).
 *
 * Pending-state logic:
 * - A payload that arrives before the local lyrics match is retained.
 * - A newer revision always replaces an older pending payload.
 * - An older delayed revision is discarded once a newer revision has been
 *   applied or retained.
 * - When the local lyrics change, the retained pending state is re-evaluated
 *   and applied if it now matches, or dropped if it now targets another song.
 *
 * The handshake requires the listener to be registered before the initial
 * sync request is emitted; otherwise the main window's response could be
 * emitted before the receiver is ready to observe it.
 */
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
    // "retained" keeps waiting for the local lyrics to catch up.
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

      // If unmount happened before listen() resolved, clean up now.
      if (cancelled) {
        unlisten();
        unlisten = null;
        return;
      }

      void emitLocalAudienceRomanizeSyncRequest().catch(() => {
        // Sync request delivery failure is non-fatal; the next authoritative
        // state change will still be emitted.
      });
    };

    void setup();

    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}
