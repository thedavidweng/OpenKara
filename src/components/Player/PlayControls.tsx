import { Loader2, Play, Pause, SkipBack, SkipForward } from "lucide-react";
import { useTranslation } from "react-i18next";
import { usePlayerStore } from "@/stores/player-store";
import type { PlaybackBarDensity } from "./playback-bar-layout";

interface PlayControlsProps {
  density?: PlaybackBarDensity;
  previewMode?: boolean;
}

export function PlayControls({
  density = "relaxed",
  previewMode = false,
}: PlayControlsProps = {}) {
  const { t } = useTranslation();
  const snapshot = usePlayerStore((s) => s.snapshot);
  const resume = usePlayerStore((s) => s.resume);
  const pause = usePlayerStore((s) => s.pause);
  const skipBack = usePlayerStore((s) => s.skipBack);
  const skipForward = usePlayerStore((s) => s.skipForward);
  const isPlaying = snapshot?.is_playing ?? false;
  const isStarting = snapshot?.state === "loading";
  const toggleLabel = isStarting
    ? t("player.loading")
    : isPlaying
      ? t("player.pause")
      : t("player.play");

  const handleToggle = () => {
    if (isStarting) return;
    if (isPlaying) {
      pause();
    } else if (snapshot?.song_id) {
      resume();
    }
  };

  return (
    <div
      className={`flex items-center text-[var(--color-control-primary)] ${
        density === "relaxed"
          ? "gap-4"
          : density === "compact"
            ? "gap-2.5"
            : "gap-2"
      }`}
      data-play-controls-visual-variant="unified"
    >
      <button
        onClick={skipBack}
        className="motion-icon-button rounded-full p-2 opacity-80 hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)] hover:opacity-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
        aria-label={t("player.previous")}
      >
        <SkipBack size={20} fill="currentColor" />
      </button>
      <button
        onClick={handleToggle}
        disabled={isStarting}
        className="motion-icon-button flex h-10 w-10 items-center justify-center rounded-full bg-[var(--color-control-primary)] text-[var(--color-control-primary-foreground)] shadow-[0_10px_24px_rgba(0,0,0,0.22)] hover:bg-[color-mix(in_srgb,var(--color-control-primary)_90%,white)] hover:shadow-[0_14px_28px_rgba(0,0,0,0.28)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-control-primary)]/50 disabled:cursor-default disabled:opacity-80"
        aria-label={toggleLabel}
        aria-busy={isStarting}
        data-preview-play-toggle={previewMode ? "true" : undefined}
      >
        {isStarting ? (
          <Loader2 size={16} className="animate-spin" />
        ) : isPlaying ? (
          <Pause size={16} fill="currentColor" />
        ) : (
          <Play size={16} fill="currentColor" className="ml-0.5" />
        )}
      </button>
      <button
        onClick={skipForward}
        className="motion-icon-button rounded-full p-2 opacity-80 hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)] hover:opacity-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
        aria-label={t("player.next")}
      >
        <SkipForward size={20} fill="currentColor" />
      </button>
    </div>
  );
}
