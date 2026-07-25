import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { SettingsDangerZoneSection } from "./SettingsDangerZoneSection";
import { SettingsOverlay } from "./SettingsOverlay";
import { SettingsDialogHost } from "./SettingsDialogHost";
import { SettingsExecutionProviderSection } from "./SettingsExecutionProviderSection";
import { SettingsGeneralSection } from "./SettingsGeneralSection";
import { SettingsLibrarySection } from "./SettingsLibrarySection";
import { SettingsModelVariantSection } from "./SettingsModelVariantSection";
import {
  SettingsOverlayContext,
  createSettingsOverlayTestContextValue,
  type SettingsOverlayContextValue,
} from "./SettingsOverlay.context";
import { SettingsStemModeSection } from "./SettingsStemModeSection";

const { mockSettingsStore } = vi.hoisted(() => ({
  mockSettingsStore: {
    close: vi.fn(),
  },
}));

vi.mock("./SettingsOverlay.controller", async () => {
  const actual = await import("./SettingsOverlay.context");

  return {
    SettingsOverlayProvider: ({ children }: { children: React.ReactNode }) => (
      <actual.SettingsOverlayContext
        value={actual.createSettingsOverlayTestContextValue()}
      >
        {children}
      </actual.SettingsOverlayContext>
    ),
  };
});

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: Object.assign(
    (selector: (state: typeof mockSettingsStore) => unknown) =>
      selector(mockSettingsStore),
    {
      getState: () => ({
        getAppSettingsSnapshot: () => ({
          stemMode: "two_stem",
          modelVariant: "htdemucs",
          language: "en",
          hideBatchSeparate: false,
          lyricsFontStep: 0,
          executionProvider: "xnnpack" as const,
          availableExecutionProviders: ["cpu" as const, "xnnpack" as const],
          eqEnabled: false,
          eqGainsDb: [0, 0, 0, 0, 0],
          crossfadeEnabled: false,
          crossfadeDurationMs: 3_000,
        }),
      }),
    },
  ),
}));

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();

  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, params?: Record<string, string>) =>
        params?.size ? `${key}:${params.size}` : key,
      i18n: { changeLanguage: vi.fn() },
    }),
  };
});

vi.mock("./ConfirmationDialog", () => ({
  ConfirmationDialog: ({
    title,
    message,
    confirmLabel,
  }: {
    title: string;
    message: string;
    confirmLabel: string;
  }) => (
    <div data-testid="confirmation-dialog">
      <span>{title}</span>
      <span>{message}</span>
      <span>{confirmLabel}</span>
    </div>
  ),
}));

function renderWithSettingsContext(
  node: ReactElement,
  value: SettingsOverlayContextValue,
) {
  return renderToStaticMarkup(
    <SettingsOverlayContext value={value}>{node}</SettingsOverlayContext>,
  );
}

