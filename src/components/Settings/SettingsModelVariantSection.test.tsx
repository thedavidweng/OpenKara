// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { createSettingsHarness } from "@/test-utils/settings-controller";
import type {
  CommandError,
  ModelStatusSnapshot,
  ModelUpdateReport,
  ModelVariant,
  RuntimeBootstrapStatusSnapshot,
} from "@/types/ipc";
import { SettingsControllerContext } from "./SettingsController.context";
import { SettingsModelVariantSection } from "./SettingsModelVariantSection";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, string | number>) =>
      vars ? `${key}:${Object.values(vars).join(",")}` : key,
  }),
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

function runtimeStatus(
  patch: Partial<RuntimeBootstrapStatusSnapshot> = {},
): RuntimeBootstrapStatusSnapshot {
  return {
    state: "ready",
    version: "1.27.1",
    runtime_path: "/tmp/runtime",
    downloaded_bytes: null,
    total_bytes: null,
    active_artifact_id: "rt-1.27.1",
    target_triple: "aarch64-apple-darwin",
    candidate_version: null,
    restart_required: false,
    error: null,
    ...patch,
  };
}

const failedWith = (message: string): CommandError => ({
  code: "model_unavailable",
  message,
  retryable: true,
  fallback: "retry",
});

const missingStatus = (variant: string): ModelStatusSnapshot => ({
  variant,
  downloaded: false,
  legacy_install_present: false,
  file_size_bytes: null,
  installed_version: null,
  pinned_version: "model-v2.1.0",
});

const downloadedStatus = (variant: string): ModelStatusSnapshot => ({
  ...missingStatus(variant),
  downloaded: true,
  file_size_bytes: 355_000_000,
  installed_version: "model-v2.1.0",
});

async function render(options: {
  status?: RuntimeBootstrapStatusSnapshot;
  statuses?: Partial<Record<ModelVariant, ModelStatusSnapshot>>;
  statusesError?: string;
  update?: ModelUpdateReport;
  updateError?: string;
}) {
  const status = options.status ?? runtimeStatus();
  const harness = createSettingsHarness({
    overrides: {
      settings: {
        getRuntimeBootstrapStatus: async () => status,
        getModelStatus: async (variant) => {
          if (options.statusesError) {
            throw new Error(options.statusesError);
          }
          return (
            options.statuses?.[variant as ModelVariant] ??
            missingStatus(variant)
          );
        },
        checkModelUpdates: async () => {
          if (options.updateError) {
            throw new Error(options.updateError);
          }
          if (!options.update) {
            throw new Error("no update report");
          }
          return options.update;
        },
      },
    },
  });

  await harness.controller.initialize();
  if (options.update || options.updateError) {
    await harness.controller.maintenance.checkModelUpdates();
  }

  return renderToStaticMarkup(
    <SettingsControllerContext value={harness.controller}>
      <SettingsModelVariantSection />
    </SettingsControllerContext>,
  );
}

