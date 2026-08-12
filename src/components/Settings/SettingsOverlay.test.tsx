// @vitest-environment jsdom

import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import type { SettingsController } from "@/lib/settings-controller";
import {
  createInitializedSettingsHarness,
  createSettingsHarness,
  type SettingsHarnessOptions,
} from "@/test-utils/settings-controller";
import type {
  ModelStatusSnapshot,
  RuntimeBootstrapStatusSnapshot,
} from "@/types/ipc";
import { SettingsControllerContext } from "./SettingsController.context";
import { SettingsDangerZoneSection } from "./SettingsDangerZoneSection";
import { SettingsDialogHost } from "./SettingsDialogHost";
import { SettingsExecutionProviderSection } from "./SettingsExecutionProviderSection";
import { SettingsGeneralSection } from "./SettingsGeneralSection";
import { SettingsLibrarySection } from "./SettingsLibrarySection";
import { SettingsModelVariantSection } from "./SettingsModelVariantSection";
import { SettingsOverlay } from "./SettingsOverlay";
import { SettingsStemModeSection } from "./SettingsStemModeSection";

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

const readyRuntime: RuntimeBootstrapStatusSnapshot = {
  state: "ready",
  version: "1.26.0",
  runtime_path: "/test/runtime",
  downloaded_bytes: null,
  total_bytes: null,
  active_artifact_id: "rt-1.26.0",
  target_triple: "aarch64-apple-darwin",
  candidate_version: null,
  restart_required: false,
  error: null,
};

const modelStatus = (
  patch: Partial<ModelStatusSnapshot> & { variant: string },
): ModelStatusSnapshot => ({
  downloaded: false,
  legacy_install_present: false,
  file_size_bytes: null,
  installed_version: null,
  pinned_version: "model-v2.1.0",
  ...patch,
});

function withModelStatuses(
  statuses: Record<string, Partial<ModelStatusSnapshot>>,
  extra: SettingsHarnessOptions = {},
): SettingsHarnessOptions {
  return {
    ...extra,
    overrides: {
      ...extra.overrides,
      settings: {
        getRuntimeBootstrapStatus: async () => readyRuntime,
        getModelStatus: async (variant) =>
          modelStatus({ variant, ...statuses[variant] }),
        ...extra.overrides?.settings,
      },
    },
  };
}

function markupOf(node: ReactElement, controller: SettingsController) {
  return renderToStaticMarkup(
    <SettingsControllerContext value={controller}>
      {node}
    </SettingsControllerContext>,
  );
}

