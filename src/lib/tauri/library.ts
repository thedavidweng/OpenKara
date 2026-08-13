import type { LibraryBackend } from "@/lib/backend/types";
import type {
  CoverArtBytes,
  DeleteSongsResult,
  ExpandedImportPaths,
  ImportCandidateDetails,
  ImportSongsResult,
  IntegrityCleanupResult,
  IntegrityReport,
  Song,
  SongProperties,
} from "@/types/ipc";
import type { InvokeCommand } from "./invoke";

export function createLibraryCommands(invoke: InvokeCommand): LibraryBackend {
  const getCoverArt: LibraryBackend["getCoverArt"] = (hash, size) =>
    invoke<CoverArtBytes>(
      "get_cover_art",
      size === undefined ? { hash } : { hash, size },
    );

  return {
    importSongs: (paths, options) =>
      invoke<ImportSongsResult>("import_songs", { paths, options }),

    getImportCandidateDetails: (paths) =>
      invoke<ImportCandidateDetails[]>("get_import_candidate_details", {
        paths,
      }),

    expandImportPaths: (paths) =>
      invoke<ExpandedImportPaths>("expand_import_paths", { paths }),

    pickImportPaths: (defaultPath) =>
      invoke<string[]>("pick_import_paths", {
        defaultPath: defaultPath ?? null,
      }),

    getLibrary: () => invoke<Song[]>("get_library"),

    searchLibrary: (query) => invoke<Song[]>("search_library", { query }),

    updateSongMetadata: (hash, title, artist) =>
      invoke<Song>("update_song_metadata", { hash, title, artist }),

    setSongsInstrumental: (songIds, instrumental) =>
      invoke<Song[]>("set_songs_instrumental", { songIds, instrumental }),

    setSongsLanguage: (songIds, language) =>
      invoke<Song[]>("set_songs_language", { songIds, language }),

    deleteSongs: (songIds) =>
      invoke<DeleteSongsResult>("delete_songs", { songIds }),

    getSongProperties: (songId) =>
      invoke<SongProperties>("get_song_properties", { songId }),

    getCoverArt,

    getCoverArtThumbnail: (hash) => getCoverArt(hash, "thumb"),

    getCoverArtPreview: (hash) => getCoverArt(hash, "preview"),

    checkLibraryIntegrity: () =>
      invoke<IntegrityReport>("check_library_integrity"),

    removeMissingLibraryEntries: (hashes) =>
      invoke<IntegrityCleanupResult>("remove_missing_library_entries", {
        hashes,
      }),
  };
}
