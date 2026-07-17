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
  mockNotifyError: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  setLyricsFontStep: mockSetLyricsFontStep,
  setEqEnabled: mockSetEqEnabled,
  setEqGains: mockSetEqGains,
  setCrossfadeEnabled: mockSetCrossfadeEnabled,
  setCrossfadeDurationMs: mockSetCrossfadeDurationMs,
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
    crossfade_duration_ms: 3000,
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
      crossfadeDurationMs: 3000,
    });
    mockSetLyricsFontStep.mockReset();
    mockSetEqEnabled.mockReset();
    mockSetEqGains.mockReset();
    mockSetCrossfadeEnabled.mockReset();
    mockSetCrossfadeDurationMs.mockReset();
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
      eqEnabled: false,
      eqGainsDb: [0, 0, 0, 0, 0],
      crossfadeEnabled: false,
      crossfadeDurationMs: 3000,
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
      crossfadeDurationMs: 3000,
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

  // ── setEqEnabled ────────────────────────────────────────────────────────

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

  // ── setEqGains ──────────────────────────────────────────────────────────

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
    // Should revert to the previous authoritative values.
    expect(store.getState().eqGainsDb).toEqual([1, 2, 3, 4, 5]);
  });

  // ── setEqBandGain ───────────────────────────────────────────────────────

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

  // ── resetEqGains ────────────────────────────────────────────────────────

  test("resetEqGains calls setEqGains with flat values", async () => {
    store.setState({ eqGainsDb: [3, -6, 0, 12, -12] });
    const flat = [0, 0, 0, 0, 0] as [number, number, number, number, number];
    const returned = makeAppSettings({ eq_gains_db: flat });
    mockSetEqGains.mockResolvedValue(returned);

    await store.getState().resetEqGains();

    expect(mockSetEqGains).toHaveBeenCalledWith(flat);
    expect(store.getState().eqGainsDb).toEqual(flat);
  });

  // ── setCrossfadeEnabled ─────────────────────────────────────────────────

  test("setCrossfadeEnabled calls api.setCrossfadeEnabled and syncs the result", async () => {
    const returned = makeAppSettings({ crossfade_enabled: true });
    mockSetCrossfadeEnabled.mockResolvedValue(returned);

    await store.getState().setCrossfadeEnabled(true);

    expect(mockSetCrossfadeEnabled).toHaveBeenCalledWith(true);
    expect(store.getState().crossfadeEnabled).toBe(true);
    expect(store.getState().hydrated).toBe(true);
  });

  test("setCrossfadeEnabled rolls back on API error", async () => {
    store.setState({ crossfadeEnabled: false });
    const error = new Error("invoke failed");
    mockSetCrossfadeEnabled.mockRejectedValue(error);

    await store.getState().setCrossfadeEnabled(true);

    expect(mockSetCrossfadeEnabled).toHaveBeenCalledWith(true);
    expect(mockNotifyError).toHaveBeenCalledWith(error);
    expect(store.getState().crossfadeEnabled).toBe(false);
  });

  test("setCrossfadeEnabled is a no-op when value hasn't changed", async () => {
    store.setState({ crossfadeEnabled: true });

    await store.getState().setCrossfadeEnabled(true);

    expect(mockSetCrossfadeEnabled).not.toHaveBeenCalled();
    expect(store.getState().crossfadeEnabled).toBe(true);
  });

  test("setCrossfadeEnabled ignores stale late-arriving response", async () => {
    // Simulate: user toggles on, then off quickly. The off response arrives
    // first; the on response arrives later but must not revert to true.
    store.setState({ crossfadeEnabled: false });

    let resolveOn!: (v: AppSettings) => void;
    let resolveOff!: (v: AppSettings) => void;
    const pOn = new Promise<AppSettings>((r) => {
      resolveOn = r;
    });
    const pOff = new Promise<AppSettings>((r) => {
      resolveOff = r;
    });

    mockSetCrossfadeEnabled.mockImplementation((enabled: boolean) =>
      enabled ? pOn : pOff,
    );

    const callOn = store.getState().setCrossfadeEnabled(true);
    const callOff = store.getState().setCrossfadeEnabled(false);

    // off resolves first
    resolveOff(makeAppSettings({ crossfade_enabled: false }));
    await callOff;
    expect(store.getState().crossfadeEnabled).toBe(false);

    // on resolves later — must not revert
    resolveOn(makeAppSettings({ crossfade_enabled: true }));
    await callOn;
    expect(store.getState().crossfadeEnabled).toBe(false);
  });

  // ── setCrossfadeDurationMs ──────────────────────────────────────────────

  test("setCrossfadeDurationMs calls api.setCrossfadeDurationMs and syncs the result", async () => {
    const returned = makeAppSettings({ crossfade_duration_ms: 5000 });
    mockSetCrossfadeDurationMs.mockResolvedValue(returned);

    await store.getState().setCrossfadeDurationMs(5000);

    expect(mockSetCrossfadeDurationMs).toHaveBeenCalledWith(5000);
    expect(store.getState().crossfadeDurationMs).toBe(5000);
    expect(store.getState().hydrated).toBe(true);
  });

  test("setCrossfadeDurationMs rolls back on API error", async () => {
    store.setState({ crossfadeDurationMs: 3000 });
    const error = new Error("invoke failed");
    mockSetCrossfadeDurationMs.mockRejectedValue(error);

    await store.getState().setCrossfadeDurationMs(5000);

    expect(mockSetCrossfadeDurationMs).toHaveBeenCalledWith(5000);
    expect(mockNotifyError).toHaveBeenCalledWith(error);
    expect(store.getState().crossfadeDurationMs).toBe(3000);
  });

  test("setCrossfadeDurationMs is a no-op when value hasn't changed", async () => {
    store.setState({ crossfadeDurationMs: 3000 });

    await store.getState().setCrossfadeDurationMs(3000);

    expect(mockSetCrossfadeDurationMs).not.toHaveBeenCalled();
    expect(store.getState().crossfadeDurationMs).toBe(3000);
  });

  test("setCrossfadeDurationMs ignores stale late-arriving response", async () => {
    // Simulate: user drags to 4000, then to 5000 before the 4000 response
    // arrives. The 5000 save resolves first; the 4000 response arrives later
    // but must not revert the store to 4000.
    store.setState({ crossfadeDurationMs: 3000 });

    let resolve4000!: (v: AppSettings) => void;
    let resolve5000!: (v: AppSettings) => void;
    const p4000 = new Promise<AppSettings>((r) => {
      resolve4000 = r;
    });
    const p5000 = new Promise<AppSettings>((r) => {
      resolve5000 = r;
    });

    mockSetCrossfadeDurationMs.mockImplementation((ms: number) =>
      ms === 4000 ? p4000 : p5000,
    );

    const call4000 = store.getState().setCrossfadeDurationMs(4000);
    const call5000 = store.getState().setCrossfadeDurationMs(5000);

    // 5000 resolves first
    resolve5000(makeAppSettings({ crossfade_duration_ms: 5000 }));
    await call5000;
    expect(store.getState().crossfadeDurationMs).toBe(5000);

    // 4000 resolves later — must not revert
    resolve4000(makeAppSettings({ crossfade_duration_ms: 4000 }));
    await call4000;
    expect(store.getState().crossfadeDurationMs).toBe(5000);
  });
});
