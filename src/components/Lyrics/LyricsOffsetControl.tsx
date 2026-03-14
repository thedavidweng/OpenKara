import { useLyricsStore } from "@/stores/lyrics-store";

export function LyricsOffsetControl() {
  const songId = useLyricsStore((s) => s.songId);
  const offsetMs = useLyricsStore((s) => s.offsetMs);
  const adjustOffset = useLyricsStore((s) => s.adjustOffset);

  if (!songId) return null;

  return (
    <div className="flex shrink-0 items-center gap-3 py-2 text-[11px] text-[var(--color-text-dim)]">
      <button
        onClick={() => adjustOffset(songId, -500)}
        className="rounded border border-[var(--color-border-light)] px-2 py-0.5 transition-colors hover:bg-[var(--color-hover)] hover:text-white"
      >
        -0.5s
      </button>
      <span className="font-[tabular-nums]">
        {offsetMs >= 0 ? "+" : ""}
        {(offsetMs / 1000).toFixed(1)}s
      </span>
      <button
        onClick={() => adjustOffset(songId, 500)}
        className="rounded border border-[var(--color-border-light)] px-2 py-0.5 transition-colors hover:bg-[var(--color-hover)] hover:text-white"
      >
        +0.5s
      </button>
    </div>
  );
}
