import { Loader2 } from "lucide-react";
import { useLibraryStore } from "@/stores/library-store";
import { usePlayerStore } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { formatDuration } from "@/lib/format";
import * as api from "@/lib/tauri";
import type { Song } from "@/types/ipc";

interface SongListItemProps {
  song: Song;
}

export function SongListItem({ song }: SongListItemProps) {
  const selectedSongId = useLibraryStore((s) => s.selectedSongId);
  const setSelectedSongId = useLibraryStore((s) => s.setSelectedSongId);
  const separationStatus = useLibraryStore(
    (s) => s.separationStatuses[song.hash],
  );
  const snapshot = usePlayerStore((s) => s.snapshot);
  const playSong = usePlayerStore((s) => s.playSong);
  const fetchLyrics = useLyricsStore((s) => s.fetchLyrics);

  const isSelected = selectedSongId === song.hash;
  const isCurrentPlaying =
    snapshot?.song_id === song.hash && snapshot?.is_playing;
  const sepState = separationStatus?.state ?? "idle";

  const handleDoubleClick = () => {
    playSong(song.hash);
    fetchLyrics(song.hash);
  };

  const handleSeparate = (e: React.MouseEvent) => {
    e.stopPropagation();
    api.separate(song.hash);
  };

  return (
    <div
      onClick={() => setSelectedSongId(song.hash)}
      onDoubleClick={handleDoubleClick}
      className={`group relative flex cursor-default select-none flex-col justify-center rounded-md px-3 py-1.5 ${
        isSelected
          ? "bg-[var(--color-accent)] text-white"
          : "text-[var(--color-text)] hover:bg-[var(--color-hover)]"
      }`}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 overflow-hidden">
          {isCurrentPlaying ? (
            <div className="flex w-3 shrink-0 justify-center">
              <span className="relative flex h-2 w-2">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-white opacity-75" />
                <span className="relative inline-flex h-2 w-2 rounded-full bg-white" />
              </span>
            </div>
          ) : (
            <div className="w-3 shrink-0" />
          )}
          <span className="truncate font-medium">
            {song.title || song.file_path.split("/").pop()}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {sepState === "idle" && (
            <button
              onClick={handleSeparate}
              className={`rounded px-1.5 py-0.5 text-[10px] border ${
                isSelected
                  ? "border-white/30 hover:bg-white/20"
                  : "border-[var(--color-border-light)] bg-[var(--color-hover)] text-[var(--color-text-dim)] hover:bg-[var(--color-active)]"
              }`}
            >
              Separate
            </button>
          )}
          {sepState === "running" && (
            <div
              className={`flex items-center gap-1 text-[11px] ${isSelected ? "text-white" : "text-[var(--color-text-dim)]"}`}
            >
              <Loader2 size={10} className="animate-spin" />
              <span>{separationStatus?.percent ?? 0}%</span>
            </div>
          )}
          {sepState === "completed" && !isSelected && (
            <span className="text-[11px] text-[var(--color-text-dim)]">
              {formatDuration(song.duration_ms)}
            </span>
          )}
          {sepState === "failed" && (
            <button
              onClick={handleSeparate}
              className="text-[10px] text-red-400"
            >
              Retry
            </button>
          )}
          {sepState !== "idle" &&
            sepState !== "running" &&
            sepState !== "completed" &&
            sepState !== "failed" && (
              <span className="text-[11px] text-[var(--color-text-dim)]">
                {formatDuration(song.duration_ms)}
              </span>
            )}
        </div>
      </div>

      <div className="flex pl-5">
        <span
          className={`truncate text-[11px] ${isSelected ? "text-white/80" : "text-[var(--color-text-dim)]"}`}
        >
          {song.artist || "Unknown Artist"}
        </span>
      </div>
    </div>
  );
}
