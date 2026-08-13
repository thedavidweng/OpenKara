// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { createSettingsHarness } from "@/test-utils/settings-controller";
import type {
  CommandError,
  RuntimeBootstrapStatusSnapshot,
  RuntimeUpdateReport,
} from "@/types/ipc";
import { SettingsControllerContext } from "./SettingsController.context";
import { SettingsRuntimeSection } from "./SettingsRuntimeSection";

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
    version: "v1.27.1",
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

function failedWith(message: string): CommandError {
  return {
    code: "model_unavailable",
    message,
    retryable: true,
    fallback: "retry",
  };
}

async function render(options: {
  status: RuntimeBootstrapStatusSnapshot;
  update?: RuntimeUpdateReport;
  updateError?: string;
}) {
  const harness = createSettingsHarness({
    runtimeStatus: options.status,
    overrides: {
      settings: {
        checkRuntimeUpdates: async () => {
          if (options.updateError) {
            throw new Error(options.updateError);
          }
          if (!options.update) {
            throw new Error("no update report");
          }
          return options.update;
        },
        getRuntimeBootstrapStatus: async () => options.status,
      },
    },
  });

  if (options.update || options.updateError) {
    await harness.controller.maintenance.checkRuntimeUpdates();
  }

  return renderToStaticMarkup(
    <SettingsControllerContext value={harness.controller}>
      <SettingsRuntimeSection />
    </SettingsControllerContext>,
  );
}

