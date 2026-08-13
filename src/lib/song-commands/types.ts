import type { ContextMenuItem } from "@/components/Library/ContextMenu";
import type { SongLanguage } from "@/components/Library/song-list-item-menu";
import type { Backend } from "@/lib/backend";
import type { Song } from "@/types/ipc";

export interface SongCommandPlaylist {
  id: string;
  name: string;
}

export interface SongCommandLibraryStore {
  selectedSongIds(): string[];
  songs(): Song[];
  setSongsLanguage(
    songIds: string[],
    language: SongLanguage | null,
  ): Promise<unknown>;
  setSongsInstrumental(
    songIds: string[],
    instrumental: boolean,
  ): Promise<unknown>;
  extractEmbeddedCoverArt(songIds: string[]): Promise<unknown>;
}

export interface SongCommandPlaylistStore {
  playlists(): SongCommandPlaylist[];
  playlistSongSets(): Map<string, Set<string>>;
  activePlaylistId(): string | null;
  createPlaylist(name: string): Promise<{ id: string }>;
  addSongsToPlaylist(playlistId: string, songIds: string[]): Promise<void>;
  removeSongsFromPlaylist(playlistId: string, songIds: string[]): Promise<void>;
}

export interface SongCommandQueueStore {
  addToQueue(songId: string): void;
  playNext(songId: string): void;
}

export interface SongCommandPlayerStore {
  currentSongId(): string | null;
  playNow(songId: string): Promise<void>;
}

export interface SongCommandRotationStore {
  singerNames(): string[];
  getNextSinger(): string | null;
  assignSingerToQueueEntry(songId: string, singer: string | null): void;
  advanceRotation(): Promise<void>;
}

export interface SongCommandLyricsStore {
  clear(): void;
  fetchLyrics(songId: string): Promise<void>;
}

export interface SongCommandStores {
  library: SongCommandLibraryStore;
  playlist: SongCommandPlaylistStore;
  queue: SongCommandQueueStore;
  player: SongCommandPlayerStore;
  rotation: SongCommandRotationStore;
  lyrics: SongCommandLyricsStore;
}

/**
 * The dialogs a command can only ask for, because the row that owns them also
 * owns whether they are on screen.
 */
export interface SongCommandDialogs {
  editInfo(): void;
  properties(): void;
  confirmDelete(songIds: string[]): void;
  createPlaylist(): void;
}

export type SongCommandTranslate = (
  key: string,
  options?: Record<string, string | number>,
) => string;

/**
 * The row a command acts on. Which songs that means is resolved against the
 * live selection at dispatch time: the whole selection when the row is part of
 * it, otherwise the row alone.
 */
export interface SongCommandContext {
  song: Song;
  t: SongCommandTranslate;
}

export type SongCommand =
  | { id: "playNow" }
  | { id: "playNext" }
  | { id: "addToQueue" }
  | { id: "queueSelected" }
  | { id: "separateSelected" }
  | { id: "toggleInstrumental" }
  | { id: "setLanguage"; language: SongLanguage | null }
  | { id: "extractCoverArt" }
  | { id: "extractSelectedCoverArt" }
  | { id: "extractEmbeddedLyrics" }
  | { id: "fetchLyricsOnline" }
  | { id: "addToPlaylist"; playlistId: string }
  | { id: "removeFromPlaylist"; playlistId: string }
  | { id: "removeFromActivePlaylist" }
  | { id: "openCreatePlaylist" }
  | { id: "createPlaylistAndAdd"; name: string }
  | { id: "editInfo" }
  | { id: "openProperties" }
  | { id: "deleteSong" }
  | { id: "deleteSelected" };

export interface SongCommandDependencies {
  backend?: Backend;
  stores?: SongCommandStores;
  dialogs: SongCommandDialogs;
}

/**
 * Every action the song list offers on a song, as one vocabulary plus the menu
 * that names it.
 *
 * `buildMenu` reads the live selection and returns items that dispatch through
 * `execute`, so the menu and the callers that bypass it — a dialog confirming
 * a new playlist, a keyboard shortcut — cannot disagree about what an action
 * does. `execute` resolves once the work it owns is done or has been reported;
 * the fire-and-forget stores it drives keep their own progress state.
 */
export interface SongCommands {
  buildMenu(context: SongCommandContext): ContextMenuItem[];
  execute(command: SongCommand, context: SongCommandContext): Promise<void>;
}
