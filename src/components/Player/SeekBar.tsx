import { useRef, useState, useCallback, useEffect } from "react";
import { usePlayerStore } from "@/stores/player-store";
import { formatDuration } from "@/lib/format";

export function SeekBar() {
  const snapshot = usePlayerStore((s) => s.snapshot);
  const positionMs = usePlayerStore((s) => s.positionMs);
  const seek = usePlayerStore((s) => s.seek);

  const durationMs = snapshot?.duration_ms ?? 0;
  const progressPercent = durationMs > 0 ? (positionMs / durationMs) * 100 : 0;

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
      seek(targetMs);
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
  const displayMs = isDragging ? (dragPercent / 100) * durationMs : positionMs;

  return (
    <div className="flex flex-1 items-center gap-3 font-[tabular-nums] text-[11px] text-[var(--color-text-dim)]">
      <span>{formatDuration(displayMs)}</span>
      <div
        ref={barRef}
        className="flex-1 h-1.5 cursor-pointer rounded-full bg-[var(--color-border)]"
        onMouseDown={handleMouseDown}
      >
        <div
          className="h-full rounded-full bg-[var(--color-text-dim)] transition-colors hover:bg-white"
          style={{ width: `${displayPercent}%` }}
        />
      </div>
      <span>{formatDuration(durationMs)}</span>
    </div>
  );
}