describe("SettingsRuntimeSection", () => {
  test("shows ready status with version and target triple", async () => {
    const html = await render({ status: runtimeStatus() });

    expect(html).toContain("settings.runtime.statusReady");
    expect(html).toContain("settings.runtime.version:v1.27.1");
    expect(html).toContain("aarch64-apple-darwin");
    expect(html).toContain("settings.runtime.checkButton");
  });

  test("renders the update-policy radio group", async () => {
    const html = await render({ status: runtimeStatus() });

    expect(html).toContain("settings.runtime.updatePolicy.label");
    expect(html).toContain("settings.runtime.updatePolicy.manual");
    expect(html).toContain("settings.runtime.updatePolicy.notify");
    expect(html).toContain("settings.runtime.updatePolicy.autoDownload");
    expect(html).toContain(
      "settings.runtime.updatePolicy.autoDownloadDescription",
    );
  });

  test("shows the restart CTA and candidate version when a candidate is staged", async () => {
    const html = await render({
      status: runtimeStatus({
        state: "candidate_ready_restart_required",
        candidate_version: "v1.28.0",
        restart_required: true,
      }),
    });

    expect(html).toContain(
      "settings.runtime.candidateReadyRestartRequired:v1.28.0",
    );
    expect(html).toContain("settings.runtime.restartButton");
  });

  test("shows the activation-failure copy and error text", async () => {
    const html = await render({
      status: runtimeStatus({
        state: "activation_failed_previous_restored",
        error: failedWith("dlopen failed on libonnxruntime"),
      }),
    });

    expect(html).toContain("settings.runtime.activationFailedPreviousRestored");
    expect(html).toContain("dlopen failed on libonnxruntime");
  });

  test("renders an update row with versions when an update is available", async () => {
    const html = await render({
      status: runtimeStatus(),
      update: {
        generation: 4,
        release_id: "2026-08-01-001",
        target_triple: "aarch64-apple-darwin",
        state: "update_available",
        installed_version: "v1.27.1",
        available_version: "v1.28.0",
        available_bytes: 42_000_000,
        restart_required: true,
      },
    });

    expect(html).toContain("settings.runtime.updateAvailable:v1.28.0");
    expect(html).toContain("v1.27.1 → v1.28.0");
    expect(html).toContain("settings.runtime.updateButton");
    expect(html).not.toContain("settings.runtime.upToDate");
  });

  test("reports up to date after a clean check", async () => {
    const html = await render({
      status: runtimeStatus(),
      update: {
        generation: 4,
        release_id: "2026-08-01-001",
        target_triple: "aarch64-apple-darwin",
        state: "up_to_date",
        installed_version: "v1.27.1",
        available_version: "v1.27.1",
        available_bytes: 0,
        restart_required: true,
      },
    });

    expect(html).toContain("settings.runtime.upToDate");
    expect(html).not.toContain("settings.runtime.updateButton");
  });

  test("shows the update-check failure without hiding the section", async () => {
    const html = await render({
      status: runtimeStatus(),
      updateError: "update check failed: offline",
    });

    expect(html).toContain("settings.runtime.checkFailed");
    expect(html).toContain("update check failed: offline");
    expect(html).toContain("settings.runtime.checkButton");
  });

  test("shows candidate download progress while downloading a candidate", async () => {
    const html = await render({
      status: runtimeStatus({ state: "downloading_candidate" }),
    });

    expect(html).toContain("settings.runtime.downloadingCandidate");
  });

  test.each([
    ["installing", "settings.runtime.banner.installingRuntime"],
    ["probing", "settings.runtime.banner.checkingCompatibility"],
    ["activating", "settings.runtime.banner.activatingRuntime"],
  ] as const)(
    "shows the %s post-download phase instead of claiming the runtime is ready",
    async (state, expectedKey) => {
      const html = await render({ status: runtimeStatus({ state }) });

      expect(html).toContain(expectedKey);
      expect(html).not.toContain("settings.runtime.statusReady");
      expect(html).not.toContain("settings.runtime.downloading");
    },
  );

  test("owns the install CTA when the runtime is missing", async () => {
    const html = await render({ status: runtimeStatus({ state: "missing" }) });

    expect(html).toContain("settings.runtime.statusMissing");
    expect(html).toContain("settings.runtime.installRequired");
    expect(html).toContain("settings.runtime.installButton");
    expect(html).toContain('data-testid="runtime-install-button"');
  });

  test("offers a repair CTA with the error text for a corrupt runtime", async () => {
    const html = await render({
      status: runtimeStatus({
        state: "corrupt",
        error: failedWith("checksum mismatch on libonnxruntime"),
      }),
    });

    expect(html).toContain("settings.runtime.corrupt");
    expect(html).toContain("settings.runtime.retryButton");
    expect(html).toContain("checksum mismatch on libonnxruntime");
    expect(html).not.toContain("settings.runtime.installRequired");
  });

  test("offers a retry CTA after a failed runtime download", async () => {
    const html = await render({
      status: runtimeStatus({ state: "failed", error: null }),
    });

    expect(html).toContain("settings.runtime.downloadFailed");
    expect(html).toContain("settings.runtime.retryButton");
  });

  test.each([
    ["download", "settings.runtime.downloadFailed"],
    ["install", "settings.runtime.installFailed"],
    ["probe", "settings.runtime.loadFailed"],
    ["activate", "settings.runtime.loadFailed"],
  ] as const)(
    "describes a failure in the %s phase without blaming the network for post-download failures",
    async (phase, expectedKey) => {
      const html = await render({
        status: runtimeStatus({
          state: "failed",
          error: failedWith("LoadLibraryExW failed for onnxruntime.dll"),
          failure_phase: phase,
        }),
      });

      expect(html).toContain(expectedKey);
      expect(html).toContain("LoadLibraryExW failed for onnxruntime.dll");
      expect(html).toContain("settings.runtime.retryButton");
      if (phase !== "download") {
        expect(html).not.toContain("settings.runtime.downloadFailed");
      }
    },
  );

  test("never claims the runtime is up to date when the check says it is not installed", async () => {
    const html = await render({
      status: runtimeStatus({ state: "missing" }),
      update: {
        generation: 4,
        release_id: "2026-08-01-001",
        target_triple: "aarch64-apple-darwin",
        state: "not_installed",
        installed_version: null,
        available_version: "v1.27.1",
        available_bytes: 42_000_000,
        restart_required: false,
      },
    });

    expect(html).not.toContain("settings.runtime.upToDate");
    expect(html).toContain("settings.runtime.statusMissing");
    expect(html).toContain("settings.runtime.installButton");
  });
});
