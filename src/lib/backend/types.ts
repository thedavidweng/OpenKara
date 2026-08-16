import type {
  AirPlayAudienceStatePayload,
  AirPlayRoutePickerBounds,
  AppSettings,
  AudioPeakSnapshot,
  CacheUsage,
  CoverArtBytes,
  CoverArtSize,
  DebugInfo,
  DeleteSongsResult,
  DeleteStemsResult,
  DowngradeResult,
  ExecutionProvider,
  ExpandedImportPaths,
  ExtractEmbeddedCoverArtResult,
  ImportCandidateDetails,
  ImportLyricsResult,
  ImportSongsOptions,
  ImportSongsResult,
  IntegrityCleanupResult,
  IntegrityReport,
  LibraryRegistrySnapshot,
  LibrarySortMode,
  LyricsOnlineFetchIntent,
  LyricsPayload,
  ModelBootstrapStatusSnapshot,
  ModelStatusSnapshot,
  ModelUpdateReport,
  PlaybackStateSnapshot,
  RegisteredLibrary,
  RemoteAuthPayload,
  RemoteAuthStart,
  RemoteAuthStatus,
  RemoteDiagnostics,
  RemoteLibraryCandidate,
  RemoteLibraryProvider,
  RuntimeBootstrapStatusSnapshot,
  RuntimeUpdateReport,
  SeparationStatusSnapshot,
  Song,
  SongProperties,
  StemName,
  ThemePreference,
  UpdatePolicy,
  UploadStatusSnapshot,
  WaveformData,
  WindowShellStateSnapshot,
} from "@/types/ipc";

export interface NativeAppMenuLabels {
  file: string;
  edit: string;
  view: string;
  window: string;
  help: string;
  import: string;
  settings: string;
  switchLibrary: string;
  toggleSidebar: string;
  copyDebugInfo: string;
}

export interface Playlist {
  id: string;
  name: string;
  song_count: number;
  created_at: number;
  updated_at: number;
}

export interface PlaylistSong {
  song_hash: string;
  added_at: number;
  sort_order: number;
  singer: string | null;
}

export interface RotationState {
  singer_names: string[];
  current_index: number;
  mode: string;
  active: boolean;
}

export type RemoteConflictResolution = "keep_local" | "use_remote";

export type CdgAvailability = "none" | "loading" | "ready" | "error";

export type CdgErrorCode =
  | "missing"
  | "empty"
  | "invalid"
  | "read_failed"
  | "zip_failed";

export interface CdgStatus {
  availability: CdgAvailability;
  songId: string | null;
  transportGeneration: number | null;
  packetCount: number | null;
  errorCode: CdgErrorCode | null;
}

export type PlainTextPageDirection = "prev" | "next";

export interface PlaybackBackend {
  play(songId: string): Promise<PlaybackStateSnapshot>;
  resume(): Promise<PlaybackStateSnapshot>;
  pause(): Promise<PlaybackStateSnapshot>;
  seek(ms: number): Promise<PlaybackStateSnapshot>;
  setVolume(level: number): Promise<PlaybackStateSnapshot>;
  setStemVolume(stem: StemName, level: number): Promise<PlaybackStateSnapshot>;
  loadStems(): Promise<PlaybackStateSnapshot>;
  getPlaybackState(): Promise<PlaybackStateSnapshot>;
  getAudioPeaks(): Promise<AudioPeakSnapshot>;
  getWaveform(hash: string, buckets?: number): Promise<WaveformData>;
  setPreloadCandidate(songId: string | null): Promise<void>;
  syncAirPlayRoutePicker(
    bounds: AirPlayRoutePickerBounds | null,
  ): Promise<void>;
  syncAirPlayAudienceState(payload: AirPlayAudienceStatePayload): Promise<void>;
  stepAirPlayPlainTextPage(direction: PlainTextPageDirection): Promise<void>;
}

export interface LibraryBackend {
  importSongs(
    paths: string[],
    options?: ImportSongsOptions,
  ): Promise<ImportSongsResult>;
  getImportCandidateDetails(paths: string[]): Promise<ImportCandidateDetails[]>;
  expandImportPaths(paths: string[]): Promise<ExpandedImportPaths>;
  pickImportPaths(defaultPath?: string): Promise<string[]>;
  getLibrary(): Promise<Song[]>;
  searchLibrary(query: string): Promise<Song[]>;
  updateSongMetadata(
    hash: string,
    title: string | null,
    artist: string | null,
  ): Promise<Song>;
  setSongsInstrumental(
    songIds: string[],
    instrumental: boolean,
  ): Promise<Song[]>;
  setSongsLanguage(songIds: string[], language: string | null): Promise<Song[]>;
  deleteSongs(songIds: string[]): Promise<DeleteSongsResult>;
  getSongProperties(songId: string): Promise<SongProperties>;
  getCoverArt(hash: string, size?: CoverArtSize): Promise<CoverArtBytes>;
  getCoverArtThumbnail(hash: string): Promise<CoverArtBytes>;
  getCoverArtPreview(hash: string): Promise<CoverArtBytes>;
  checkLibraryIntegrity(): Promise<IntegrityReport>;
  removeMissingLibraryEntries(
    hashes: string[],
  ): Promise<IntegrityCleanupResult>;
}