describe("SettingsOverlay sections", () => {
  test("renders all primary sections", async () => {
    const harness = await createInitializedSettingsHarness({
      settings: {
        execution_provider: "xnnpack",
        available_execution_providers: ["cpu", "xnnpack"],
        compatible_execution_providers: ["cpu", "xnnpack"],
      },
    });

    const markup = markupOf(
      <>
        <SettingsLibrarySection />
        <SettingsStemModeSection />
        <SettingsModelVariantSection />
        <SettingsExecutionProviderSection />
        <SettingsGeneralSection />
        <SettingsDangerZoneSection />
      </>,
      harness.controller,
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
    const { default: controllerSource } =
      await import("@/lib/settings-controller/settings-controller.ts?raw");

    expect(source).not.toContain("window.prompt");
    expect(controllerSource).not.toContain("window.prompt");
  });

  test("renders downloaded, downloading, and not-downloaded model statuses", async () => {
    const harness = await createInitializedSettingsHarness(
      withModelStatuses({
        htdemucs: { downloaded: true, file_size_bytes: 1024 },
        htdemucs_ft: { downloaded: false },
      }),
    );
    harness.backend.settings.downloadModel = () => new Promise(() => {});
    void harness.controller.maintenance.downloadModel("htdemucs_ft");

    const markup = markupOf(
      <SettingsModelVariantSection />,
      harness.controller,
    );

    expect(markup).toContain("settings.modelVariant.downloaded");
    expect(markup).toContain("1.0 KB");
    expect(markup).toContain("settings.modelVariant.downloading");
  });

  test("model variant section shows legacy-on-disk label", async () => {
    const harness = await createInitializedSettingsHarness(
      withModelStatuses({
        htdemucs: { legacy_install_present: true, file_size_bytes: 2048 },
      }),
    );

    const markup = markupOf(
      <SettingsModelVariantSection />,
      harness.controller,
    );

    expect(markup).toContain("settings.modelVariant.legacyOnDisk");
  });

  test("danger zone shows model delete when legacy file exists without verified download", async () => {
    const harness = await createInitializedSettingsHarness(
      withModelStatuses({
        htdemucs: { legacy_install_present: true, file_size_bytes: 2048 },
      }),
    );

    const markup = markupOf(<SettingsDangerZoneSection />, harness.controller);

    expect(markup).toContain("settings.dangerZone.deleteModelStandard");
  });

  test("danger zone hides model deletion actions when models are not downloaded", async () => {
    const harness = await createInitializedSettingsHarness(
      withModelStatuses({}),
    );

    const markup = markupOf(<SettingsDangerZoneSection />, harness.controller);

    expect(markup).not.toContain("settings.dangerZone.deleteModelStandard");
    expect(markup).not.toContain("settings.dangerZone.deleteModelHQ");
  });

  test("danger zone shows delete without file size when downloaded but size is null", async () => {
    const harness = await createInitializedSettingsHarness(
      withModelStatuses({ htdemucs: { downloaded: true } }),
    );

    const markup = markupOf(<SettingsDangerZoneSection />, harness.controller);

    expect(markup).toContain("settings.dangerZone.deleteModelStandard");
  });

  test("dialog host renders the active dialog", async () => {
    const harness = createSettingsHarness();
    await harness.controller.preferences.selectModelVariant("htdemucs_ft");

    const markup = markupOf(<SettingsDialogHost />, harness.controller);

    expect(markup).toContain("settings.modelVariant.ftWarningTitle");
    expect(markup).toContain("settings.modelVariant.ftWarningMessage");
    expect(markup).toContain("settings.modelVariant.ftWarningConfirm");
  });

  test("renders integrity cleanup confirmation dialog with selected count", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => ({
            checked_local_songs: 3,
            skipped_remote_songs: 0,
            missing_primary_media: ["hash-a", "hash-b", "hash-c"].map(
              (song_hash) => ({
                song_hash,
                asset_type: "primary_media",
                path: `media/${song_hash}.mp3`,
              }),
            ),
            empty_primary_media: [],
            missing_optional_assets: [],
            empty_optional_assets: [],
            orphaned_managed_files: [],
          }),
        },
      },
    });
    await harness.controller.library.checkIntegrity();
    await harness.controller.maintenance.openDialog(
      "integrity_cleanup_confirm",
    );

    const markup = markupOf(<SettingsDialogHost />, harness.controller);

    expect(markup).toContain("settings.integrity.confirmCleanupTitle");
    expect(markup).toContain("settings.integrity.confirmCleanupMessage");
    expect(markup).toContain("settings.integrity.confirmCleanupButton");
  });

  test("settings overlay exposes a keyboard-operable modal dialog", () => {
    const markup = renderToStaticMarkup(<SettingsOverlay />);

    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('aria-modal="true"');
    expect(markup).toContain('aria-labelledby="settings-overlay-title"');
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
  test("non-Windows provider list renders exactly CPU and XNNPACK", async () => {
    const harness = await createInitializedSettingsHarness({
      settings: {
        execution_provider: "xnnpack",
        available_execution_providers: ["cpu", "xnnpack"],
        compatible_execution_providers: ["cpu", "xnnpack"],
      },
    });

    const markup = markupOf(
      <SettingsExecutionProviderSection />,
      harness.controller,
    );

    expect(markup).toContain("settings.executionProvider.cpu");
    expect(markup).toContain("settings.executionProvider.xnnpack");
    expect(markup).not.toContain("settings.executionProvider.directml");
    expect(markup).not.toContain("settings.executionProvider.coreml");
    expect(markup).not.toContain("settings.executionProvider.metal");
    expect(markup).not.toContain("settings.executionProvider.auto");
  });

  test("Windows provider list renders DirectML once and can mark it selected", async () => {
    const harness = await createInitializedSettingsHarness({
      settings: {
        execution_provider: "directml",
        available_execution_providers: ["cpu", "xnnpack", "directml"],
        compatible_execution_providers: ["cpu", "xnnpack"],
      },
    });

    const markup = markupOf(
      <SettingsExecutionProviderSection />,
      harness.controller,
    );

    expect(markup).toContain("settings.executionProvider.directml");
    expect(
      (markup.match(/settings\.executionProvider\.directml</g) ?? []).length,
    ).toBe(1);
    expect(markup).toContain("settings.executionProvider.cpu");
    expect(markup).toContain("settings.executionProvider.xnnpack");
    expect(markup).toContain("settings.executionProvider.incompatibleWarning");
    expect(markup).toContain('data-incompatible="true"');
  });

  test("safe fallback selection renders exactly one selected option from the list", async () => {
    const harness = await createInitializedSettingsHarness({
      settings: {
        execution_provider: "xnnpack",
        available_execution_providers: ["cpu", "xnnpack"],
        compatible_execution_providers: ["cpu", "xnnpack"],
      },
    });

    const markup = markupOf(
      <SettingsExecutionProviderSection />,
      harness.controller,
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
