import { describe, expect, test, vi } from "vitest";

const mockGetAppSettingsSnapshot = vi.hoisted(() =>
  vi.fn().mockReturnValue({
    hydrated: true,
    stemMode: "four_stem",
    modelVariant: "htdemucs_ft",
    language: "ja",
    hideBatchSeparate: true,
    coverArtBackdrop: true,
    hideUpgradeAll: true,
    lyricsFontStep: 2,
    executionProvider: "cpu",
    availableExecutionProviders: ["cpu"],
    compatibleExecutionProviders: ["cpu"],
  }),
);

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: {
    getState: () => ({
      getAppSettingsSnapshot: mockGetAppSettingsSnapshot,
    }),
  },
}));

const mockReactUse = vi.hoisted(() => vi.fn());

vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return {
    ...actual,
    use: mockReactUse,
  };
});

import {
  createSettingsOverlayTestContextValue,
  useSettingsOverlay,
} from "./SettingsOverlay.context";

describe("createSettingsOverlayTestContextValue", () => {
  test("returns default state and no-op actions when called with no args", () => {
    const value = createSettingsOverlayTestContextValue();

    expect(value.state.libraryPath).toBeNull();
    expect(value.state.libraryError).toBeNull();
    expect(value.state.libraryRegistry).toBeNull();
    expect(value.state.libraries).toEqual([]);
    expect(value.state.activeLibraryId).toBeNull();
    expect(value.state.stemMode).toBe("four_stem");
    expect(value.state.modelVariant).toBe("htdemucs_ft");
    expect(value.state.modelStatuses).toEqual({});
    expect(value.state.downloadingModel).toBeNull();
    expect(value.state.language).toBe("ja");
    expect(value.state.hideBatchSeparate).toBe(true);
    expect(value.state.coverArtBackdrop).toBe(true);
    expect(value.state.executionProvider).toBe("cpu");
    expect(value.state.availableExecutionProviders).toEqual(["cpu"]);

    expect(value.meta.isInitializing).toBe(true);
    expect(value.meta.dangerDialog).toBeNull();
    expect(value.meta.stemsSize).toBeNull();
    expect(value.meta.downgradeSavings).toBeNull();
    expect(value.meta.deletingStemsInProgress).toBe(false);
    expect(value.meta.deletingLyricsInProgress).toBe(false);
    expect(value.meta.downgradingInProgress).toBe(false);

    expect(typeof value.actions.initialize).toBe("function");
    expect(typeof value.actions.createLibrary).toBe("function");
    expect(typeof value.actions.closeDialog).toBe("function");
    expect(typeof value.actions.refreshModelStatuses).toBe("function");
  });

  test("with partial state override merges correctly", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        activeLibraryId: "lib-42",
        language: "fr",
        downloadingModel: "htdemucs_ft",
      },
    });

    expect(value.state.activeLibraryId).toBe("lib-42");
    expect(value.state.language).toBe("fr");
    expect(value.state.downloadingModel).toBe("htdemucs_ft");

    expect(value.state.libraryPath).toBeNull();
    expect(value.state.stemMode).toBe("four_stem");
    expect(value.state.hideBatchSeparate).toBe(true);
  });

  test("with partial meta override merges correctly", () => {
    const value = createSettingsOverlayTestContextValue({
      meta: {
        isInitializing: false,
        dangerDialog: "delete_stems",
        stemsSize: 1024,
      },
    });

    expect(value.meta.isInitializing).toBe(false);
    expect(value.meta.dangerDialog).toBe("delete_stems");
    expect(value.meta.stemsSize).toBe(1024);

    expect(value.meta.downgradeSavings).toBeNull();
    expect(value.meta.deletingStemsInProgress).toBe(false);
  });

  test("with partial actions override merges correctly", () => {
    const customInitialize = vi.fn<() => Promise<void>>();
    const customCloseDialog = vi.fn();

    const value = createSettingsOverlayTestContextValue(
      {},
      {
        initialize: customInitialize,
        closeDialog: customCloseDialog,
      },
    );

    expect(value.actions.initialize).toBe(customInitialize);
    expect(value.actions.closeDialog).toBe(customCloseDialog);

    // Unoverridden actions remain no-ops (not the same reference, but still functions)
    expect(typeof value.actions.createLibrary).toBe("function");
    expect(typeof value.actions.refreshModelStatuses).toBe("function");
    expect(value.actions.initialize).not.toBe(
      createSettingsOverlayTestContextValue().actions.initialize,
    );
  });
});

test("default action stubs are callable no-ops", async () => {
  const value = createSettingsOverlayTestContextValue();
  await expect(value.actions.setEqEnabled(true)).resolves.toBeUndefined();
  await expect(
    value.actions.setEqGains([0, 0, 0, 0, 0]),
  ).resolves.toBeUndefined();
  await expect(value.actions.resetEqGains()).resolves.toBeUndefined();
  await expect(
    value.actions.setThemePreference("dark"),
  ).resolves.toBeUndefined();
  await expect(
    value.actions.setThemePreference("light"),
  ).resolves.toBeUndefined();
  await expect(
    value.actions.setThemePreference("system"),
  ).resolves.toBeUndefined();
  await expect(value.actions.checkLibraryIntegrity()).resolves.toBeUndefined();
  expect(() => value.actions.toggleIntegritySelection("hash")).not.toThrow();
  await expect(
    value.actions.confirmIntegrityCleanup(),
  ).resolves.toBeUndefined();
  expect(() => value.actions.openIntegrityCleanupConfirmDialog()).not.toThrow();
  expect(() => value.actions.closeIntegrityReport()).not.toThrow();
});

describe("useSettingsOverlay", () => {
  test("throws when called outside provider", () => {
    mockReactUse.mockReturnValue(null);

    expect(() => useSettingsOverlay()).toThrow(
      "SettingsOverlay components must be used within the provider.",
    );
  });
});