export interface LibrarySetupBackend {
  getLibraryPath(): Promise<string | null>;
  getLibraryRegistry(): Promise<LibraryRegistrySnapshot>;
  getActiveLibrary(): Promise<RegisteredLibrary | null>;
  createLocalLibrary(path: string): Promise<void>;
  registerLocalLibrary(path: string): Promise<void>;
  switchLibrary(libraryId: string): Promise<LibraryRegistrySnapshot>;
  removeLibrary(libraryId: string): Promise<LibraryRegistrySnapshot>;
  renameLibrary(
    libraryId: string,
    displayName: string,
  ): Promise<LibraryRegistrySnapshot>;
  deleteLibrary(libraryId: string): Promise<LibraryRegistrySnapshot>;
}

export interface RemoteRepositoryBackend {
  beginRemoteAuth(
    provider: RemoteLibraryProvider,
    payload?: RemoteAuthPayload,
  ): Promise<RemoteAuthStart>;
  pollRemoteAuth(sessionId: string): Promise<RemoteAuthStatus>;
  cancelRemoteAuth(sessionId: string): Promise<void>;
  openExternalUrl(url: string): Promise<void>;
  listRemoteLibraryRoots(sessionId: string): Promise<RemoteLibraryCandidate[]>;
  createRemoteLibrary(
    sessionId: string,
    displayName: string,
  ): Promise<RemoteLibraryCandidate>;
  resolveRemoteLibraryCandidate(
    sessionId: string,
    displayName: string,
  ): Promise<RemoteLibraryCandidate>;
  registerRemoteLibrary(
    sessionId: string,
    remoteRootLocator: string,
    displayName?: string | null,
  ): Promise<LibraryRegistrySnapshot>;
  reauthorizeRemoteRepository(
    libraryId: string,
    sessionId: string,
    remoteRootLocator: string,
    displayName: string,
  ): Promise<LibraryRegistrySnapshot>;
  relocateRemoteRepository(
    libraryId: string,
    sessionId: string,
    remoteRootLocator: string,
    displayName: string,
  ): Promise<LibraryRegistrySnapshot>;
  mirrorLocalLibraryToRemote(
    localLibraryId: string,
    remoteLibraryId: string,
  ): Promise<void>;
  refreshRemoteRepository(): Promise<void>;
  publishSongToRemote(songId: string): Promise<unknown>;
  publishSongsToRemote(songIds: string[]): Promise<unknown>;
  getAllUploadStatuses(): Promise<UploadStatusSnapshot[]>;
  getRemoteCacheUsage(): Promise<CacheUsage>;
  clearRemoteCache(): Promise<number>;
  resolveRemoteConflict(resolution: RemoteConflictResolution): Promise<void>;
  getRemoteDiagnostics(): Promise<RemoteDiagnostics>;
}

export interface SettingsBackend {
  getSettings(): Promise<AppSettings>;
  getDebugInfo(): Promise<DebugInfo>;
  getWindowShellState(): Promise<WindowShellStateSnapshot>;
  setNativeSidebarVisibility(visible: boolean): Promise<void>;
  windowReady(): Promise<void>;
  setNativeAppMenuLabels(labels: NativeAppMenuLabels): Promise<void>;
  restartApp(): Promise<void>;
  setLanguage(language: string): Promise<AppSettings>;
  setStemMode(mode: string): Promise<AppSettings>;
  setModelVariant(variant: string): Promise<AppSettings>;
  setHideBatchSeparate(value: boolean): Promise<AppSettings>;
  setCoverArtBackdrop(value: boolean): Promise<AppSettings>;
  setLyricsBlurInactive(value: boolean): Promise<AppSettings>;
  setHideUpgradeAll(value: boolean): Promise<AppSettings>;
  setExecutionProvider(provider: ExecutionProvider): Promise<AppSettings>;
  setLyricsFontStep(step: number): Promise<AppSettings>;
  setEqEnabled(enabled: boolean): Promise<AppSettings>;
  setEqGains(
    gainsDb: [number, number, number, number, number],
  ): Promise<AppSettings>;
  setCrossfadeEnabled(enabled: boolean): Promise<AppSettings>;
  setCrossfadeDurationMs(durationMs: number): Promise<AppSettings>;
  setLibrarySortMode(mode: LibrarySortMode): Promise<AppSettings>;
  setThemePreference(preference: ThemePreference): Promise<AppSettings>;
  setUpdatePolicy(policy: UpdatePolicy): Promise<AppSettings>;
  getModelBootstrapStatus(): Promise<ModelBootstrapStatusSnapshot>;
  getModelStatus(variant: string): Promise<ModelStatusSnapshot>;
  downloadModel(variant: string): Promise<ModelBootstrapStatusSnapshot>;
  deleteModel(variant: string): Promise<void>;
  checkModelUpdates(): Promise<ModelUpdateReport>;
  getRuntimeBootstrapStatus(): Promise<RuntimeBootstrapStatusSnapshot>;
  downloadRuntime(): Promise<RuntimeBootstrapStatusSnapshot>;
  deleteRuntime(): Promise<void>;
  checkRuntimeUpdates(): Promise<RuntimeUpdateReport>;
}

