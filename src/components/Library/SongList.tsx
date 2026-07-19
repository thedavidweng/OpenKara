import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { SongListItem } from "./SongListItem";
import { EmptyLibrary } from "./EmptyLibrary";
import { AlphabetRail } from "./AlphabetRail";
import { useLibraryStore } from "@/stores/library-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import { useSettingsStore } from "@/stores/settings-store";
import { sortSongs } from "@/lib/song-sort";
import { buildAlphabetIndex } from "@/lib/alphabet-index";
import type { Song } from "@/types/ipc";

const SONG_ROW_ESTIMATE_PX = 68;
const SONG_ROW_GAP_PX = 4;

// matchMedia legacy fallback for older WebViews that lack addEventListener.
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
    // Legacy fallback for older WebKit.
    mql.addListener(handler);
    return () => mql.removeListener(handler);
  }, []);

  return matches;
}

export function SongList() {
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
      // Preserve the backend-provided sort_order exactly. Build a Map so the
      // ordered entries map to library songs without inheriting library order.
      const libraryByHash = new Map(librarySongs.map((s) => [s.hash, s]));
      return playlistSongEntries
        .map((entry) => libraryByHash.get(entry.song_hash))
        .filter((song): song is Song => song != null);
    },
    [getPlaylistSongs],
  );

  // Cancel stale async playlist loads when the active playlist changes
  // rapidly. Without this guard, a slow response from playlist A can overwrite
  // the song list after the user has already switched to playlist B.
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

  // Derive the final display order in one memoized step:
  //   1. Playlists use backend sort_order directly (no library sort).
  //   2. Otherwise start from library/search songs, apply the separated
  //      filter when selected, then sort by the current library sort mode.
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

  // TanStack Virtual returns non-memoizable functions by design
  const virtualizer = useVirtualizer({
    count: displaySongs.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => SONG_ROW_ESTIMATE_PX,
    gap: SONG_ROW_GAP_PX,
    overscan: 8,
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

  // When the sort mode changes (and no playlist is active), clear only the
  // range-selection anchor and scroll the virtual list back to the first row.
  // Metadata/import changes rederive order through memoization without another
  // fetch, so this effect is keyed only on the sort mode. The virtualizer and
  // anchor clearer are stable across renders (zustand action identity + TanStack
  // virtualizer identity tied to the scroll element), so they are intentionally
  // omitted from the dependency array.
  const previousSortModeRef = useRef(librarySortMode);
  useEffect(() => {
    if (previousSortModeRef.current === librarySortMode) return;
    previousSortModeRef.current = librarySortMode;
    if (activePlaylistId) return;
    clearRangeSelectionAnchor();
    if (displaySongs.length > 0) {
      virtualizer.scrollToIndex(0, { align: "start" });
    }
    // When displaySongs is empty the component renders <EmptyLibrary />, so
    // there is no scroll container to reset — no else branch is needed.
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
                className="absolute left-0 top-0 w-full"
                style={{ transform: `translateY(${virtualRow.start}px)` }}
              >
                <SongListItem song={song} orderedHashes={orderedHashes} />
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
