import type { ReactNode } from "react";
import { vi } from "vitest";
import { SettingsControllerContext } from "@/components/Settings/SettingsController.context";
import {
  createMockBackend,
  DEFAULT_MOCK_DATA,
  type BackendOverrides,
  type MockBackend,
} from "@/lib/backend/mock-backend";
import {
  createSettingsController,
  type SettingsController,
  type SettingsView,
} from "@/lib/settings-controller";
import type { MockData } from "@/mock/tauri-mock-impl";
import type { WebviewSyncChannel } from "@/runtime/webview-sync";
import { createRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import {
  createSettingsStore,
  type AppSettingsSnapshot,
  type SettingsSyncSnapshot,
} from "@/stores/settings-store";
import type {
  AppSettings,
  LibraryRegistrySnapshot,
  RegisteredLibrary,
  RuntimeBootstrapStatusSnapshot,
} from "@/types/ipc";
import {
  createRecordingLibrarySession,
  type RecordingLibrarySession,
} from "./library-session";

const detachedSyncChannel: WebviewSyncChannel<SettingsSyncSnapshot> = {
  publish: () => {},
  subscribe: () => () => {},
  close: () => {},
};

export interface SettingsHarnessOptions {
  data?: MockData;
  /** Preferences the in-memory backend already has stored. */
  settings?: Partial<AppSettings>;
  overrides?: BackendOverrides;
  /** Preferences seeded into the store without a backend round trip. */
  preferences?: Partial<AppSettingsSnapshot>;
  runtimeStatus?: RuntimeBootstrapStatusSnapshot;
  libraries?: RegisteredLibrary[];
  activeLibraryId?: string | null;
}

function withRegistryOverride(
  overrides: BackendOverrides | undefined,
  libraries: RegisteredLibrary[] | undefined,
  activeLibraryId: string | null | undefined,
): BackendOverrides | undefined {
  if (!libraries) {
    return overrides;
  }

  const registry: LibraryRegistrySnapshot = {
    active_library_id: activeLibraryId ?? null,
    libraries,
  };

  return {
    ...overrides,
    librarySetup: {
      getLibraryRegistry: async () => registry,
      ...overrides?.librarySetup,
    },
  };
}

/**
 * Drives a real `SettingsController` over the in-memory backend, the real
 * preference store, and a recording `LibrarySession`, so tests assert on the
 * view and on the calls the controller actually makes.
 */
export function createSettingsHarness({
  data = DEFAULT_MOCK_DATA,
  settings,
  overrides,
  preferences,
  runtimeStatus,
  libraries,
  activeLibraryId,
}: SettingsHarnessOptions = {}) {
  const backend: MockBackend = createMockBackend({
    data: settings
      ? { ...data, settings: { ...data.settings, ...settings } }
      : data,
    overrides: withRegistryOverride(overrides, libraries, activeLibraryId),
  });
  const librarySession: RecordingLibrarySession =
    createRecordingLibrarySession();

  const preferencesStore = createSettingsStore(
    detachedSyncChannel,
    backend,
  ).store;
  const runtimeBootstrapStore = createRuntimeBootstrapStore(backend);

  if (preferences) {
    preferencesStore.getState().patchAppSettings(preferences);
  }
  if (runtimeStatus) {
    runtimeBootstrapStore.getState().updateStatus(runtimeStatus);
  }

  const stores = {
    preferences: {
      getSnapshot: () => preferencesStore.getState().getAppSettingsSnapshot(),
      subscribe: (listener: () => void) => preferencesStore.subscribe(listener),
      hydrate: preferencesStore.getState().hydrateAppSettings,
      patch: preferencesStore.getState().patchAppSettings,
      setEqEnabled: preferencesStore.getState().setEqEnabled,
      setEqGains: preferencesStore.getState().setEqGains,
      setCrossfadeEnabled: preferencesStore.getState().setCrossfadeEnabled,
      setCrossfadeDurationMs:
        preferencesStore.getState().setCrossfadeDurationMs,
      setThemePreference: preferencesStore.getState().setThemePreference,
      setUpdatePolicy: preferencesStore.getState().setUpdatePolicy,
    },
    runtimeStatus: {
      getStatus: () => runtimeBootstrapStore.getState().status,
      subscribe: (listener: () => void) =>
        runtimeBootstrapStore.subscribe(listener),
      updateStatus: runtimeBootstrapStore.getState().updateStatus,
    },
    modelBootstrap: { reload: vi.fn() },
    library: {
      getSelectedSongIds: vi.fn((): ReadonlySet<string> => new Set()),
      clearSelection: vi.fn(),
      clearAllSeparationStatuses: vi.fn(),
      updateSeparationStatus: vi.fn(),
      loadLibrary: vi.fn(async () => {}),
    },
    queue: { removeSongIds: vi.fn() },
    player: { loadState: vi.fn(async () => {}) },
    lyrics: { clear: vi.fn() },
  };

  const notifyError = vi.fn();
  const selectDirectory = vi.fn(async (): Promise<string | null> => null);
  const changeLanguage = vi.fn();

  const controller = createSettingsController({
    backend,
    createLibrarySession: librarySession.createLibrarySession,
    selectDirectory,
    stores,
    notifyError,
    changeLanguage,
  });

  return {
    backend,
    controller,
    librarySession,
    stores,
    preferencesStore,
    runtimeBootstrapStore,
    notifyError,
    selectDirectory,
    changeLanguage,
    view: (): SettingsView => controller.getView(),
  };
}

export type SettingsHarness = ReturnType<typeof createSettingsHarness>;

export async function createInitializedSettingsHarness(
  options: SettingsHarnessOptions = {},
): Promise<SettingsHarness> {
  const harness = createSettingsHarness(options);
  await harness.controller.initialize();
  return harness;
}

export function withSettings(controller: SettingsController) {
  return function SettingsWrapper({ children }: { children: ReactNode }) {
    return (
      <SettingsControllerContext value={controller}>
        {children}
      </SettingsControllerContext>
    );
  };
}
