import { useCallback, useEffect, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { SongListItem } from "./SongListItem";
import { EmptyLibrary } from "./EmptyLibrary";
import { useLibraryStore } from "@/stores/library-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import type { Song } from "@/types/ipc";

const SONG_ROW_ESTIMATE_PX = 68;
const SONG_ROW_GAP_PX = 4;

export function SongList() {
  const songs = useLibraryStore((s) => s.songs);
  const filter = useLibraryStore((s) => s.filter);
  // Item 6: Only subscribe to separationStatuses when the "separated" filter
  // is active. This prevents the entire list from re-rendering on every
  // separation progress tick — only completed status changes matter for the
  // filter view.
  const separatedSongs = useLibraryStore((s) =>
    s.filter === "separated"
      ? s.songs.filter(
          (song) => s.separationStatuses[song.hash]?.state === "completed",
        )
      : null,
  );
  const activePlaylistId = usePlaylistStore((s) => s.activePlaylistId);
  const getPlaylistSongs = usePlaylistStore((s) => s.getPlaylistSongs);
  const playlistSongSets = usePlaylistStore((s) => s.playlistSongSets);
  const [playlistSongs, setPlaylistSongs] = useState<Song[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);

  const loadPlaylistSongsFromLibrary = useCallback(
    async (playlistId: string, librarySongs: Song[]): Promise<Song[]> => {
      const playlistSongEntries = await getPlaylistSongs(playlistId);
      const hashSet = new Set(playlistSongEntries.map((p) => p.song_hash));
      return librarySongs.filter((s) => hashSet.has(s.hash));
    },
    [getPlaylistSongs],
  );

  // F8: Cancel stale async playlist loads when the active playlist changes
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

  const displaySongs = activePlaylistId
    ? playlistSongs
    : filter === "separated"
      ? (separatedSongs ?? songs)
      : songs;

  const orderedHashes = displaySongs.map((s) => s.hash);

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual returns non-memoizable functions by design
  const virtualizer = useVirtualizer({
    count: displaySongs.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => SONG_ROW_ESTIMATE_PX,
    gap: SONG_ROW_GAP_PX,
    overscan: 8,
  });

  if (displaySongs.length === 0) {
    return <EmptyLibrary />;
  }

  return (
    <div
      ref={scrollRef}
      className="flex-1 overflow-y-auto"
      data-song-list-visual-variant="unified"
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
  );
}
