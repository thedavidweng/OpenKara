import type { LyricsBackend } from "@/lib/backend/types";
import type { ImportLyricsResult, LyricsPayload } from "@/types/ipc";
import type { InvokeCommand } from "./invoke";

export function createLyricsCommands(invoke: InvokeCommand): LyricsBackend {
  return {
    importLyricsFiles: (paths) =>
      invoke<ImportLyricsResult>("import_lyrics_files", { paths }),

    fetchLyrics: (songId) => invoke<LyricsPayload>("fetch_lyrics", { songId }),

    setLyricsOffset: (songId, ms) =>
      invoke<void>("set_lyrics_offset", { songId, ms }),

    saveManualLyrics: (songId, text) =>
      invoke<LyricsPayload>("save_manual_lyrics", { songId, text }),

    extractEmbeddedLyrics: (songId) =>
      invoke<LyricsPayload>("extract_embedded_lyrics", { songId }),

    fetchLyricsOnline: (songId, intent) =>
      invoke<LyricsPayload>("fetch_lyrics_online", { songId, intent }),
  };
}
