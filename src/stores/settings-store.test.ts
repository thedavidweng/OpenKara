import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { AppSettings } from "@/types/ipc";
import {
  createSettingsStore,
  type SettingsSyncSnapshot,
  useSettingsStore,
} from "./settings-store";

const {
  mockSetLyricsFontStep,
  mockSetEqEnabled,
  mockSetEqGains,
  mockSetCrossfadeEnabled,
  mockSetCrossfadeDurationMs,
  mockSetLibrarySortMode,
  mockSetThemePreference,
  mockNotifyError,
} = vi.hoisted(() => ({
  mockSetLyricsFontStep: vi.fn<(step: number) => Promise<AppSettings>>(),
  mockSetEqEnabled: vi.fn<(enabled: boolean) => Promise<AppSettings>>(),
  mockSetEqGains:
    vi.fn<
      (
        gainsDb: [number, number, number, number, number],
      ) => Promise<AppSettings>
    >(),
  mockSetCrossfadeEnabled: vi.fn<(enabled: boolean) => Promise<AppSettings>>(),
  mockSetCrossfadeDurationMs:
    vi.fn<(durationMs: number) => Promise<AppSettings>>(),
  mockSetLibrarySortMode: vi.fn<(mode: string) => Promise<AppSettings>>(),
  mockSetThemePreference: vi.fn<(preference: string) => Promise<AppSettings>>(),
  mockNotifyError: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  setLyricsFontStep: mockSetLyricsFontStep,
  setEqEnabled: mockSetEqEnabled,
  setEqGains: mockSetEqGains,
  setCrossfadeEnabled: mockSetCrossfadeEnabled,
  setCrossfadeDurationMs: mockSetCrossfadeDurationMs,
  setLibrarySortMode: mockSetLibrarySortMode,
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
    eq_enabled: false,
    eq_gains_db: [0, 0, 0, 0, 0],
    crossfade_enabled: false,
    crossfade_duration_ms: 3_000,
    library_sort_mode: "recently_imported",
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
      eqEnabled: false,
      eqGainsDb: [0, 0, 0, 0, 0],
      crossfadeEnabled: false,
      crossfadeDurationMs: 3_000,
      librarySortMode: "recently_imported",
      themePreference: "dark",
    });
    mockSetLyricsFontStep.mockReset();
    mockSetEqEnabled.mockReset();
    mockSetEqGains.mockReset();
    mockSetCrossfadeEnabled.mockReset();
    mockSetCrossfadeDurationMs.mockReset();
    mockSetLibrarySortMode.mockReset();
    mockSetThemePreference.mockReset();
    mockNotifyError.mockReset();
  });

  afterEach(() => {
    dispose?.();
  });

  test("toggle flips isOpen from false to true", () => {
    store.getState().toggle();
    expect(store.getState().isOpen).toBe(true);
  });

  test("toggle flips isOpen from true to false", () => {
    store.setState({ isOpen: true });
    store.getState().toggle();
    expect(store.getState().isOpen).toBe(false);
  });

  test("close sets isOpen to false", () => {
    store.setState({ isOpen: true });
    store.getState().close();
    expect(store.getState().isOpen).toBe(false);
  });

  test("close is a no-op when already closed", () => {
    store.getState().close();
    expect(store.getState().isOpen).toBe(false);
  });

  test("open sets isOpen to true", () => {
    store.getState().open();
    expect(store.getState().isOpen).toBe(true);
  });

  test("open is a no-op when already open", () => {
    store.setState({ isOpen: true });
    store.getState().open();
    expect(store.getState().isOpen).toBe(true);
  });

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
      library_sort_mode: "artist_asc",
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
    expect(state.librarySortMode).toBe("artist_asc");
    expect(state.themePreference).toBe("dark");
  });

  test("hydrateAppSettings maps snake_case keys to camelCase", () => {
    const settings = makeAppSettings({ cover_art_backdrop: false });
    store.getState().hydrateAppSettings(settings);
    expect(store.getState().coverArtBackdrop).toBe(false);
  });

  test("patchAppSettings applies partial update", () => {
    store
      .getState()
      .patchAppSettings({ language: "ko", stemMode: "four_stem" });
    const state = store.getState();
    expect(state.language).toBe("ko");
    expect(state.stemMode).toBe("four_stem");
    expect(state.modelVariant).toBe("htdemucs");
    expect(state.lyricsFontStep).toBe(0);
  });

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
    expect(store.getState().lyricsFontStep).toBe(0);
  });

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

  test("setLibrarySortMode applies optimistic update immediately", async () => {
    let resolveInvocation: (value: AppSettings) => void = () => {};
    mockSetLibrarySortMode.mockImplementation(
      () =>
        new Promise<AppSettings>((resolve) => {
          resolveInvocation = resolve;
        }),
    );

    let pending = true;
    const promise = store.getState().setLibrarySortMode("title_asc");
    void promise.finally(() => {
      pending = false;
    });

    await Promise.resolve();
    expect(store.getState().librarySortMode).toBe("title_asc");
    expect(pending).toBe(true);

    resolveInvocation(makeAppSettings({ library_sort_mode: "title_asc" }));
    await promise;

    expect(mockSetLibrarySortMode).toHaveBeenCalledWith("title_asc");
    expect(store.getState().librarySortMode).toBe("title_asc");
  });

  test("setLibrarySortMode applies authoritative snapshot on success", async () => {
    const returned = makeAppSettings({
      library_sort_mode: "artist_asc",
      lyrics_font_step: 3,
    });
    mockSetLibrarySortMode.mockResolvedValue(returned);

    await store.getState().setLibrarySortMode("artist_asc");

    expect(mockSetLibrarySortMode).toHaveBeenCalledWith("artist_asc");
    expect(store.getState().librarySortMode).toBe("artist_asc");
    // This command owns only the sort preference; applying the full response
    // could overwrite another locally pending setting.
    expect(store.getState().lyricsFontStep).toBe(0);
  });

  test("setLibrarySortMode rolls back optimistic update on failure", async () => {
    const error = new Error("invoke failed");
    mockSetLibrarySortMode.mockRejectedValue(error);

    await store.getState().setLibrarySortMode("title_asc");

    expect(mockNotifyError).toHaveBeenCalledWith(error);
    expect(store.getState().librarySortMode).toBe("recently_imported");
  });

  test("setLibrarySortMode rollback does not clobber other settings changed in flight", async () => {
    // Simulate a concurrent lyricsFontStep change that succeeds while the
    // sort-mode save is in flight. The rollback must only restore
    // librarySortMode, not revert lyricsFontStep.
    let rejectSort: (error: Error) => void = () => {};
    mockSetLibrarySortMode.mockReturnValue(
      new Promise<AppSettings>((_resolve, reject) => {
        rejectSort = reject;
      }),
    );

    const sortPromise = store.getState().setLibrarySortMode("title_asc");
    await Promise.resolve();

    // While the sort save is pending, change lyricsFontStep directly in the
    // store (simulating a successful concurrent setter).
    store.setState({ lyricsFontStep: 5 });

    rejectSort(new Error("sort fail"));
    await sortPromise.catch(() => {});

    expect(store.getState().librarySortMode).toBe("recently_imported");
    expect(store.getState().lyricsFontStep).toBe(5);
  });

  test("setLibrarySortMode ignores stale success from a superseded call", async () => {
    let resolveFirst: (value: AppSettings) => void = () => {};
    let resolveSecond: (value: AppSettings) => void = () => {};
    mockSetLibrarySortMode.mockImplementation((mode: string) =>
      mode === "title_asc"
        ? new Promise<AppSettings>((resolve) => {
            resolveFirst = resolve;
          })
        : new Promise<AppSettings>((resolve) => {
            resolveSecond = resolve;
          }),
    );

    const firstPromise = store.getState().setLibrarySortMode("title_asc");
    const secondPromise = store.getState().setLibrarySortMode("artist_asc");
    await Promise.resolve();

    // Resolve the slower first call after the second has already started.
    resolveFirst(makeAppSettings({ library_sort_mode: "title_asc" }));
    await firstPromise;

    // The second call is still in flight; the stale first response must not
    // overwrite the optimistic artist_asc state.
    expect(store.getState().librarySortMode).toBe("artist_asc");

    resolveSecond(makeAppSettings({ library_sort_mode: "artist_asc" }));
    await secondPromise;

    expect(store.getState().librarySortMode).toBe("artist_asc");
  });

  test("setLibrarySortMode ignores stale failure from a superseded call", async () => {
    let resolveSecond: (value: AppSettings) => void = () => {};
    let rejectFirst: (error: Error) => void = () => {};
    mockSetLibrarySortMode.mockImplementation((mode: string) =>
      mode === "title_asc"
        ? new Promise<AppSettings>((_resolve, reject) => {
            rejectFirst = reject;
          })
        : new Promise<AppSettings>((resolve) => {
            resolveSecond = resolve;
          }),
    );

    const firstPromise = store.getState().setLibrarySortMode("title_asc");
    const secondPromise = store.getState().setLibrarySortMode("artist_asc");
    await Promise.resolve();

    // The first call fails after the second has started. The stale rollback
    // must not overwrite the optimistic artist_asc state.
    rejectFirst(new Error("first failed"));
    await firstPromise.catch(() => {});
    expect(store.getState().librarySortMode).toBe("artist_asc");
    expect(mockNotifyError).not.toHaveBeenCalled();

    resolveSecond(makeAppSettings({ library_sort_mode: "artist_asc" }));
    await secondPromise;
    expect(store.getState().librarySortMode).toBe("artist_asc");
  });

  test("setLibrarySortMode restores the committed mode after two rapid failures", async () => {
    let rejectFirst: (error: Error) => void = () => {};
    let rejectSecond: (error: Error) => void = () => {};
    mockSetLibrarySortMode.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((_resolve, reject) => {
          rejectFirst = reject;
        }),
    );
    mockSetLibrarySortMode.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((_resolve, reject) => {
          rejectSecond = reject;
        }),
    );

    const first = store.getState().setLibrarySortMode("title_asc");
    const second = store.getState().setLibrarySortMode("artist_asc");

    rejectSecond(new Error("newer request rejected"));
    await second;
    expect(store.getState().librarySortMode).toBe("title_asc");

    rejectFirst(new Error("older request rejected"));
    await first;
    expect(store.getState().librarySortMode).toBe("recently_imported");
    expect(mockNotifyError).toHaveBeenCalledTimes(1);
  });

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
      eqEnabled: false,
      eqGainsDb: [0, 0, 0, 0, 0],
      librarySortMode: "title_asc",
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
      eqEnabled: false,
      eqGainsDb: [0, 0, 0, 0, 0],
      crossfadeEnabled: false,
      crossfadeDurationMs: 3_000,
      librarySortMode: "title_asc",
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

  test("setEqEnabled optimistically updates and hydrates on success", async () => {
    const returned = makeAppSettings({ eq_enabled: true });
    mockSetEqEnabled.mockResolvedValue(returned);

    await store.getState().setEqEnabled(true);

    expect(mockSetEqEnabled).toHaveBeenCalledWith(true);
    expect(store.getState().eqEnabled).toBe(true);
  });

  test("setEqEnabled reverts on failure", async () => {
    const error = new Error("invoke failed");
    mockSetEqEnabled.mockRejectedValue(error);

    await store.getState().setEqEnabled(true);

    expect(mockNotifyError).toHaveBeenCalledWith(error);
    expect(store.getState().eqEnabled).toBe(false);
  });

  test("setEqGains optimistically updates and hydrates on success", async () => {
    const gains: [number, number, number, number, number] = [0, 3, -6, 0, 12];
    const returned = makeAppSettings({ eq_gains_db: gains });
    mockSetEqGains.mockResolvedValue(returned);

    await store.getState().setEqGains(gains);

    expect(mockSetEqGains).toHaveBeenCalledWith(gains);
    expect(store.getState().eqGainsDb).toEqual(gains);
  });

  test("setEqGains reverts optimistic state and calls notifyError on failure", async () => {
    const error = new Error("invoke failed");
    mockSetEqGains.mockRejectedValue(error);
    store.setState({ eqGainsDb: [1, 2, 3, 4, 5] });

    await store.getState().setEqGains([0, 0, 6, 0, 0]);

    expect(mockNotifyError).toHaveBeenCalledWith(error);
    expect(store.getState().eqGainsDb).toEqual([1, 2, 3, 4, 5]);
  });

  test("setEqEnabled discards an older successful response", async () => {
    let resolveFirst: (value: AppSettings) => void = () => {};
    let resolveSecond: (value: AppSettings) => void = () => {};
    mockSetEqEnabled.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((resolve) => {
          resolveFirst = resolve;
        }),
    );
    mockSetEqEnabled.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((resolve) => {
          resolveSecond = resolve;
        }),
    );

    const first = store.getState().setEqEnabled(true);
    const second = store.getState().setEqEnabled(false);

    resolveSecond(makeAppSettings({ eq_enabled: false }));
    await second;
    resolveFirst(makeAppSettings({ eq_enabled: true }));
    await first;

    expect(store.getState().eqEnabled).toBe(false);
  });

  test("setEqEnabled ignores an older failure", async () => {
    let rejectFirst: (error: Error) => void = () => {};
    mockSetEqEnabled.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((_resolve, reject) => {
          rejectFirst = reject;
        }),
    );
    mockSetEqEnabled.mockResolvedValueOnce(
      makeAppSettings({ eq_enabled: false }),
    );

    const first = store.getState().setEqEnabled(true);
    const second = store.getState().setEqEnabled(false);

    await second;
    rejectFirst(new Error("stale failure"));
    await first;

    expect(store.getState().eqEnabled).toBe(false);
    expect(mockNotifyError).not.toHaveBeenCalled();
  });

  test("setEqEnabled restores the confirmed value after two rapid failures", async () => {
    let rejectFirst: (error: Error) => void = () => {};
    let rejectSecond: (error: Error) => void = () => {};
    mockSetEqEnabled.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((_resolve, reject) => {
          rejectFirst = reject;
        }),
    );
    mockSetEqEnabled.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((_resolve, reject) => {
          rejectSecond = reject;
        }),
    );

    const first = store.getState().setEqEnabled(true);
    const second = store.getState().setEqEnabled(false);

    rejectSecond(new Error("newer request rejected"));
    await second;
    // The older request is still pending, so its desired value remains visible.
    expect(store.getState().eqEnabled).toBe(true);

    const firstError = new Error("older request rejected");
    rejectFirst(firstError);
    await first;

    expect(store.getState().eqEnabled).toBe(false);
    expect(mockNotifyError).toHaveBeenCalledTimes(1);
  });

  test("setEqEnabled preserves a pending request across a full settings snapshot", async () => {
    let resolveRequest: (value: AppSettings) => void = () => {};
    mockSetEqEnabled.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((resolve) => {
          resolveRequest = resolve;
        }),
    );

    const request = store.getState().setEqEnabled(true);
    store.getState().hydrateAppSettings(makeAppSettings({ eq_enabled: false }));
    expect(store.getState().eqEnabled).toBe(true);
    resolveRequest(makeAppSettings({ eq_enabled: true }));
    await request;

    expect(store.getState().eqEnabled).toBe(true);
  });

  test("setEqGains discards an older successful response", async () => {
    let resolveFirst: (value: AppSettings) => void = () => {};
    let resolveSecond: (value: AppSettings) => void = () => {};
    mockSetEqGains.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((resolve) => {
          resolveFirst = resolve;
        }),
    );
    mockSetEqGains.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((resolve) => {
          resolveSecond = resolve;
        }),
    );

    const firstGains: [number, number, number, number, number] = [
      6, 0, 0, 0, 0,
    ];
    const secondGains: [number, number, number, number, number] = [
      0, 0, 0, 0, 12,
    ];
    const first = store.getState().setEqGains(firstGains);
    const second = store.getState().setEqGains(secondGains);

    resolveSecond(makeAppSettings({ eq_gains_db: secondGains }));
    await second;
    resolveFirst(makeAppSettings({ eq_gains_db: firstGains }));
    await first;

    expect(store.getState().eqGainsDb).toEqual(secondGains);
  });

  test("setEqGains ignores an older failure", async () => {
    let rejectFirst: (error: Error) => void = () => {};
    mockSetEqGains.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((_resolve, reject) => {
          rejectFirst = reject;
        }),
    );
    const newest: [number, number, number, number, number] = [0, 0, 0, 0, 12];
    mockSetEqGains.mockResolvedValueOnce(
      makeAppSettings({ eq_gains_db: newest }),
    );

    const first = store.getState().setEqGains([6, 0, 0, 0, 0]);
    const second = store.getState().setEqGains(newest);

    await second;
    rejectFirst(new Error("stale failure"));
    await first;

    expect(store.getState().eqGainsDb).toEqual(newest);
    expect(mockNotifyError).not.toHaveBeenCalled();
  });

  test("setEqGains restores the confirmed value after two rapid failures", async () => {
    const confirmed: [number, number, number, number, number] = [1, 2, 3, 4, 5];
    const firstGains: [number, number, number, number, number] = [
      6, 0, 0, 0, 0,
    ];
    const secondGains: [number, number, number, number, number] = [
      0, 0, 0, 0, 12,
    ];
    let rejectFirst: (error: Error) => void = () => {};
    let rejectSecond: (error: Error) => void = () => {};
    store.setState({ eqGainsDb: confirmed });
    mockSetEqGains.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((_resolve, reject) => {
          rejectFirst = reject;
        }),
    );
    mockSetEqGains.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((_resolve, reject) => {
          rejectSecond = reject;
        }),
    );

    const first = store.getState().setEqGains(firstGains);
    const second = store.getState().setEqGains(secondGains);

    rejectSecond(new Error("newer request rejected"));
    await second;
    expect(store.getState().eqGainsDb).toEqual(firstGains);

    rejectFirst(new Error("older request rejected"));
    await first;

    expect(store.getState().eqGainsDb).toEqual(confirmed);
    expect(mockNotifyError).toHaveBeenCalledTimes(1);
  });

  test("cross-field successful snapshots update only their owned field", async () => {
    let resolveEnabled: (value: AppSettings) => void = () => {};
    mockSetEqEnabled.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((resolve) => {
          resolveEnabled = resolve;
        }),
    );
    const newest: [number, number, number, number, number] = [0, 3, 0, 0, 0];
    mockSetEqGains.mockResolvedValueOnce(
      makeAppSettings({ eq_enabled: false, eq_gains_db: newest }),
    );

    const toggle = store.getState().setEqEnabled(true);
    const gains = store.getState().setEqGains(newest);

    await gains;
    resolveEnabled(
      makeAppSettings({ eq_enabled: true, eq_gains_db: [0, 0, 0, 0, 0] }),
    );
    await toggle;

    expect(store.getState().eqEnabled).toBe(true);
    expect(store.getState().eqGainsDb).toEqual(newest);
  });

  test("a failed EQ toggle rolls back after a newer gains request", async () => {
    const error = new Error("toggle rejected");
    let rejectEnabled: (error: Error) => void = () => {};
    mockSetEqEnabled.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((_resolve, reject) => {
          rejectEnabled = reject;
        }),
    );
    const newest: [number, number, number, number, number] = [0, 3, 0, 0, 0];
    mockSetEqGains.mockResolvedValueOnce(
      makeAppSettings({ eq_enabled: false, eq_gains_db: newest }),
    );

    const toggle = store.getState().setEqEnabled(true);
    const gains = store.getState().setEqGains(newest);

    await gains;
    rejectEnabled(error);
    await toggle;

    expect(store.getState().eqEnabled).toBe(false);
    expect(store.getState().eqGainsDb).toEqual(newest);
    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });

  test("a failed EQ gains mutation rolls back after a newer toggle", async () => {
    const error = new Error("gains rejected");
    const previous: [number, number, number, number, number] = [1, 2, 3, 4, 5];
    const requested: [number, number, number, number, number] = [6, 0, 0, 0, 0];
    store.setState({ eqGainsDb: previous });

    let rejectGains: (error: Error) => void = () => {};
    mockSetEqGains.mockImplementationOnce(
      () =>
        new Promise<AppSettings>((_resolve, reject) => {
          rejectGains = reject;
        }),
    );
    mockSetEqEnabled.mockResolvedValueOnce(
      makeAppSettings({ eq_enabled: true, eq_gains_db: [0, 0, 0, 0, 0] }),
    );

    const gains = store.getState().setEqGains(requested);
    const toggle = store.getState().setEqEnabled(true);

    await toggle;
    rejectGains(error);
    await gains;

    expect(store.getState().eqEnabled).toBe(true);
    expect(store.getState().eqGainsDb).toEqual(previous);
    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });

  test("setEqBandGain updates a single band and calls setEqGains", async () => {
    const returned = makeAppSettings({ eq_gains_db: [0, 0, 6, 0, 0] });
    mockSetEqGains.mockResolvedValue(returned);

    await store.getState().setEqBandGain(2, 6);

    expect(mockSetEqGains).toHaveBeenCalledWith([0, 0, 6, 0, 0]);
    expect(store.getState().eqGainsDb).toEqual([0, 0, 6, 0, 0]);
  });

  test("setEqBandGain clamps to ±12 dB", async () => {
    const returned = makeAppSettings({ eq_gains_db: [12, 0, 0, 0, 0] });
    mockSetEqGains.mockResolvedValue(returned);

    await store.getState().setEqBandGain(0, 20);

    expect(mockSetEqGains).toHaveBeenCalledWith([12, 0, 0, 0, 0]);
  });

  test("setEqBandGain is a no-op when value is unchanged", async () => {
    await store.getState().setEqBandGain(0, 0);

    expect(mockSetEqGains).not.toHaveBeenCalled();
  });

  test("resetEqGains calls setEqGains with flat values", async () => {
    store.setState({ eqGainsDb: [3, -6, 0, 12, -12] });
    const flat = [0, 0, 0, 0, 0] as [number, number, number, number, number];
    const returned = makeAppSettings({ eq_gains_db: flat });
    mockSetEqGains.mockResolvedValue(returned);

    await store.getState().resetEqGains();

    expect(mockSetEqGains).toHaveBeenCalledWith(flat);
    expect(store.getState().eqGainsDb).toEqual(flat);
  });

  test("setCrossfadeEnabled optimistically updates and hydrates on success", async () => {
    const returned = makeAppSettings({ crossfade_enabled: true });
    mockSetCrossfadeEnabled.mockResolvedValue(returned);

    await store.getState().setCrossfadeEnabled(true);

    expect(mockSetCrossfadeEnabled).toHaveBeenCalledWith(true);
    expect(store.getState().crossfadeEnabled).toBe(true);
  });

  test("setCrossfadeEnabled reverts on failure", async () => {
    const error = new Error("invoke failed");
    mockSetCrossfadeEnabled.mockRejectedValue(error);

    await store.getState().setCrossfadeEnabled(true);

    expect(mockNotifyError).toHaveBeenCalledWith(error);
    expect(store.getState().crossfadeEnabled).toBe(false);
  });

  test("setCrossfadeDurationMs optimistically updates and hydrates on success", async () => {
    const returned = makeAppSettings({ crossfade_duration_ms: 5_000 });
    mockSetCrossfadeDurationMs.mockResolvedValue(returned);

    await store.getState().setCrossfadeDurationMs(5_000);

    expect(mockSetCrossfadeDurationMs).toHaveBeenCalledWith(5_000);
    expect(store.getState().crossfadeDurationMs).toBe(5_000);
  });

  test("setCrossfadeDurationMs reverts optimistic state and calls notifyError on failure", async () => {
    const error = new Error("invoke failed");
    mockSetCrossfadeDurationMs.mockRejectedValue(error);
    store.setState({ crossfadeDurationMs: 2_000 });

    await store.getState().setCrossfadeDurationMs(5_000);

    expect(mockNotifyError).toHaveBeenCalledWith(error);
    expect(store.getState().crossfadeDurationMs).toBe(2_000);
  });

  test("setCrossfadeDurationMs clamps to [500, 10000]", async () => {
    const returned = makeAppSettings({ crossfade_duration_ms: 500 });
    mockSetCrossfadeDurationMs.mockResolvedValue(returned);

    await store.getState().setCrossfadeDurationMs(100);

    expect(mockSetCrossfadeDurationMs).toHaveBeenCalledWith(500);
  });

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
    mockSetThemePreference.mockResolvedValue(
      makeAppSettings({ theme_preference: "light" }),
    );
    const firstPromise = store.getState().setThemePreference("light");

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
