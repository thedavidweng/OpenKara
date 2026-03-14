import { useRef, useEffect } from "react";
import { LyricLine } from "./LyricLine";
import { LyricsOffsetControl } from "./LyricsOffsetControl";
import { LyricsEmptyState } from "./LyricsEmptyState";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";

export function LyricsPanel() {
  const lines = useLyricsStore((s) => s.lines);
  const activeLineIndex = useLyricsStore((s) => s.activeLineIndex);
  const isLoading = useLyricsStore((s) => s.isLoading);
  const songId = usePlayerStore((s) => s.snapshot?.song_id);
  const containerRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to active line
  useEffect(() => {
    if (activeLineIndex < 0 || !containerRef.current) return;
    const lineEl = containerRef.current.children[activeLineIndex] as
      | HTMLElement
      | undefined;
    lineEl?.scrollIntoView({ behavior: "smooth", block: "center" });
  }, [activeLineIndex]);

  if (!songId) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <p className="text-[14px] text-[var(--color-text-dimmer)]">
          Select a song to start
        </p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <p className="text-[14px] text-[var(--color-text-dim)]">
          Loading lyrics...
        </p>
      </div>
    );
  }

  if (lines.length === 0) {
    return <LyricsEmptyState />;
  }

  return (
    <div className="relative flex flex-1 flex-col items-center overflow-hidden">
      <div
        ref={containerRef}
        className="custom-scrollbar flex w-full max-w-2xl flex-1 flex-col items-center gap-7 overflow-y-auto px-12 py-8"
      >
        {lines.map((line, idx) => (
          <LyricLine
            key={idx}
            line={line}
            state={
              idx === activeLineIndex
                ? "active"
                : idx < activeLineIndex
                  ? "past"
                  : "future"
            }
          />
        ))}
      </div>
      <LyricsOffsetControl />
    </div>
  );
}
