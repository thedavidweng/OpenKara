import { useRef, useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { usePlayerStore, selectCurrentPositionMs } from "@/stores/player-store";
import { formatDuration } from "@/lib/format";
import {
  PLAYBACK_BAR_SEEK_MIN_WIDTH_CLASS,
  PLAYBACK_BAR_SEEK_RAIL_MIN_WIDTH_CLASS,
  PLAYBACK_BAR_TIME_LABEL_WIDTH_CLASS,
  type PlaybackBarDensity,
} from "./playback-bar-layout";

interface SeekBarProps {
  density?: PlaybackBarDensity;
}

export function SeekBar({ density = "relaxed" }: SeekBarProps = {}) {
  const { t } = useTranslation();
  const snapshot = usePlayerStore((s) => s.snapshot);
  const positionMs = usePlayerStore((s) => s.positionMs);
  const playingSinceMs = usePlayerStore((s) => s.playingSinceMs);
  const seek = usePlayerStore((s) => s.seek);

  // Use a rAF loop to smoothly extrapolate the current position between
  // IPC position events. This prevents the progress bar from jumping
  // per-event and provides buttery-smooth animation.
  const [displayPositionMs, setDisplayPositionMs] = useState(positionMs);

  useEffect(() => {
    let rafId: number;
    const tick = () => {
      const current = selectCurrentPositionMs({
        snapshot,
        positionMs,
        playingSinceMs,
      });
      setDisplayPositionMs(current);
      rafId = requestAnimationFrame(tick);
    };
    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, [snapshot, positionMs, playingSinceMs]);

  const durationMs = snapshot?.duration_ms ?? 0;
  const progressPercent =
    durationMs > 0 ? (displayPositionMs / durationMs) * 100 : 0;

  const barRef = useRef<HTMLDivElement>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [dragPercent, setDragPercent] = useState(0);

  const getPercentFromEvent = useCallback((clientX: number) => {
    if (!barRef.current) return 0;
    const rect = barRef.current.getBoundingClientRect();
    return Math.max(
      0,
      Math.min(100, ((clientX - rect.left) / rect.width) * 100),
    );
  }, []);

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      setIsDragging(true);
      setDragPercent(getPercentFromEvent(e.clientX));
    },
    [getPercentFromEvent],
  );

  useEffect(() => {
    if (!isDragging) return;

    const handleMouseMove = (e: MouseEvent) => {
      setDragPercent(getPercentFromEvent(e.clientX));
    };

    const handleMouseUp = (e: MouseEvent) => {
      const percent = getPercentFromEvent(e.clientX);
      const targetMs = (percent / 100) * durationMs;
      void seek(targetMs);
      setIsDragging(false);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isDragging, durationMs, seek, getPercentFromEvent]);

  const displayPercent = isDragging ? dragPercent : progressPercent;
  const displayMs = isDragging
    ? (dragPercent / 100) * durationMs
    : displayPositionMs;

  return (
    <div
      className={`flex ${PLAYBACK_BAR_SEEK_MIN_WIDTH_CLASS} flex-1 items-center font-[tabular-nums] text-[11px] text-[var(--color-text-dim)] ${
        density === "relaxed" ? "gap-3" : "gap-2"
      }`}
    >
      <span
        className={`${PLAYBACK_BAR_TIME_LABEL_WIDTH_CLASS} shrink-0 whitespace-nowrap text-center`}
      >
        {formatDuration(displayMs)}
      </span>
      <div
        ref={barRef}
        className={`group relative h-1.5 ${PLAYBACK_BAR_SEEK_RAIL_MIN_WIDTH_CLASS} flex-1 rounded-full bg-[var(--color-border)]`}
        onMouseDown={handleMouseDown}
        role="slider"
        aria-label={t("player.seek")}
        aria-valuemin={0}
        aria-valuemax={durationMs}
        aria-valuenow={Math.round(displayMs)}
        aria-valuetext={formatDuration(displayMs)}
      >
        <div
          className={`relative h-full rounded-full transition-colors ${
            isDragging
              ? "bg-[var(--color-control-primary)]"
              : "bg-[var(--color-text-dim)] group-hover:bg-[var(--color-control-primary)]"
          }`}
          style={{ width: `${displayPercent}%` }}
        >
          {/* Playhead dot — visible on hover and during drag */}
          <div
            className={`absolute -right-1.5 top-1/2 h-3 w-3 -translate-y-1/2 rounded-full bg-[var(--color-control-primary)] shadow-sm transition-opacity ${
              isDragging ? "opacity-100" : "opacity-0 group-hover:opacity-100"
            }`}
          />
        </div>
      </div>
      <span
        className={`${PLAYBACK_BAR_TIME_LABEL_WIDTH_CLASS} shrink-0 whitespace-nowrap text-center`}
      >
        {formatDuration(durationMs)}
      </span>
    </div>
  );
}
