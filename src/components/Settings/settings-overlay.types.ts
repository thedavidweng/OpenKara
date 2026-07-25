import { open } from "@tauri-apps/plugin-dialog";
import * as api from "@/lib/tauri";
import { useLibraryStore } from "@/stores/library-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import { useQueueStore } from "@/stores/queue-store";
import { useSettingsStore } from "@/stores/settings-store";
import type {
  ExecutionProvider,
  IntegrityReport,
  LibraryRegistrySnapshot,
  LibrarySortMode,
  ModelUpdateCheckSnapshot,
  ModelVariant,
  RegisteredLibrary,
  RuntimeBootstrapState,
  RuntimeUpdateReport,
  StemMode,
  ThemePreference,
  UpdatePolicy,
} from "@/types/ipc";

export type DangerDialog =
  | "delete_stems"
  | "downgrade_stems"
  | "delete_lyrics"
  | "delete_runtime"
  | "ft_warning"
  | "integrity_cleanup_confirm"
  | null;

export interface ModelStatusView {
  downloaded: boolean;
  legacy_install_present: boolean;
  file_size: number | null;
  installed_version: string | null;
  pinned_version: string;
}

export interface ModelUpdateView {
  status: "checking" | "checked" | "failed";
  error: string | null;
  generation: number | null;
  models: ModelUpdateCheckSnapshot[];
}

export interface RuntimeStatusView {
  state: RuntimeBootstrapState;
  version: string;
  runtime_path: string;
  active_artifact_id: string | null;
  target_triple: string;
  candidate_version: string | null;
  restart_required: boolean;
  error: string | null;
}

export interface RuntimeUpdateView {
  status: "checking" | "checked" | "failed";
  error: string | null;
  report: RuntimeUpdateReport | null;
}

export interface SettingsOverlayState {
  libraryPath: string | null;
  libraryError: string | null;
  libraryRegistry: LibraryRegistrySnapshot | null;
  libraries: RegisteredLibrary[];
  activeLibraryId: string | null;
  stemMode: StemMode;
  modelVariant: ModelVariant;
  modelStatuses: Partial<Record<ModelVariant, ModelStatusView>>;
  downloadingModel: ModelVariant | null;
  modelUpdate: ModelUpdateView | null;
  runtimeStatus: RuntimeStatusView | null;
  runtimeUpdate: RuntimeUpdateView | null;
  language: string;
  hideBatchSeparate: boolean;
  coverArtBackdrop: boolean;
  executionProvider: ExecutionProvider;
  availableExecutionProviders: ExecutionProvider[];
  eqEnabled: boolean;
  eqGainsDb: [number, number, number, number, number];
  crossfadeEnabled: boolean;
  crossfadeDurationMs: number;
  librarySortMode: LibrarySortMode;
  themePreference: ThemePreference;
  updatePolicy: UpdatePolicy;
  integrityReport: IntegrityReport | null;
  integritySelection: Set<string>;
  integritySkippedCount: number | null;
}

export interface SettingsOverlayMeta {
  isInitializing: boolean;
  dangerDialog: DangerDialog;
  stemsSize: number | null;
  downgradeSavings: number | null;
  deletingStemsInProgress: boolean;
  deletingLyricsInProgress: boolean;
  downgradingInProgress: boolean;
  integrityCheckInProgress: boolean;
  integrityCleanupInProgress: boolean;
}

export interface SettingsOverlaySnapshot {
  state: SettingsOverlayState;
  meta: SettingsOverlayMeta;
}

export interface SettingsOverlayActions {
  initialize: () => Promise<void>;
  createLibrary: (dialogTitle: string) => Promise<void>;
  openLibrary: (dialogTitle: string) => Promise<void>;
  switchLibrary: (libraryId: string) => Promise<void>;
  refreshRemoteRepository: (libraryId: string) => Promise<void>;
  renameLibrary: (libraryId: string, displayName: string) => Promise<void>;
  removeLibrary: (libraryId: string) => Promise<void>;
  deleteLibrary: (libraryId: string, confirmationName: string) => Promise<void>;
  setLanguage: (language: string) => Promise<void>;
  restartApp: () => Promise<void>;
  setStemMode: (mode: StemMode) => Promise<void>;
  setExecutionProvider: (provider: ExecutionProvider) => Promise<void>;
  selectModelVariant: (variant: ModelVariant) => Promise<void>;
  confirmFtModel: () => Promise<void>;
  deleteModel: (variant: ModelVariant) => Promise<void>;
  checkModelUpdates: () => Promise<void>;
  updateModel: (variant: ModelVariant) => Promise<void>;
  toggleHideBatchSeparate: (value: boolean) => Promise<void>;
  toggleCoverArtBackdrop: (value: boolean) => Promise<void>;
  setEqEnabled: (enabled: boolean) => Promise<void>;
  setEqGains: (
    gainsDb: [number, number, number, number, number],
  ) => Promise<void>;
  resetEqGains: () => Promise<void>;
  setCrossfadeEnabled: (enabled: boolean) => Promise<void>;
  setCrossfadeDurationMs: (durationMs: number) => Promise<void>;
  setThemePreference: (preference: ThemePreference) => Promise<void>;
  setUpdatePolicy: (policy: UpdatePolicy) => Promise<void>;
  checkRuntimeUpdates: () => Promise<void>;
  updateRuntime: () => Promise<void>;
  openDeleteStemsDialog: () => Promise<void>;
  confirmDeleteStems: () => Promise<void>;
  openDowngradeDialog: () => Promise<void>;
  confirmDowngrade: () => Promise<void>;
  openDeleteLyricsDialog: () => void;
  confirmDeleteLyrics: () => Promise<void>;
  closeDialog: () => void;
  refreshModelStatuses: () => Promise<void>;
  refreshRuntimeStatus: () => Promise<void>;
  downloadRuntime: () => Promise<void>;
  deleteRuntime: () => Promise<void>;
  openDeleteRuntimeDialog: () => void;
  checkLibraryIntegrity: () => Promise<void>;
  toggleIntegritySelection: (hash: string) => void;
  confirmIntegrityCleanup: () => Promise<void>;
  openIntegrityCleanupConfirmDialog: () => void;
  closeIntegrityReport: () => void;
}