describe("SettingsModelVariantSection", () => {
  test("shows the installed model version next to the download state", async () => {
    const html = await render({
      statuses: { htdemucs: downloadedStatus("htdemucs") },
    });

    expect(html).toContain("settings.modelVariant.downloaded");
    expect(html).toContain("model-v2.1.0");
  });

  test("shows file size next to the download state when present", async () => {
    const html = await render({
      statuses: { htdemucs: downloadedStatus("htdemucs") },
    });

    expect(html).toContain("338.6 MB");
  });

  test("omits file size from the download state when null", async () => {
    const html = await render({
      statuses: {
        htdemucs: { ...downloadedStatus("htdemucs"), file_size_bytes: null },
      },
    });

    expect(html).toContain("settings.modelVariant.downloaded");
  });

  test("shows legacy install label with file size when legacy model is on disk", async () => {
    const html = await render({
      statuses: {
        htdemucs: {
          ...missingStatus("htdemucs"),
          legacy_install_present: true,
          file_size_bytes: 200_000_000,
        },
      },
    });

    expect(html).toContain("settings.modelVariant.legacyOnDisk");
    expect(html).toContain("190.7 MB");
  });

  test("shows legacy install label without file size when size is null", async () => {
    const html = await render({
      statuses: {
        htdemucs: {
          ...missingStatus("htdemucs"),
          legacy_install_present: true,
        },
      },
    });

    expect(html).toContain("settings.modelVariant.legacyOnDisk");
  });

  test("explains why model downloads are unavailable without owning the runtime install", async () => {
    const html = await render({
      status: runtimeStatus({
        state: "corrupt",
        error: failedWith("checksum mismatch on libonnxruntime"),
      }),
    });

    expect(html).toContain("settings.runtime.corrupt");
    expect(html).not.toContain("settings.runtime.retryButton");
    expect(html).not.toContain("settings.runtime.installButton");
  });

  test("describes a runtime load failure without blaming the network", async () => {
    const html = await render({
      status: runtimeStatus({
        state: "failed",
        error: failedWith("LoadLibraryExW failed for onnxruntime.dll"),
        failure_phase: "probe",
      }),
    });

    expect(html).toContain("settings.runtime.loadFailed");
    expect(html).not.toContain("settings.runtime.downloadFailed");
  });

  test("keeps the variant picker and model-update button visible while the runtime is missing", async () => {
    const html = await render({ status: runtimeStatus({ state: "missing" }) });

    expect(html).toContain("settings.modelVariant.htdemucs");
    expect(html).toContain("settings.modelVariant.htdemucsFt");
    expect(html).toContain("settings.modelUpdate.checkButton");
    expect(html).toContain("settings.modelVariant.runtimeRequired");
    expect(html).not.toContain("settings.runtime.installButton");
  });

  test("surfaces runtime download progress while it is being provisioned", async () => {
    const html = await render({
      status: runtimeStatus({ state: "downloading" }),
    });

    expect(html).toContain("settings.runtime.downloading");
    expect(html).toContain("settings.modelUpdate.checkButton");
  });

  test("disables the variant buttons until the runtime can load models", async () => {
    const html = await render({ status: runtimeStatus({ state: "missing" }) });

    expect(
      (html.match(/<button disabled=""/g) ?? []).length,
    ).toBeGreaterThanOrEqual(2);
  });

  test("renders the update check button when the runtime is ready", async () => {
    const html = await render({});

    expect(html).toContain("settings.modelUpdate.checkButton");
  });

  test("renders an update row with versions when an update is available", async () => {
    const html = await render({
      statuses: { htdemucs: downloadedStatus("htdemucs") },
      update: {
        generation: 4,
        release_id: "2026-08-01-001",
        models: [
          {
            variant: "htdemucs",
            state: "update_available",
            installed_version: "model-v2.1.0",
            available_version: "model-v2.2.0",
            available_bytes: 360_000_000,
          },
        ],
      },
    });

    expect(html).toContain("settings.modelUpdate.updateAvailable");
    expect(html).toContain("model-v2.1.0 → model-v2.2.0");
    expect(html).toContain("settings.modelUpdate.updateButton");
    expect(html).not.toContain("settings.modelUpdate.upToDate");
  });

  test("reports everything up to date after a clean check", async () => {
    const html = await render({
      update: {
        generation: 3,
        release_id: "2026-08-01-001",
        models: [
          {
            variant: "htdemucs",
            state: "up_to_date",
            installed_version: "model-v2.1.0",
            available_version: "model-v2.1.0",
            available_bytes: 355_000_000,
          },
        ],
      },
    });

    expect(html).toContain("settings.modelUpdate.upToDate");
    expect(html).not.toContain("settings.modelUpdate.updateButton");
  });

  test("shows the update-check failure without hiding model controls", async () => {
    const html = await render({
      statuses: { htdemucs: downloadedStatus("htdemucs") },
      updateError: "update check failed: offline",
    });

    expect(html).toContain("settings.modelUpdate.checkFailed");
    expect(html).toContain("update check failed: offline");
    expect(html).toContain("settings.modelVariant.htdemucs");
  });

  test("shows the status-read failure instead of stale model statuses", async () => {
    const html = await render({
      statusesError: "model directory unreadable",
    });

    expect(html).toContain("settings.modelVariant.statusUnavailable");
    expect(html).toContain("settings.modelVariant.statusReadFailed");
    expect(html).toContain("model directory unreadable");
    expect(html).not.toContain("settings.modelVariant.notDownloaded");
  });
});