export interface LyricsBackend {
  importLyricsFiles(paths: string[]): Promise<ImportLyricsResult>;
  fetchLyrics(songId: string): Promise<LyricsPayload>;
  setLyricsOffset(songId: string, ms: number): Promise<void>;
  saveManualLyrics(songId: string, text: string): Promise<LyricsPayload>;
  extractEmbeddedLyrics(songId: string): Promise<LyricsPayload>;
  fetchLyricsOnline(
    songId: string,
    intent: LyricsOnlineFetchIntent,
  ): Promise<LyricsPayload>;
}

export interface SeparationBackend {
  separate(songId: string): Promise<SeparationStatusSnapshot>;
  cancelSeparation(songId: string): Promise<void>;
  getSeparationStatus(songId: string): Promise<SeparationStatusSnapshot>;
  getAllSeparationStatuses(): Promise<SeparationStatusSnapshot[]>;
  upgradeToFourStem(songId: string): Promise<SeparationStatusSnapshot>;
  reSeparate(
    songId: string,
    stemMode: string,
  ): Promise<SeparationStatusSnapshot>;
}

export interface MaintenanceBackend {
  deleteAllStems(): Promise<DeleteStemsResult>;
  estimateStemsSize(): Promise<number>;
  deleteAllCachedLyrics(): Promise<number>;
  extractEmbeddedCoverArt(
    songIds: string[],
  ): Promise<ExtractEmbeddedCoverArtResult>;
  batchSeparate(songIds: string[]): Promise<void>;
  cancelBatchSeparation(): Promise<void>;
  downgradeToTwoStem(songId: string): Promise<SeparationStatusSnapshot>;
  downgradeAllToTwoStem(): Promise<DowngradeResult>;
  estimateDowngradeSavings(): Promise<number>;
}

export interface PlaylistBackend {
  listPlaylists(): Promise<Playlist[]>;
  createPlaylist(name: string): Promise<Playlist>;
  renamePlaylist(playlistId: string, name: string): Promise<void>;
  deletePlaylist(playlistId: string): Promise<void>;
  addSongsToPlaylist(playlistId: string, songHashes: string[]): Promise<void>;
  removeSongsFromPlaylist(
    playlistId: string,
    songHashes: string[],
  ): Promise<void>;
  getPlaylistSongs(playlistId: string): Promise<PlaylistSong[]>;
  setRotationState(rotation: RotationState): Promise<void>;
  getRotationState(): Promise<RotationState>;
  advanceRotation(): Promise<RotationState>;
  setQueueEntrySinger(
    playlistId: string,
    songHash: string,
    singer: string | null,
  ): Promise<void>;
}

export interface CdgBackend {
  getCdgFrame(
    songId: string,
    transportGeneration: number,
    positionMs: number,
    lastFrameVersion: number,
  ): Promise<ArrayBuffer>;
  getCdgStatus(songId: string, transportGeneration: number): Promise<CdgStatus>;
}

/**
 * Everything the frontend can ask the desktop backend to do, grouped by
 * domain. `TauriBackend` speaks to Rust over IPC; `MockBackend` replays the
 * shared in-memory fake that also drives E2E and the website preview.
 */
export interface Backend {
  playback: PlaybackBackend;
  library: LibraryBackend;
  librarySetup: LibrarySetupBackend;
  remoteRepository: RemoteRepositoryBackend;
  settings: SettingsBackend;
  lyrics: LyricsBackend;
  separation: SeparationBackend;
  maintenance: MaintenanceBackend;
  playlist: PlaylistBackend;
  cdg: CdgBackend;
}
