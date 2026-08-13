import type { Backend } from "@/lib/backend";
import type { LibraryRegistrySnapshot } from "@/types/ipc";

export interface LibrarySessionLibraryStore {
  clearSongs(): void;
  clearAllSeparationStatuses(): void;
  clearAllUploadStatuses(): void;
  clearSelection(): void;
  loadLibrary(): Promise<void>;
}

export interface LibrarySessionQueueStore {
  clearQueue(): void;
}

export interface LibrarySessionLyricsStore {
  clear(): void;
}

export interface LibrarySessionPlayerStore {
  loadState(): Promise<void>;
}

export interface LibrarySessionStores {
  library: LibrarySessionLibraryStore;
  queue: LibrarySessionQueueStore;
  lyrics: LibrarySessionLyricsStore;
  player: LibrarySessionPlayerStore;
}

/**
 * Screens whose data outlives a library change and therefore has to be re-read
 * once the Local Working Copy is consistent again. The session always refreshes
 * the registry before model statuses.
 */
export interface LibrarySessionViews {
  refreshRegistry(): Promise<void>;
  refreshModelStatuses(): Promise<void>;
}

export interface LibrarySessionDependencies {
  backend: Backend;
  stores?: LibrarySessionStores;
  views?: LibrarySessionViews;
}

/**
 * Owns every side effect of changing which library the app is working against.
 *
 * Each entry resolves only once the Local Working Copy and everything derived
 * from it — separation and upload status, selection, queue, lyrics, player
 * state, the song list, and the registry/model views — is consistent with the
 * library that is active on return. Callers never sequence those steps
 * themselves; a rejected entry means the sequence stopped where it failed.
 */
export interface LibrarySession {
  /** Creates a Local Working Copy inside `parentDirectory` and registers it. */
  createLocalLibrary(parentDirectory: string): Promise<void>;
  /** Registers an existing Local Working Copy directory. */
  openLocalLibrary(directory: string): Promise<void>;
  /**
   * Activates `libraryId`, running Refresh Repository first when the target is
   * a Remote Repository, then re-derives everything downstream of it.
   */
  switchLibrary(libraryId: string): Promise<void>;
  /**
   * Runs Refresh Repository against the already-active Remote Repository, then
   * re-derives everything downstream of it.
   */
  refreshRepository(): Promise<void>;
  /**
   * Adopts a registry that changed under the session — rename, Disconnect
   * Repository, or Delete Repository. Reloads the song list when a library is
   * still active and otherwise empties the Local Working Copy derived state.
   */
  adoptRegistry(registry: LibraryRegistrySnapshot): Promise<void>;
}