describe("SettingsOverlay sections", () => {
  test("renders all primary sections", () => {
    const value = createSettingsOverlayTestContextValue();

    const markup = renderWithSettingsContext(
      <>
        <SettingsLibrarySection />
        <SettingsStemModeSection />
        <SettingsModelVariantSection />
        <SettingsExecutionProviderSection />
        <SettingsGeneralSection />
        <SettingsDangerZoneSection />
      </>,
      value,
    );

    expect(markup).toContain("settings.library.label");
    expect(markup).toContain("settings.stemMode.label");
    expect(markup).toContain("settings.modelVariant.label");
    expect(markup).toContain("settings.executionProvider.cpu");
    expect(markup).toContain("settings.executionProvider.xnnpack");
    expect(markup).not.toContain("settings.executionProvider.coreml");
    expect(markup).not.toContain("settings.executionProvider.auto");
    expect(markup).toContain("settings.language.label");
    expect(markup).toContain("settings.dangerZone.label");
  });

  test("settings library section does not depend on browser prompt dialogs", async () => {
    const { default: source } =
      await import("./SettingsLibrarySection.tsx?raw");
    const { default: actionsSource } =
      await import("./settings-overlay.library-actions.ts?raw");

    expect(source).not.toContain("window.prompt");
    expect(actionsSource).not.toContain("window.prompt");
  });

  test("renders downloaded, downloading, and not-downloaded model statuses", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        runtimeStatus: {
          state: "ready",
          version: "1.26.0",
          runtime_path: "/test/runtime",
          active_artifact_id: "rt-1.26.0",
          target_triple: "aarch64-apple-darwin",
          candidate_version: null,
          restart_required: false,
          error: null,
        },
        modelStatuses: {
          htdemucs: {
            downloaded: true,
            legacy_install_present: false,
            file_size: 1024,
            installed_version: null,
            pinned_version: "model-v2.1.0",
          },
          htdemucs_ft: {
            downloaded: false,
            legacy_install_present: false,
            file_size: null,
            installed_version: null,
            pinned_version: "model-v2.1.0",
          },
        },
        downloadingModel: "htdemucs_ft",
      },
    });

    const markup = renderWithSettingsContext(
      <SettingsModelVariantSection />,
      value,
    );

    expect(markup).toContain("settings.modelVariant.downloaded");
    expect(markup).toContain("1.0 KB");
    expect(markup).toContain("settings.modelVariant.downloading");
  });

  test("model variant section shows legacy-on-disk label", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        runtimeStatus: {
          state: "ready",
          version: "1.26.0",
          runtime_path: "/test/runtime",
          active_artifact_id: "rt-1.26.0",
          target_triple: "aarch64-apple-darwin",
          candidate_version: null,
          restart_required: false,
          error: null,
        },
        modelStatuses: {
          htdemucs: {
            downloaded: false,
            legacy_install_present: true,
            file_size: 2048,
            installed_version: null,
            pinned_version: "model-v2.1.0",
          },
          htdemucs_ft: {
            downloaded: false,
            legacy_install_present: false,
            file_size: null,
            installed_version: null,
            pinned_version: "model-v2.1.0",
          },
        },
      },
    });

    const markup = renderWithSettingsContext(
      <SettingsModelVariantSection />,
      value,
    );

    expect(markup).toContain("settings.modelVariant.legacyOnDisk");
  });

  test("danger zone shows model delete when legacy file exists without verified download", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        modelVariant: "htdemucs",
        modelStatuses: {
          htdemucs: {
            downloaded: false,
            legacy_install_present: true,
            file_size: 2048,
            installed_version: null,
            pinned_version: "model-v2.1.0",
          },
          htdemucs_ft: {
            downloaded: false,
            legacy_install_present: false,
            file_size: null,
            installed_version: null,
            pinned_version: "model-v2.1.0",
          },
        },
      },
    });

    const markup = renderWithSettingsContext(
      <SettingsDangerZoneSection />,
      value,
    );

    expect(markup).toContain("settings.dangerZone.deleteModelStandard");
  });

  test("danger zone hides model deletion actions when models are not downloaded", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        modelStatuses: {
          htdemucs: {
            downloaded: false,
            legacy_install_present: false,
            file_size: null,
            installed_version: null,
            pinned_version: "model-v2.1.0",
          },
          htdemucs_ft: {
            downloaded: false,
            legacy_install_present: false,
            file_size: null,
            installed_version: null,
            pinned_version: "model-v2.1.0",
          },
        },
      },
    });

    const markup = renderWithSettingsContext(
      <SettingsDangerZoneSection />,
      value,
    );

    expect(markup).not.toContain("settings.dangerZone.deleteModelStandard");
    expect(markup).not.toContain("settings.dangerZone.deleteModelHQ");
  });

  test("dialog host renders the active dialog", () => {
    const value = createSettingsOverlayTestContextValue({
      meta: {
        dangerDialog: "ft_warning",
      },
    });

    const markup = renderWithSettingsContext(<SettingsDialogHost />, value);

    expect(markup).toContain("settings.modelVariant.ftWarningTitle");
    expect(markup).toContain("settings.modelVariant.ftWarningMessage");
    expect(markup).toContain("settings.modelVariant.ftWarningConfirm");
  });

  test("renders integrity cleanup confirmation dialog with selected count", () => {
    const value = createSettingsOverlayTestContextValue({
      state: { integritySelection: new Set(["hash-a", "hash-b", "hash-c"]) },
      meta: { dangerDialog: "integrity_cleanup_confirm" },
    });

    const markup = renderWithSettingsContext(<SettingsDialogHost />, value);

    expect(markup).toContain("settings.integrity.confirmCleanupTitle");
    expect(markup).toContain("settings.integrity.confirmCleanupMessage");
    expect(markup).toContain("settings.integrity.confirmCleanupButton");
  });

  test("settings overlay renders a close control for mouse users", () => {
    const markup = renderToStaticMarkup(<SettingsOverlay />);

    expect(markup).toContain('aria-label="common.close"');
  });

  test("settings overlay captures pointer events across the entire backdrop", () => {
    const markup = renderToStaticMarkup(<SettingsOverlay />);

    expect(markup).toContain("pointer-events-auto");
    expect(markup).not.toContain("pointer-events-none");
  });

  test("uses semantic foreground tokens instead of white text on light surfaces", () => {
    const markup = renderToStaticMarkup(<SettingsOverlay />);

    expect(markup).toContain("text-[var(--color-text)]");
    expect(markup).toContain("bg-[var(--color-surface)]");
    expect(markup).not.toContain("text-white");
    expect(markup).not.toContain("hover:text-white");
  });
});

