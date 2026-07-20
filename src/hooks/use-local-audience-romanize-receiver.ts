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

  // Highest revision observed by this receiver instance. Both applied and
  // retained payloads bump this so a delayed older revision is ignored.
  const lastRevisionRef = useRef<number>(0);
  // Retained payload waiting for the local lyrics to match its identity.
  const pendingRef = useRef<LocalAudienceRomanizeState | null>(null);

  // Try to apply a payload against the current local lyrics. Returns
  // "applied" if it matched, "retained" if it is pending a lyric identity
  // match (including the case where local lyrics have not loaded yet), or
  // "dropped" if it targets another song and cannot match.
  const tryApply = (
    payload: LocalAudienceRomanizeState,
  ): "applied" | "retained" | "dropped" => {
    const currentSongId = useLyricsStore.getState().songId;
    const currentLines = useLyricsStore.getState().lines;

    // Local lyrics not loaded yet: retain until they arrive so an early
    // authoritative snapshot is not lost before we can validate identity.
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

  // Re-evaluate the retained pending payload when the local lyrics change.
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

  // Register the state listener and emit the initial sync request.
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
            // "retained": store as pending, replacing any older pending
            // payload (the newer revision wins).
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

      // Listener is registered; safe to request the current snapshot. The
      // main window answers with the latest revision regardless of its
      // audience-active announcement state.
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
    // The effect intentionally re-runs only on mount. applyRemoteRomanizeState
    // is a stable Zustand action; songId/lines changes are handled by the
    // re-evaluation effect above.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}
