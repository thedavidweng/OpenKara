import { Folder, CheckCircle2, UploadCloud } from "lucide-react";
import { SearchBox } from "@/components/Library/SearchBox";
import { SongList } from "@/components/Library/SongList";
import { ImportButton } from "@/components/Library/ImportButton";
import { useLibraryStore } from "@/stores/library-store";

export function Sidebar() {
  const songs = useLibraryStore((s) => s.songs);
  const filter = useLibraryStore((s) => s.filter);
  const setFilter = useLibraryStore((s) => s.setFilter);
  const separationStatuses = useLibraryStore((s) => s.separationStatuses);

  const separatedCount = songs.filter(
    (s) => separationStatuses[s.hash]?.state === "completed",
  ).length;

  return (
    <div className="flex w-[260px] shrink-0 flex-col border-r border-[var(--color-border)] bg-[var(--color-sidebar)]">
      {/* macOS traffic light spacing */}
      <div
        className="flex h-12 shrink-0 items-center gap-2 px-4"
        data-tauri-drag-region
      >
        <div className="h-3 w-3 rounded-full bg-[#FF5F56] border border-[#E0443E]/50" />
        <div className="h-3 w-3 rounded-full bg-[#FFBD2E] border border-[#DEA123]/50" />
        <div className="h-3 w-3 rounded-full bg-[#27C93F] border border-[#1AAB29]/50" />
      </div>

      <div className="shrink-0 px-3 pb-3">
        <SearchBox />
      </div>

      {/* Filter tabs */}
      <div className="shrink-0 space-y-0.5 px-2">
        <div className="px-2 pb-1 text-[11px] font-semibold tracking-wide text-[var(--color-text-dim)]">
          LIBRARY
        </div>
        <button
          onClick={() => setFilter("all")}
          className={`flex w-full items-center justify-between rounded-md px-2 py-1.5 ${
            filter === "all"
              ? "bg-[var(--color-hover)] text-white"
              : "text-[var(--color-text)] hover:bg-[var(--color-hover)]"
          }`}
        >
          <span className="flex items-center gap-2">
            <Folder
              size={14}
              className="text-[var(--color-accent)]"
              fill="currentColor"
              fillOpacity={0.2}
            />
            <span>All Tracks</span>
          </span>
          <span className="text-[11px] text-[var(--color-text-dim)]">
            {songs.length}
          </span>
        </button>
        <button
          onClick={() => setFilter("separated")}
          className={`flex w-full items-center justify-between rounded-md px-2 py-1.5 ${
            filter === "separated"
              ? "bg-[var(--color-hover)] text-white"
              : "text-[var(--color-text)] hover:bg-[var(--color-hover)]"
          }`}
        >
          <span className="flex items-center gap-2">
            <CheckCircle2 size={14} className="text-[var(--color-text-dim)]" />
            <span>Separated</span>
          </span>
          <span className="text-[11px] text-[var(--color-text-dim)]">
            {separatedCount}
          </span>
        </button>
      </div>

      {/* Song list */}
      <div className="mt-4 flex flex-1 flex-col overflow-hidden px-2">
        <div className="flex items-center justify-between px-2 pb-1 text-[11px] font-semibold tracking-wide text-[var(--color-text-dim)]">
          <span>LOCAL MUSIC</span>
          <ImportButton>
            <UploadCloud size={12} className="hover:text-white" />
          </ImportButton>
        </div>
        <SongList />
      </div>
    </div>
  );
}