export interface SettingsOverlayControllerDependencies {
  api: Pick<
    typeof api,
    | "createLibrary"
    | "createLocalLibrary"
    | "deleteAllCachedLyrics"
    | "deleteAllStems"
    | "checkModelUpdates"
    | "checkRuntimeUpdates"
    | "deleteModel"
    | "deleteRuntime"
    | "downloadModel"
    | "downloadRuntime"
    | "downgradeAllToTwoStem"
    | "estimateDowngradeSavings"
    | "estimateStemsSize"
    | "getAllSeparationStatuses"
    | "getLibraryPath"
    | "getLibraryRegistry"
    | "getRuntimeBootstrapStatus"
    | "getSettings"
    | "getModelStatus"
    | "openLibrary"
    | "registerLocalLibrary"
    | "renameLibrary"
    | "removeLibrary"
    | "deleteLibrary"
    | "mirrorLocalLibraryToRemote"
    | "reauthorizeRemoteLibrary"
    | "restartApp"
    | "switchLibrary"
    | "refreshRemoteRepository"
    | "setExecutionProvider"
    | "setHideBatchSeparate"
    | "setCoverArtBackdrop"
    | "setLanguage"
    | "setModelVariant"
    | "setStemMode"
    | "setEqEnabled"
    | "setEqGains"
    | "setCrossfadeEnabled"
    | "setCrossfadeDurationMs"
    | "setThemePreference"
    | "setUpdatePolicy"
    | "checkLibraryIntegrity"
    | "removeMissingLibraryEntries"
  >;
  notifyError: (error: unknown) => void;
  openDirectory: typeof open;
  changeLanguage: (language: string) => void | Promise<unknown>;
  libraryStore: Pick<
    ReturnType<typeof useLibraryStore.getState>,
    | "clearAllSeparationStatuses"
    | "clearAllUploadStatuses"
    | "clearSelection"
    | "loadLibrary"
    | "updateSeparationStatus"
  >;
  queueStore: Pick<
    ReturnType<typeof useQueueStore.getState>,
    "clearQueue" | "removeSongIds"
  >;
  playerStore: Pick<ReturnType<typeof usePlayerStore.getState>, "loadState">;
  lyricsStore: Pick<ReturnType<typeof useLyricsStore.getState>, "clear">;
  settingsStore: Pick<
    ReturnType<typeof useSettingsStore.getState>,
    | "getAppSettingsSnapshot"
    | "hydrateAppSettings"
    | "patchAppSettings"
    | "setEqEnabled"
    | "setEqGains"
    | "setCrossfadeEnabled"
    | "setCrossfadeDurationMs"
    | "setThemePreference"
    | "setUpdatePolicy"
  >;
}

export interface SettingsOverlayStateControls {
  getSnapshot: () => SettingsOverlaySnapshot;
  setSnapshot: (
    updater: (previous: SettingsOverlaySnapshot) => SettingsOverlaySnapshot,
  ) => void;
}

export type PatchState = (patch: Partial<SettingsOverlayState>) => void;
export type PatchMeta = (patch: Partial<SettingsOverlayMeta>) => void;

export interface SettingsActionContext {
  dependencies: SettingsOverlayControllerDependencies;
  controls: SettingsOverlayStateControls;
  patchState: PatchState;
  patchMeta: PatchMeta;
  refreshLibraryRegistry: () => Promise<void>;
  refreshModelStatuses: () => Promise<void>;
  applyModelVariant: (variant: ModelVariant) => Promise<void>;
  selectSingleDirectory: (dialogTitle: string) => Promise<string | null>;
  closeDialog: () => void;
}
