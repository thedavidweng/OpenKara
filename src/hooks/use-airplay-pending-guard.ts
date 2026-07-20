import { useEffect, useRef } from "react";
import { usePlayerStore } from "@/stores/player-store";

export function useAirPlayPendingGuard(
  songId: string | null | undefined,
  isPlainText: boolean,
  isAudience: boolean,
  isAirPlayRemotePagingTarget: boolean,
  airPlayPlainTextPagePending: boolean,
): void {
  const lastPendingSongIdRef = useRef<string | null>(null);

  useEffect(() => {
    if (!airPlayPlainTextPagePending) {
      lastPendingSongIdRef.current = songId ?? null;
      return;
    }

    const songChanged =
      lastPendingSongIdRef.current !== null &&
      lastPendingSongIdRef.current !== (songId ?? null);
    if (
      isAudience ||
      songChanged ||
      !isPlainText ||
      !isAirPlayRemotePagingTarget
    ) {
      usePlayerStore.getState().clearAirPlayPlainTextPagePending();
      return;
    }

    lastPendingSongIdRef.current = songId ?? null;
  }, [
    airPlayPlainTextPagePending,
    isAirPlayRemotePagingTarget,
    isAudience,
    isPlainText,
    songId,
  ]);
}
