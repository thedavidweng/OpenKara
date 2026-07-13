import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { AppSettings } from "@/types/ipc";
import {
  createSettingsStore,
  type SettingsSyncSnapshot,
  useSettingsStore,
} from "./settings-store";

const { mockSetLyricsFontStep, mockSetThemePreference, mockNotifyError } =
  vi.hoisted(() => ({
    mockSetLyricsFontStep: vi.fn<(step: number) => Promise<AppSettings>>(),
    mockSetThemePreference:
      vi.fn<(preference: string) => Promise<AppSettings>>(),
    mockNotifyError: vi.fn(),
  }));

vi.mock("@/lib/tauri", () => ({
  setLyricsFontStep: mockSetLyricsFontStep,
  setThemePreference: mockSetThemePreference,
}));

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
}));

vi.mock("@/runtime/webview-sync", () => ({
  createWebviewSyncChannel: () => ({
    subscribe: () => () => {},
    publish: () => {},
    close: () => {},
  }),
}));

interface FakeChannel {
  onmessage: ((event: { data: unknown }) => void) | null;
  postMessage: (data: unknown) => void;
  close: () => void;
}

function makeAppSettings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    stem_mode: "two_stem",
    model_variant: "htdemucs",
    language: null,
    hide_batch_separate: false,
    cover_art_backdrop: true,
    lyrics_font_step: 0,
    execution_provider: "cpu",
    available_execution_providers: ["cpu"],
    theme_preference: "dark",
    ...overrides,
  };
}

describe("settings-store sync", () => {
  beforeEach(() => {
    useSettingsStore.setState({ isOpen: false });
  });

  test("syncs settings overlay visibility across webview contexts", async () => {
    const channelsByName = new Map<string, Set<FakeChannel>>();
    const channelFactory = (name: string) => {
      const peers = channelsByName.get(name) ?? new Set<FakeChannel>();
      channelsByName.set(name, peers);

      const channel: FakeChannel = {
        onmessage: null,
        postMessage(data: unknown) {
          for (const peer of peers) {
            if (peer === channel) {
              continue;
            }
            peer.onmessage?.({ data });
          }
        },
        close() {
          peers.delete(channel);
        },
      };

      peers.add(channel);
      return channel;
    };

    const actual = await vi.importActual<
      typeof import("@/runtime/webview-sync")
    >("@/runtime/webview-sync");

    const primary = createSettingsStore(
      actual.createWebviewSyncChannel<SettingsSyncSnapshot>("settings", {
        channelFactory,
        originId: "primary",
      }),
    );
    const secondary = createSettingsStore(
      actual.createWebviewSyncChannel<SettingsSyncSnapshot>("settings", {
        channelFactory,
        originId: "secondary",
      }),
    );

    primary.store.getState().open();

    expect(secondary.store.getState().isOpen).toBe(true);

    primary.dispose();
    secondary.dispose();
  });
});