describe("SettingsExecutionProviderSection rendering", () => {
  test("non-Windows provider list renders exactly CPU and XNNPACK", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        executionProvider: "xnnpack",
        availableExecutionProviders: ["cpu", "xnnpack"],
      },
    });

    const markup = renderWithSettingsContext(
      <SettingsExecutionProviderSection />,
      value,
    );

    expect(markup).toContain("settings.executionProvider.cpu");
    expect(markup).toContain("settings.executionProvider.xnnpack");
    // Never render foreign/unsupported providers.
    expect(markup).not.toContain("settings.executionProvider.directml");
    expect(markup).not.toContain("settings.executionProvider.coreml");
    expect(markup).not.toContain("settings.executionProvider.metal");
    expect(markup).not.toContain("settings.executionProvider.auto");
  });

  test("Windows provider list renders DirectML once and can mark it selected", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        executionProvider: "directml",
        availableExecutionProviders: ["cpu", "xnnpack", "directml"],
      },
    });

    const markup = renderWithSettingsContext(
      <SettingsExecutionProviderSection />,
      value,
    );

    // DirectML title appears exactly once (one option button). The
    // description key shares the prefix, so match the title followed by `<`.
    expect(markup).toContain("settings.executionProvider.directml");
    expect(
      (markup.match(/settings\.executionProvider\.directml</g) ?? []).length,
    ).toBe(1);
    expect(markup).toContain("settings.executionProvider.cpu");
    expect(markup).toContain("settings.executionProvider.xnnpack");
  });

  test("safe fallback selection renders exactly one selected option from the list", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        // Stale directml normalized to xnnpack by the backend.
        executionProvider: "xnnpack",
        availableExecutionProviders: ["cpu", "xnnpack"],
      },
    });

    const markup = renderWithSettingsContext(
      <SettingsExecutionProviderSection />,
      value,
    );

    const selectedCount = (
      markup.match(
        /border-\[var\(--color-accent\)\] bg-\[var\(--color-accent\)\]\/15/g,
      ) ?? []
    ).length;
    expect(selectedCount).toBe(1);
    expect(markup).toContain("settings.executionProvider.xnnpack");
    expect(markup).not.toContain("settings.executionProvider.directml");
  });
});
