import type { Backend } from "@/lib/backend";
import type {
  LibrarySession,
  LibrarySessionViews,
} from "@/lib/library-session";
import type { AppSettingsSnapshot } from "@/stores/settings-store";
import type {
  AppSettings,
  ExecutionProvider,
  IntegrityReport,
  LibraryRegistrySnapshot,
  ModelUpdateCheckSnapshot,
  ModelVariant,
  RegisteredLibrary,
  RuntimeBootstrapFailurePhase,
  RuntimeBootstrapState,
  RuntimeBootstrapStatusSnapshot,
  RuntimeUpdateReport,
  SeparationStatusSnapshot,
  StemMode,
  ThemePreference,
  UpdatePolicy,
} from "@/types/ipc";

export interface ModelStatusView {
  downloaded: boolean;
  legacy_install_present: boolean;
  file_size_bytes: number | null;
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
  failure_phase: RuntimeBootstrapFailurePhase | null;
}

export interface RuntimeUpdateView {
  status: "checking" | "checked" | "failed";
  error: string | null;
  report: RuntimeUpdateReport | null;
}

export type SettingsDialog =
  | "delete_stems"
  | "downgrade_stems"
  | "delete_lyrics"
  | "delete_runtime"
  | "ft_warning"
  | "integrity_cleanup_confirm";

/**
 * The preference store's snapshot with the ADR-0021 reconciliation applied
 * once: `language` is the code the app actually renders in, never the "not
 * chosen yet" null.
 */
export interface SettingsPreferencesView extends Omit<
  AppSettingsSnapshot,
  "language"
> {
  language: string;
}

export interface SettingsLibraryView {
  registry: LibraryRegistrySnapshot | null;
  libraries: RegisteredLibrary[];
  activeLibraryId: string | null;
  activeLibraryPath: string | null;
  error: string | null;
}

export interface SettingsModelsView {
  statuses: Partial<Record<ModelVariant, ModelStatusView>>;
  downloading: ModelVariant | null;
  update: ModelUpdateView | null;
}

export interface SettingsRuntimeView {
  status: RuntimeStatusView | null;
  update: RuntimeUpdateView | null;
}

export interface SettingsIntegrityView {
  report: IntegrityReport | null;
  selection: ReadonlySet<string>;
  skippedCount: number | null;
  checking: boolean;
  cleaningUp: boolean;
}

export interface SettingsMaintenanceView {
  stemsSize: number | null;
  downgradeSavings: number | null;
  deletingStems: boolean;
  deletingLyrics: boolean;
  downgrading: boolean;
}

/** Everything the Settings surfaces render, in one authoritative object. */
export interface SettingsView {
  isInitializing: boolean;
  dialog: SettingsDialog | null;
  library: SettingsLibraryView;
  preferences: SettingsPreferencesView;
  models: SettingsModelsView;
  runtime: SettingsRuntimeView;
  integrity: SettingsIntegrityView;
  maintenance: SettingsMaintenanceView;
}

/** The preferences a Settings surface may write, in view-model spelling. */
export interface WritablePreferences {
  language: string;
  stemMode: StemMode;
  executionProvider: ExecutionProvider;
  hideBatchSeparate: boolean;
  coverArtBackdrop: boolean;
  hideUpgradeAll: boolean;
  eqEnabled: boolean;
  eqGainsDb: [number, number, number, number, number];
  crossfadeEnabled: boolean;
  crossfadeDurationMs: number;
  themePreference: ThemePreference;
  updatePolicy: UpdatePolicy;
}

export type SettingsPreferencePatch = Partial<WritablePreferences>;

export interface SettingsLibraryCommands {
  /** Creates a Local Working Copy under a directory the user picks. */
  create(dialogTitle: string): Promise<void>;
  /** Registers an existing Local Working Copy the user picks. */
  open(dialogTitle: string): Promise<void>;
  /** Makes `libraryId` the active library. */
  activate(libraryId: string): Promise<void>;
  /**
   * Refresh Repository for a Remote Repository, activating it first when it is
   * not already active. Ignores libraries that are not remote.
   */
  refresh(libraryId: string): Promise<void>;
  rename(libraryId: string, displayName: string): Promise<void>;
  /** Disconnect Repository after a confirmation prompt. */
  disconnect(libraryId: string): Promise<void>;
  /**
   * Delete Repository. Runs only when the prompt is accepted and
   * `confirmationName` matches the library's display name exactly.
   */
  delete(libraryId: string, confirmationName: string): Promise<void>;
  checkIntegrity(): Promise<void>;
  toggleIntegrityEntry(songHash: string): void;
  /** Removes the selected integrity entries, then re-reads the song list. */
  cleanUpIntegrity(): Promise<void>;
  dismissIntegrityReport(): void;
}

