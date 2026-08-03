import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { SettingsRuntimeSection } from "./SettingsRuntimeSection";
import {
  SettingsOverlayContext,
  createSettingsOverlayTestContextValue,
} from "./SettingsOverlay.context";
import type {
  RuntimeStatusView,
  RuntimeUpdateView,
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
  version: "v1.27.1",
  runtime_path: "/tmp/runtime",
  active_artifact_id: "rt-1.27.1",
  target_triple: "aarch64-apple-darwin",
  candidate_version: null,
  restart_required: false,
  error: null,
};

function render(state: Partial<SettingsOverlayState>) {
  const value = createSettingsOverlayTestContextValue({
    state,
    meta: { isInitializing: false },
  });
  return renderToStaticMarkup(
    <SettingsOverlayContext.Provider value={value}>
      <SettingsRuntimeSection />
    </SettingsOverlayContext.Provider>,
  );
}

describe("SettingsRuntimeSection", () => {
  test("shows ready status with version and target triple", () => {
    const html = render({ runtimeStatus: readyRuntime });

    expect(html).toContain("settings.runtime.statusReady");
    expect(html).toContain("settings.runtime.version:v1.27.1");
    expect(html).toContain("aarch64-apple-darwin");
    expect(html).toContain("settings.runtime.checkButton");
  });

  test("renders the update-policy radio group", () => {
    const html = render({ runtimeStatus: readyRuntime });

    expect(html).toContain("settings.runtime.updatePolicy.label");
    expect(html).toContain("settings.runtime.updatePolicy.manual");
    expect(html).toContain("settings.runtime.updatePolicy.notify");
    expect(html).toContain("settings.runtime.updatePolicy.autoDownload");
    expect(html).toContain(
      "settings.runtime.updatePolicy.autoDownloadDescription",
    );
  });

  test("shows the restart CTA and candidate version when a candidate is staged", () => {
    const html = render({
      runtimeStatus: {
        ...readyRuntime,
        state: "candidate_ready_restart_required",
        candidate_version: "v1.28.0",
        restart_required: true,
      },
    });

    expect(html).toContain(
      "settings.runtime.candidateReadyRestartRequired:v1.28.0",
    );
    expect(html).toContain("settings.runtime.restartButton");
  });

  test("shows the activation-failure copy and error text", () => {
    const html = render({
      runtimeStatus: {
        ...readyRuntime,
        state: "activation_failed_previous_restored",
        error: "dlopen failed on libonnxruntime",
      },
    });

    expect(html).toContain("settings.runtime.activationFailedPreviousRestored");
    expect(html).toContain("dlopen failed on libonnxruntime");
  });

  test("renders an update row with versions when an update is available", () => {
    const update: RuntimeUpdateView = {
      status: "checked",
      error: null,
      report: {
        generation: 4,
        release_id: "2026-08-01-001",
        target_triple: "aarch64-apple-darwin",
        state: "update_available",
        installed_version: "v1.27.1",
        available_version: "v1.28.0",
        available_bytes: 42_000_000,
        restart_required: true,
      },
    };
    const html = render({ runtimeStatus: readyRuntime, runtimeUpdate: update });

    expect(html).toContain("settings.runtime.updateAvailable:v1.28.0");
    expect(html).toContain("v1.27.1 → v1.28.0");
    expect(html).toContain("settings.runtime.updateButton");
    expect(html).not.toContain("settings.runtime.upToDate");
  });

  test("reports up to date after a clean check", () => {
    const update: RuntimeUpdateView = {
      status: "checked",
      error: null,
      report: {
        generation: 4,
        release_id: "2026-08-01-001",
        target_triple: "aarch64-apple-darwin",
        state: "up_to_date",
        installed_version: "v1.27.1",
        available_version: "v1.27.1",
        available_bytes: 0,
        restart_required: true,
      },
    };
    const html = render({ runtimeStatus: readyRuntime, runtimeUpdate: update });

    expect(html).toContain("settings.runtime.upToDate");
    expect(html).not.toContain("settings.runtime.updateButton");
  });

  test("shows the update-check failure without hiding the section", () => {
    const html = render({
      runtimeStatus: readyRuntime,
      runtimeUpdate: {
        status: "failed",
        error: "update check failed: offline",
        report: null,
      },
    });

    expect(html).toContain("settings.runtime.checkFailed");
    expect(html).toContain("update check failed: offline");
    expect(html).toContain("settings.runtime.checkButton");
  });

  test("shows candidate download progress while downloading a candidate", () => {
    const html = render({
      runtimeStatus: { ...readyRuntime, state: "downloading_candidate" },
    });

    expect(html).toContain("settings.runtime.downloadingCandidate");
  });

  test.each([
    ["installing", "settings.runtime.banner.installingRuntime"],
    ["probing", "settings.runtime.banner.checkingCompatibility"],
    ["activating", "settings.runtime.banner.activatingRuntime"],
  ] as const)(
    "shows the %s post-download phase instead of claiming the runtime is ready",
    (runtimeState, expectedKey) => {
      const html = render({
        runtimeStatus: { ...readyRuntime, state: runtimeState },
      });

      expect(html).toContain(expectedKey);
      expect(html).not.toContain("settings.runtime.statusReady");
      expect(html).not.toContain("settings.runtime.downloading");
    },
  );

  test("owns the install CTA when the runtime is missing", () => {
    const html = render({
      runtimeStatus: { ...readyRuntime, state: "missing" },
    });

    expect(html).toContain("settings.runtime.statusMissing");
    expect(html).toContain("settings.runtime.installRequired");
    expect(html).toContain("settings.runtime.installButton");
    expect(html).toContain('data-testid="runtime-install-button"');
  });

  test("offers a repair CTA with the error text for a corrupt runtime", () => {
    const html = render({
      runtimeStatus: {
        ...readyRuntime,
        state: "corrupt",
        error: "checksum mismatch on libonnxruntime",
      },
    });

    expect(html).toContain("settings.runtime.corrupt");
    expect(html).toContain("settings.runtime.retryButton");
    expect(html).toContain("checksum mismatch on libonnxruntime");
    expect(html).not.toContain("settings.runtime.installRequired");
  });

  test("offers a retry CTA after a failed runtime download", () => {
    const html = render({
      runtimeStatus: { ...readyRuntime, state: "failed", error: null },
    });

    expect(html).toContain("settings.runtime.downloadFailed");
    expect(html).toContain("settings.runtime.retryButton");
  });

  test("never claims the runtime is up to date when the check says it is not installed", () => {
    const update: RuntimeUpdateView = {
      status: "checked",
      error: null,
      report: {
        generation: 4,
        release_id: "2026-08-01-001",
        target_triple: "aarch64-apple-darwin",
        state: "not_installed",
        installed_version: null,
        available_version: "v1.27.1",
        available_bytes: 42_000_000,
        restart_required: false,
      },
    };
    const html = render({
      runtimeStatus: { ...readyRuntime, state: "missing" },
      runtimeUpdate: update,
    });

    expect(html).not.toContain("settings.runtime.upToDate");
    expect(html).toContain("settings.runtime.statusMissing");
    expect(html).toContain("settings.runtime.installButton");
  });
});
