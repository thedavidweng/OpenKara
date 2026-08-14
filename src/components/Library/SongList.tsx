import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { SongListItem } from "./SongListItem";
import { EmptyLibrary } from "./EmptyLibrary";
import { AlphabetRail } from "./AlphabetRail";
import { resolveSongListMeasureElement } from "./song-list-virtual";
import { useLibraryStore } from "@/stores/library-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import { useSettingsStore } from "@/stores/settings-store";
import { sortSongs } from "@/lib/song-sort";
import { buildAlphabetIndex } from "@/lib/alphabet-index";
import type { Song } from "@/types/ipc";

const SONG_ROW_ESTIMATE_PX = 68;
const SONG_ROW_GAP_PX = 4;

function useAtLeast600Px(): boolean {
  const [matches, setMatches] = useState(() => {
    if (typeof window === "undefined" || !window.matchMedia) return false;
    return window.matchMedia("(min-width: 600px)").matches;
  });

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;
    const mql = window.matchMedia("(min-width: 600px)");
    const handler = (e: MediaQueryListEvent) => setMatches(e.matches);
    if (mql.addEventListener) {
      mql.addEventListener("change", handler);
      return () => mql.removeEventListener("change", handler);
    }
    // Older WebViews lack addEventListener on MediaQueryList.
    mql.addListener(handler);
    return () => mql.removeListener(handler);
  }, []);

  return matches;
}

interface SongListProps {
  previewMode?: boolean;
}

export function SongList({ previewMode = false }: SongListProps = {}) {
  const songs = useLibraryStore((s) => s.songs);
  const filter = useLibraryStore((s) => s.filter);
  const separationStatuses = useLibraryStore((s) => s.separationStatuses);
  const clearRangeSelectionAnchor = useLibraryStore(
    (s) => s.clearRangeSelectionAnchor,
  );
  const activePlaylistId = usePlaylistStore((s) => s.activePlaylistId);
  const getPlaylistSongs = usePlaylistStore((s) => s.getPlaylistSongs);
  const playlistSongSets = usePlaylistStore((s) => s.playlistSongSets);
  const librarySortMode = useSettingsStore((s) => s.librarySortMode);
  const [playlistSongs, setPlaylistSongs] = useState<Song[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);

  const loadPlaylistSongsFromLibrary = useCallback(
    async (playlistId: string, librarySongs: Song[]): Promise<Song[]> => {
      const playlistSongEntries = await getPlaylistSongs(playlistId);
      const libraryByHash = new Map(librarySongs.map((s) => [s.hash, s]));
      return playlistSongEntries
        .map((entry) => libraryByHash.get(entry.song_hash))
        .filter((song): song is Song => song != null);
    },
    [getPlaylistSongs],
  );

  useEffect(() => {
    if (activePlaylistId) {
      let cancelled = false;
      void loadPlaylistSongsFromLibrary(activePlaylistId, songs).then(
        (result) => {
          if (!cancelled) {
            setPlaylistSongs(result);
          }
        },
      );
      return () => {
        cancelled = true;
      };
    }
  }, [activePlaylistId, songs, loadPlaylistSongsFromLibrary, playlistSongSets]);

  const separatedSongs = useMemo(
    () =>
      filter === "separated"
        ? songs.filter(
            (song) => separationStatuses[song.hash]?.state === "completed",
          )
        : null,
    [filter, songs, separationStatuses],
  );

  const displaySongs = useMemo(() => {
    if (activePlaylistId) return playlistSongs;
    const base = filter === "separated" ? (separatedSongs ?? songs) : songs;
    return sortSongs(base, librarySortMode);
  }, [
    activePlaylistId,
    playlistSongs,
    filter,
    separatedSongs,
    songs,
    librarySortMode,
  ]);

  const orderedHashes = displaySongs.map((s) => s.hash);

  const virtualizer = useVirtualizer({
    count: displaySongs.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => SONG_ROW_ESTIMATE_PX,
    gap: SONG_ROW_GAP_PX,
    overscan: 8,
    measureElement: resolveSongListMeasureElement(
      typeof window !== "undefined",
      typeof navigator !== "undefined" ? navigator.userAgent : "",
    ),
  });

  const isAtLeast600Px = useAtLeast600Px();

  const railSortMode =
    librarySortMode === "title_asc" || librarySortMode === "artist_asc"
      ? librarySortMode
      : null;

  const indexByBucket = useMemo(() => {
    if (!railSortMode) return null;
    return buildAlphabetIndex(displaySongs, railSortMode);
  }, [displaySongs, railSortMode]);

  const showRail =
    activePlaylistId == null &&
    displaySongs.length > 0 &&
    railSortMode !== null &&
    isAtLeast600Px;

  const previousSortModeRef = useRef(librarySortMode);
  useEffect(() => {
    if (previousSortModeRef.current === librarySortMode) return;
    previousSortModeRef.current = librarySortMode;
    if (activePlaylistId) return;
    clearRangeSelectionAnchor();
    if (displaySongs.length > 0) {
      virtualizer.scrollToIndex(0, { align: "start" });
    }
  }, [
    librarySortMode,
    activePlaylistId,
    displaySongs.length,
    clearRangeSelectionAnchor,
    virtualizer,
  ]);

  if (displaySongs.length === 0) {
    return <EmptyLibrary />;
  }

  return (
    <div className="relative min-h-0 flex-1">
      <div
        ref={scrollRef}
        className="h-full overflow-y-auto"
        data-testid="song-list"
        data-song-list-visual-variant="unified"
        data-preview-song-list="true"
        style={showRail ? { paddingRight: "24px" } : undefined}
      >
        <div
          className="relative w-full"
          style={{ height: `${virtualizer.getTotalSize()}px` }}
        >
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const song = displaySongs[virtualRow.index];
            return (
              <div
                key={song.hash}
                data-index={virtualRow.index}
                ref={virtualizer.measureElement}
                className="absolute left-0 top-0 w-full"
                style={{ transform: `translateY(${virtualRow.start}px)` }}
              >
                <SongListItem
                  song={song}
                  orderedHashes={orderedHashes}
                  previewMode={previewMode}
                />
              </div>
            );
          })}
        </div>
      </div>
      {showRail && indexByBucket && (
        <AlphabetRail
          indexByBucket={indexByBucket}
          onNavigate={(index) => {
            clearRangeSelectionAnchor();
            virtualizer.scrollToIndex(index, { align: "start" });
          }}
        />
      )}
    </div>
  );
}