export interface SettingsPreferenceCommands {
  /**
   * Writes every preference in `patch`. Preferences the user expects to move
   * instantly are applied to the store before the backend confirms and
   * reconciled from the store afterwards; the store owns any rollback.
   */
  set(patch: SettingsPreferencePatch): Promise<void>;
  /**
   * Applies a separation model, downloading it first when it is missing.
   * Selecting the fine-tuned model from another variant raises its
   * confirmation dialog instead of applying immediately.
   */
  selectModelVariant(variant: ModelVariant): Promise<void>;
}

export interface SettingsMaintenanceCommands {
  /** Raises `dialog`, reading any size estimate it displays first. */
  openDialog(dialog: SettingsDialog): Promise<void>;
  /** Runs the action the open dialog names and leaves no dialog open. */
  confirmDialog(): Promise<void>;
  closeDialog(): void;
  restartApp(): Promise<void>;
  checkModelUpdates(): Promise<void>;
  downloadModel(variant: ModelVariant): Promise<void>;
  deleteModel(variant: ModelVariant): Promise<void>;
  checkRuntimeUpdates(): Promise<void>;
  /** Installs or replaces the ONNX Runtime. */
  installRuntime(): Promise<void>;
}

/**
 * Owns everything the Settings surfaces read and every mutation they can
 * start. Commands never reject: a failure lands either in `view.library.error`
 * (library work), in the matching `update` slice (update checks), or on the
 * error reporter. The view is replaced, never mutated, so consumers can
 * compare identities.
 */
export interface SettingsController {
  getView(): SettingsView;
  subscribe(listener: () => void): () => void;
  /**
   * Reads the registry and preferences, clears `isInitializing` as soon as
   * both settle, then resolves once model and runtime status have been read.
   */
  initialize(): Promise<void>;
  library: SettingsLibraryCommands;
  preferences: SettingsPreferenceCommands;
  maintenance: SettingsMaintenanceCommands;
}

export interface SettingsPreferencesStore {
  getSnapshot(): AppSettingsSnapshot;
  subscribe(listener: () => void): () => void;
  hydrate(settings: AppSettings): void;
  patch(patch: Partial<AppSettingsSnapshot>): void;
  setEqEnabled(enabled: boolean): Promise<void>;
  setEqGains(gainsDb: [number, number, number, number, number]): Promise<void>;
  setCrossfadeEnabled(enabled: boolean): Promise<void>;
  setCrossfadeDurationMs(durationMs: number): Promise<void>;
  setThemePreference(preference: ThemePreference): Promise<void>;
  setUpdatePolicy(policy: UpdatePolicy): Promise<void>;
}

export interface SettingsRuntimeStatusStore {
  getStatus(): RuntimeBootstrapStatusSnapshot | null;
  subscribe(listener: () => void): () => void;
  updateStatus(status: RuntimeBootstrapStatusSnapshot): void;
}

export interface SettingsModelBootstrapStore {
  reload(): void;
}

export interface SettingsLibraryStore {
  getSelectedSongIds(): ReadonlySet<string>;
  clearSelection(): void;
  clearAllSeparationStatuses(): void;
  updateSeparationStatus(status: SeparationStatusSnapshot): void;
  loadLibrary(): Promise<void>;
}

export interface SettingsQueueStore {
  removeSongIds(songIds: string[]): void;
}

export interface SettingsPlayerStore {
  loadState(): Promise<void>;
}

export interface SettingsLyricsStore {
  clear(): void;
}

export interface SettingsControllerStores {
  preferences: SettingsPreferencesStore;
  runtimeStatus: SettingsRuntimeStatusStore;
  modelBootstrap: SettingsModelBootstrapStore;
  library: SettingsLibraryStore;
  queue: SettingsQueueStore;
  player: SettingsPlayerStore;
  lyrics: SettingsLyricsStore;
}

export interface SettingsControllerDependencies {
  backend: Backend;
  createLibrarySession(views: LibrarySessionViews): LibrarySession;
  selectDirectory(dialogTitle: string): Promise<string | null>;
  stores?: SettingsControllerStores;
  notifyError?(error: unknown): void;
  changeLanguage?(language: string): void | Promise<unknown>;
}
