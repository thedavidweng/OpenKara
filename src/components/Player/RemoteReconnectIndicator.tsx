import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useRemotePlaybackStore } from "@/stores/remote-playback-store";
import { usePlayerStore } from "@/stores/player-store";

/**
 * Reconnect indicator for remote playback (PR #8, issue #151).
 *
 * Renders a compact "reconnecting…" / "resync" / "failed" badge in the
 * playback bar when the backend reconnect coordinator is active for the
 * current song. The indicator auto-resets to idle when the user switches
 * songs or playback stops.
 */
export function RemoteReconnectIndicator() {
  const { t } = useTranslation();
  const reconnectState = useRemotePlaybackStore((s) => s.reconnectState);
  const attempt = useRemotePlaybackStore((s) => s.attempt);
  const maxAttempts = useRemotePlaybackStore((s) => s.maxAttempts);
  const reason = useRemotePlaybackStore((s) => s.reason);
  const resyncDeltaMs = useRemotePlaybackStore((s) => s.resyncDeltaMs);
  const reconnectSongId = useRemotePlaybackStore((s) => s.songId);
  const resetReconnect = useRemotePlaybackStore((s) => s.reset);
  const currentSongId = usePlayerStore((s) => s.snapshot?.song_id) ?? null;

  useEffect(() => {
    if (
      reconnectSongId !== null &&
      currentSongId !== null &&
      reconnectSongId !== currentSongId
    ) {
      resetReconnect();
    }
    if (reconnectSongId !== null && currentSongId === null) {
      resetReconnect();
    }
  }, [reconnectSongId, currentSongId, resetReconnect]);

  if (reconnectState === "idle") {
    return null;
  }

  if (reconnectState === "reconnecting") {
    return (
      <span
        role="status"
        aria-live="polite"
        data-testid="remote-reconnect-indicator"
        data-reconnect-state="reconnecting"
        className="flex items-center gap-1.5 rounded-md bg-[var(--color-accent)]/10 px-2 py-0.5 text-[11px] text-[var(--color-accent)]"
        title={reason ?? undefined}
      >
        <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-[var(--color-accent)]" />
        {t("player.reconnecting", {
          attempt,
          max: maxAttempts,
          defaultValue: "Reconnecting… ({{attempt}}/{{max}})",
        })}
      </span>
    );
  }

  if (reconnectState === "resync") {
    return (
      <span
        role="status"
        aria-live="polite"
        data-testid="remote-reconnect-indicator"
        data-reconnect-state="resync"
        className="flex items-center gap-1.5 rounded-md bg-[var(--color-ghost-hover)] px-2 py-0.5 text-[11px] text-[var(--color-text-dim)]"
      >
        {t("player.resync", {
          delta: resyncDeltaMs ?? 0,
          defaultValue: "Resynced −{{delta}} ms",
        })}
      </span>
    );
  }

  // failed
  return (
    <span
      role="alert"
      aria-live="assertive"
      data-testid="remote-reconnect-indicator"
      data-reconnect-state="failed"
      className="flex items-center gap-1.5 rounded-md bg-[var(--color-destructive)]/10 px-2 py-0.5 text-[11px] text-[var(--color-destructive)]"
      title={reason ?? undefined}
    >
      {t("player.reconnectFailed", {
        defaultValue: "Reconnect failed",
      })}
    </span>
  );
}
