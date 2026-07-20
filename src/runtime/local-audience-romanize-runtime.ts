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

// Module-local monotonic revision counter. Each emitted snapshot carries a
// strictly increasing revision so the fullscreen receiver can discard stale
// payloads that arrive after a newer state was already applied.
let revisionCounter = 0;

function nextRevision(): number {
  return ++revisionCounter;
}

/**
 * Main-window authoritative romanization runtime. Mount once from
 * `useAppRuntime` so panel unmounts, CDG rendering, or layout changes never
 * interrupt synchronization. The main window is the only romanization
 * computation owner; this runtime projects its state to the fullscreen
 * audience window and services explicit set requests from the fullscreen
 * Romanize control.
 *
 * The runtime emits a fresh snapshot whenever the authoritative state
 * changes while local audience output is active, and answers sync requests
 * immediately regardless of the audience-active announcement state so a
 * race between the announcement and the listener registration cannot drop
 * the initial snapshot.
 */
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

  // Latest snapshot kept in a ref so the sync-request handler can answer
  // without re-reading the store and without depending on the latest render.
  const latestSnapshotRef = useRef<LocalAudienceRomanizeState | null>(null);
  // Track whether the runtime listeners have been registered so the
  // sync-request handler is only attached once.
  const listenersReadyRef = useRef(false);

  // Re-emit the authoritative snapshot whenever the source state changes.
  // The effect depends on the projected values so a no-op render does not
  // produce a duplicate event; the revision counter guarantees the receiver
  // can still distinguish genuine state changes from re-emissions.
  useEffect(() => {
    if (!enabled) {
      latestSnapshotRef.current = null;
      return;
    }

    // Always build the snapshot so the sync-request handler can answer with
    // the latest state even when the audience window is not yet announced
    // as active (the announcement may lag behind the listener registration).
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

    // Emit failures are auxiliary-output failures; they must not interrupt
    // local lyrics or playback.
    void emitLocalAudienceRomanizeState(snapshot).catch(() => {
      // The fullscreen window may have closed mid-emit; the next change
      // will retry.
    });
  }, [
    enabled,
    songId,
    lines,
    showRomanized,
    isRomanizing,
    romanizedLines,
    localAudienceOutputActive,
  ]);

  // Register listeners for sync requests and explicit set requests. These
  // are independent of the projected state so they attach once per mount.
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
          // Answer with the latest snapshot regardless of
          // localAudienceOutputActive so a race between the announcement
          // and the fullscreen listener registration cannot drop the
          // initial state.
          void emitLocalAudienceRomanizeState(snapshot).catch(() => {
            // Auxiliary sync delivery failure is non-fatal.
          });
        },
      );

      const setUnlisten = await listen<LocalAudienceRomanizeSetRequest>(
        LOCAL_AUDIENCE_ROMANIZE_SET_EVENT,
        (event) => {
          if (cancelled) return;
          const request = event.payload;
          // Validate the request against the current authoritative song
          // before applying. A stale request targeting a previous song
          // must never toggle the current song's romanization.
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
