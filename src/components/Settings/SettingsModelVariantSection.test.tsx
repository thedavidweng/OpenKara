import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { SettingsModelVariantSection } from "./SettingsModelVariantSection";
import {
  SettingsOverlayContext,
  createSettingsOverlayTestContextValue,
} from "./SettingsOverlay.context";
import type {
  ModelUpdateView,
  ModelStatusView,
  RuntimeStatusView,
  SettingsOverlayState,
} from "./settings-overlay.types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, string | number>) =>
      vars ? `${key}:${Object.values(vars).join(",")}` : key,
  }),
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

const readyRuntime: RuntimeStatusView = {
  state: "ready",
  version: "1.27.1",
  runtime_path: "/tmp/runtime",
  active_artifact_id: "rt-1.27.1",
  target_triple: "aarch64-apple-darwin",
  candidate_version: null,
  restart_required: false,
  error: null,
};

const downloadedStatus: ModelStatusView = {
  downloaded: true,
  legacy_install_present: false,
  file_size_bytes: 355_000_000,
  installed_version: "model-v2.1.0",
  pinned_version: "model-v2.1.0",
};

function render(state: Partial<SettingsOverlayState>) {
  const value = createSettingsOverlayTestContextValue({
    state,
    meta: { isInitializing: false },
  });
  return renderToStaticMarkup(
    <SettingsOverlayContext.Provider value={value}>
      <SettingsModelVariantSection />
    </SettingsOverlayContext.Provider>,
  );
}

describe("SettingsModelVariantSection", () => {
  test("shows the installed model version next to the download state", () => {
    const html = render({
      runtimeStatus: readyRuntime,
      modelStatuses: { htdemucs: downloadedStatus },
    });

    expect(html).toContain("settings.modelVariant.downloaded");
    expect(html).toContain("model-v2.1.0");
  });

  test("shows file size next to the download state when present", () => {
    const html = render({
      runtimeStatus: readyRuntime,
      modelStatuses: { htdemucs: downloadedStatus },
    });

    expect(html).toContain("338.6 MB");
  });

  test("omits file size from the download state when null", () => {
    const html = render({
      runtimeStatus: readyRuntime,
      modelStatuses: {
        htdemucs: { ...downloadedStatus, file_size_bytes: null },
      },
    });

    expect(html).toContain("settings.modelVariant.downloaded");
  });

  test("shows legacy install label with file size when legacy model is on disk", () => {
    const html = render({
      runtimeStatus: readyRuntime,
      modelStatuses: {
        htdemucs: {
          downloaded: false,
          legacy_install_present: true,
          file_size_bytes: 200_000_000,
          installed_version: null,
          pinned_version: "model-v2.1.0",
        },
      },
    });

    expect(html).toContain("settings.modelVariant.legacyOnDisk");
    expect(html).toContain("190.7 MB");
  });

  test("shows legacy install label without file size when size is null", () => {
    const html = render({
      runtimeStatus: readyRuntime,
      modelStatuses: {
        htdemucs: {
          downloaded: false,
          legacy_install_present: true,
          file_size_bytes: null,
          installed_version: null,
          pinned_version: "model-v2.1.0",
        },
      },
    });

    expect(html).toContain("settings.modelVariant.legacyOnDisk");
  });

  test("explains why model downloads are unavailable without owning the runtime install", () => {
    const html = render({
      runtimeStatus: {
        ...readyRuntime,
        state: "corrupt",
        error: "checksum mismatch on libonnxruntime",
      },
    });

    expect(html).toContain("settings.runtime.corrupt");
    // Installing/repairing the runtime belongs to the ONNX Runtime card.
    expect(html).not.toContain("settings.runtime.retryButton");
    expect(html).not.toContain("settings.runtime.installButton");
  });

  test("keeps the variant picker and model-update button visible while the runtime is missing", () => {
    const html = render({
      runtimeStatus: { ...readyRuntime, state: "missing" },
    });

    // The old behavior replaced this whole card with a runtime install CTA,
    // which is why there was no way to check for model updates.
    expect(html).toContain("settings.modelVariant.htdemucs");
    expect(html).toContain("settings.modelVariant.htdemucsFt");
    expect(html).toContain("settings.modelUpdate.checkButton");
    expect(html).toContain("settings.modelVariant.runtimeRequired");
    expect(html).not.toContain("settings.runtime.installButton");
  });

  test("surfaces runtime download progress while it is being provisioned", () => {
    const html = render({
      runtimeStatus: { ...readyRuntime, state: "downloading" },
    });

    expect(html).toContain("settings.runtime.downloading");
    expect(html).toContain("settings.modelUpdate.checkButton");
  });

  test("disables the variant buttons until the runtime can load models", () => {
    const html = render({
      runtimeStatus: { ...readyRuntime, state: "missing" },
    });

    const disabledVariantButtons = (html.match(/<button disabled=""/g) ?? [])
      .length;
    expect(disabledVariantButtons).toBeGreaterThanOrEqual(2);
  });

  test("renders the update check button when the runtime is ready", () => {
    const html = render({ runtimeStatus: readyRuntime });

    expect(html).toContain("settings.modelUpdate.checkButton");
  });

  test("renders an update row with versions when an update is available", () => {
    const update: ModelUpdateView = {
      status: "checked",
      error: null,
      generation: 4,
      models: [
        {
          variant: "htdemucs",
          state: "update_available",
          installed_version: "model-v2.1.0",
          available_version: "model-v2.2.0",
          available_bytes: 360_000_000,
        },
      ],
    };
    const html = render({
      runtimeStatus: readyRuntime,
      modelStatuses: { htdemucs: downloadedStatus },
      modelUpdate: update,
    });

    expect(html).toContain("settings.modelUpdate.updateAvailable");
    expect(html).toContain("model-v2.1.0 → model-v2.2.0");
    expect(html).toContain("settings.modelUpdate.updateButton");
    expect(html).not.toContain("settings.modelUpdate.upToDate");
  });

  test("reports everything up to date after a clean check", () => {
    const html = render({
      runtimeStatus: readyRuntime,
      modelUpdate: {
        status: "checked",
        error: null,
        generation: 3,
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

  test("shows the update-check failure without hiding model controls", () => {
    const html = render({
      runtimeStatus: readyRuntime,
      modelStatuses: { htdemucs: downloadedStatus },
      modelUpdate: {
        status: "failed",
        error: "update check failed: offline",
        generation: null,
        models: [],
      },
    });

    expect(html).toContain("settings.modelUpdate.checkFailed");
    expect(html).toContain("update check failed: offline");
    expect(html).toContain("settings.modelVariant.htdemucs");
  });
});
