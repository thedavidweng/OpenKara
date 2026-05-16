import { useEffect, useState, useCallback } from "react";
import { SongListItem } from "./SongListItem";
import { EmptyLibrary } from "./EmptyLibrary";
import { useLibraryStore } from "@/stores/library-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import type { Song } from "@/types/ipc";

export function SongList() {
  const songs = useLibraryStore((s) => s.songs);
  const filter = useLibraryStore((s) => s.filter);
  const separationStatuses = useLibraryStore((s) => s.separationStatuses);
  const activePlaylistId = usePlaylistStore((s) => s.activePlaylistId);
  const getPlaylistSongs = usePlaylistStore((s) => s.getPlaylistSongs);
  const playlistSongSets = usePlaylistStore((s) => s.playlistSongSets);
  const [playlistSongs, setPlaylistSongs] = useState<Song[]>([]);

  const loadPlaylistSongsFromLibrary = useCallback(
    async (playlistId: string, librarySongs: Song[]): Promise<Song[]> => {
      const playlistSongEntries = await getPlaylistSongs(playlistId);
      const hashSet = new Set(playlistSongEntries.map((p) => p.song_hash));
      return librarySongs.filter((s) => hashSet.has(s.hash));
    },
    [getPlaylistSongs],
  );

  useEffect(() => {
    if (activePlaylistId) {
      void loadPlaylistSongsFromLibrary(activePlaylistId, songs).then(
        setPlaylistSongs,
      );
    }
  }, [activePlaylistId, songs, loadPlaylistSongsFromLibrary, playlistSongSets]);

  const displaySongs = activePlaylistId
    ? playlistSongs
    : filter === "separated"
      ? songs.filter((s) => separationStatuses[s.hash]?.state === "completed")
      : songs;

  const orderedHashes = displaySongs.map((s) => s.hash);

  if (displaySongs.length === 0) {
    return <EmptyLibrary />;
  }

  return (
    <div
      className="custom-scrollbar flex-1 space-y-1 overflow-y-auto"
      data-song-list-visual-variant="unified"
    >
      {displaySongs.map((song) => (
        <SongListItem
          key={song.hash}
          song={song}
          orderedHashes={orderedHashes}
        />
      ))}
    </div>
  );
}