describe("settings-store actions", () => {
  let store: ReturnType<typeof createSettingsStore>["store"];
  let dispose: (() => void) | undefined;

  beforeEach(() => {
    const instance = createSettingsStore();
    store = instance.store;
    dispose = instance.dispose;
    store.setState({
      isOpen: false,
      hydrated: false,
      stemMode: "two_stem",
      modelVariant: "htdemucs",
      language: null,
      hideBatchSeparate: false,
      coverArtBackdrop: true,
      lyricsFontStep: 0,
      executionProvider: "cpu",
      availableExecutionProviders: ["cpu"],
      themePreference: "dark",
    });
    mockSetLyricsFontStep.mockReset();
    mockSetThemePreference.mockReset();
    mockNotifyError.mockReset();
  });

  afterEach(() => {
    dispose?.();
  });

  // ── toggle ──────────────────────────────────────────────────────────────

  test("toggle flips isOpen from false to true", () => {
    store.getState().toggle();
    expect(store.getState().isOpen).toBe(true);
  });

  test("toggle flips isOpen from true to false", () => {
    store.setState({ isOpen: true });
    store.getState().toggle();
    expect(store.getState().isOpen).toBe(false);
  });

  // ── close ───────────────────────────────────────────────────────────────

  test("close sets isOpen to false", () => {
    store.setState({ isOpen: true });
    store.getState().close();
    expect(store.getState().isOpen).toBe(false);
  });

  test("close is a no-op when already closed", () => {
    store.getState().close();
    expect(store.getState().isOpen).toBe(false);
  });

  // ── open ────────────────────────────────────────────────────────────────

  test("open sets isOpen to true", () => {
    store.getState().open();
    expect(store.getState().isOpen).toBe(true);
  });

  test("open is a no-op when already open", () => {
    store.setState({ isOpen: true });
    store.getState().open();
    expect(store.getState().isOpen).toBe(true);
  });

  // ── hydrateAppSettings ──────────────────────────────────────────────────

  test("hydrateAppSettings sets all fields from AppSettings object", () => {
    const settings = makeAppSettings({
      stem_mode: "four_stem",
      model_variant: "htdemucs_ft",
      language: "ja",
      hide_batch_separate: true,
      cover_art_backdrop: false,
      lyrics_font_step: 1,
      execution_provider: "xnnpack",
      available_execution_providers: ["cpu", "xnnpack"],
      theme_preference: "dark",
    });

    store.getState().hydrateAppSettings(settings);

    const state = store.getState();
    expect(state.hydrated).toBe(true);
    expect(state.stemMode).toBe("four_stem");
    expect(state.modelVariant).toBe("htdemucs_ft");
    expect(state.language).toBe("ja");
    expect(state.hideBatchSeparate).toBe(true);
    expect(state.coverArtBackdrop).toBe(false);
    expect(state.lyricsFontStep).toBe(1);
    expect(state.executionProvider).toBe("xnnpack");
    expect(state.availableExecutionProviders).toEqual(["cpu", "xnnpack"]);
  });

  test("hydrateAppSettings maps snake_case keys to camelCase", () => {
    const settings = makeAppSettings({ cover_art_backdrop: false });
    store.getState().hydrateAppSettings(settings);
    expect(store.getState().coverArtBackdrop).toBe(false);
  });

  // ── patchAppSettings ────────────────────────────────────────────────────

  test("patchAppSettings applies partial update", () => {
    store
      .getState()
      .patchAppSettings({ language: "ko", stemMode: "four_stem" });
    const state = store.getState();
    expect(state.language).toBe("ko");
    expect(state.stemMode).toBe("four_stem");
    // Other fields unchanged
    expect(state.modelVariant).toBe("htdemucs");
    expect(state.lyricsFontStep).toBe(0);
  });

  test("open sets isOpen to true", () => {
    store.getState().open();
    expect(store.getState().isOpen).toBe(true);
  });

  // ── setLyricsFontStep ──────────────────────────────────────────────────

  test("setLyricsFontStep updates state on success", async () => {
    const returned = makeAppSettings({ lyrics_font_step: 2 });
    mockSetLyricsFontStep.mockResolvedValue(returned);

    await store.getState().setLyricsFontStep(2);

    expect(mockSetLyricsFontStep).toHaveBeenCalledWith(2);
    expect(store.getState().lyricsFontStep).toBe(2);
    expect(store.getState().hydrated).toBe(true);
  });

  test("setLyricsFontStep calls notifyError on failure", async () => {
    const error = new Error("invoke failed");
    mockSetLyricsFontStep.mockRejectedValue(error);

    await store.getState().setLyricsFontStep(1);

    expect(mockNotifyError).toHaveBeenCalledWith(error);
    // State unchanged
    expect(store.getState().lyricsFontStep).toBe(0);
  });

  // ── adjustLyricsFontStep ───────────────────────────────────────────────

  test("adjustLyricsFontStep adds delta to current step", async () => {
    store.setState({ lyricsFontStep: 0 });
    const returned = makeAppSettings({ lyrics_font_step: 1 });
    mockSetLyricsFontStep.mockResolvedValue(returned);

    await store.getState().adjustLyricsFontStep(1);

    expect(mockSetLyricsFontStep).toHaveBeenCalledWith(1);
    expect(store.getState().lyricsFontStep).toBe(1);
  });

  test("adjustLyricsFontStep clamps to upper bound of 2", async () => {
    store.setState({ lyricsFontStep: 2 });

    await store.getState().adjustLyricsFontStep(1);

    expect(mockSetLyricsFontStep).not.toHaveBeenCalled();
    expect(store.getState().lyricsFontStep).toBe(2);
  });

  test("adjustLyricsFontStep clamps to lower bound of -2", async () => {
    store.setState({ lyricsFontStep: -2 });

    await store.getState().adjustLyricsFontStep(-1);

    expect(mockSetLyricsFontStep).not.toHaveBeenCalled();
    expect(store.getState().lyricsFontStep).toBe(-2);
  });

  test("adjustLyricsFontStep allows moving within bounds", async () => {
    store.setState({ lyricsFontStep: -2 });
    const returned = makeAppSettings({ lyrics_font_step: -1 });
    mockSetLyricsFontStep.mockResolvedValue(returned);

    await store.getState().adjustLyricsFontStep(1);

    expect(mockSetLyricsFontStep).toHaveBeenCalledWith(-1);
    expect(store.getState().lyricsFontStep).toBe(-1);
  });

  test("adjustLyricsFontStep is a no-op when delta would not change step", async () => {
    store.setState({ lyricsFontStep: 0 });

    await store.getState().adjustLyricsFontStep(0);

    expect(mockSetLyricsFontStep).not.toHaveBeenCalled();
  });

  // ── resetLyricsFontStep ────────────────────────────────────────────────

  test("resetLyricsFontStep is a no-op when already 0", async () => {
    store.setState({ lyricsFontStep: 0 });

    await store.getState().resetLyricsFontStep();

    expect(mockSetLyricsFontStep).not.toHaveBeenCalled();
  });

  test("resetLyricsFontStep calls setLyricsFontStep(0) when not 0", async () => {
    store.setState({ lyricsFontStep: 2 });
    const returned = makeAppSettings({ lyrics_font_step: 0 });
    mockSetLyricsFontStep.mockResolvedValue(returned);

    await store.getState().resetLyricsFontStep();

    expect(mockSetLyricsFontStep).toHaveBeenCalledWith(0);
    expect(store.getState().lyricsFontStep).toBe(0);
  });

  // ── getAppSettingsSnapshot ──────────────────────────────────────────────

  test("getAppSettingsSnapshot returns subset of state without isOpen", () => {
    store.setState({
      isOpen: true,
      hydrated: true,
      stemMode: "four_stem",
      modelVariant: "htdemucs_ft",
      language: "en",
      hideBatchSeparate: true,
      coverArtBackdrop: false,
      lyricsFontStep: 1,
      executionProvider: "xnnpack",
      availableExecutionProviders: ["cpu", "xnnpack"],
      themePreference: "dark",
    });

    const snapshot = store.getState().getAppSettingsSnapshot();

    expect(snapshot).toEqual({
      hydrated: true,
      stemMode: "four_stem",
      modelVariant: "htdemucs_ft",
      language: "en",
      hideBatchSeparate: true,
      coverArtBackdrop: false,
      lyricsFontStep: 1,
      executionProvider: "xnnpack",
      availableExecutionProviders: ["cpu", "xnnpack"],
      themePreference: "dark",
    });
    expect(snapshot).not.toHaveProperty("isOpen");
  });

  test("getAppSettingsSnapshot does not include action methods", () => {
    const snapshot = store.getState().getAppSettingsSnapshot();

    expect(snapshot).not.toHaveProperty("toggle");
    expect(snapshot).not.toHaveProperty("close");
    expect(snapshot).not.toHaveProperty("open");
    expect(snapshot).not.toHaveProperty("setLyricsFontStep");
    expect(snapshot).not.toHaveProperty("adjustLyricsFontStep");
    expect(snapshot).not.toHaveProperty("resetLyricsFontStep");
    expect(snapshot).not.toHaveProperty("hydrateAppSettings");
    expect(snapshot).not.toHaveProperty("patchAppSettings");
  });

  // ── setThemePreference ──────────────────────────────────────────────────

  test("setThemePreference updates state on success", async () => {
    const returned = makeAppSettings({ theme_preference: "light" });
    mockSetThemePreference.mockResolvedValue(returned);

    await store.getState().setThemePreference("light");

    expect(mockSetThemePreference).toHaveBeenCalledWith("light");
    expect(store.getState().themePreference).toBe("light");
  });

  test("setThemePreference is a no-op when preference is unchanged", async () => {
    store.setState({ themePreference: "dark" });

    await store.getState().setThemePreference("dark");

    expect(mockSetThemePreference).not.toHaveBeenCalled();
  });

  test("setThemePreference rolls back and notifies on failure", async () => {
    const error = new Error("ipc failure");
    mockSetThemePreference.mockRejectedValue(error);

    await store.getState().setThemePreference("light");

    expect(store.getState().themePreference).toBe("dark");
    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });

  test("setThemePreference does not roll back if a newer mutation superseded it", async () => {
    // Start a mutation to "light"
    mockSetThemePreference.mockResolvedValue(
      makeAppSettings({ theme_preference: "light" }),
    );
    const firstPromise = store.getState().setThemePreference("light");

    // Before it resolves, start a second mutation to "system"
    mockSetThemePreference.mockResolvedValue(
      makeAppSettings({ theme_preference: "system" }),
    );
    const secondPromise = store.getState().setThemePreference("system");

    await firstPromise;
    await secondPromise;

    // The second mutation wins; the first's syncPatch is skipped because the
    // generation no longer matches.
    expect(store.getState().themePreference).toBe("system");
  });
});
