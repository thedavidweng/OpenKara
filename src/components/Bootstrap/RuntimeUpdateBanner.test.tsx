// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { RuntimeUpdateBanner } from "./RuntimeUpdateBanner";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import type {
  RuntimeBootstrapState,
  RuntimeBootstrapStatusSnapshot,
} from "@/types/ipc";

const {
  mockRestartApp,
  mockDownloadRuntime,
  mockNotifyError,
  mockGetErrorMessage,
} = vi.hoisted(() => ({
  mockRestartApp: vi.fn().mockResolvedValue(undefined),
  mockDownloadRuntime: vi.fn(),
  mockNotifyError: vi.fn(),
  mockGetErrorMessage: vi.fn(
    (error: { message?: string }) => error.message ?? "",
  ),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

vi.mock("@/lib/tauri", () => ({
  restartApp: mockRestartApp,
  downloadRuntime: mockDownloadRuntime,
}));
vi.mock("@/lib/errors", () => ({
  getErrorMessage: mockGetErrorMessage,
  notifyError: mockNotifyError,
}));

function setStatus(
  state: RuntimeBootstrapState,
  overrides: Partial<RuntimeBootstrapStatusSnapshot> = {},
) {
  const status: RuntimeBootstrapStatusSnapshot = {
    state,
    runtime_path: "/tmp/runtime",
    downloaded_bytes: null,
    total_bytes: null,
    version: "v1.27.1",
    active_artifact_id: "rt-1.27.1",
    target_triple: "aarch64-apple-darwin",
    candidate_version: null,
    restart_required: false,
    error: null,
    ...overrides,
  };
  useRuntimeBootstrapStore.setState({ status });
}

describe("RuntimeUpdateBanner", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useRuntimeBootstrapStore.setState({ status: null });
  });

  afterEach(cleanup);

  test("renders nothing when there is no runtime status", () => {
    const { container } = render(<RuntimeUpdateBanner />);
    expect(container.firstChild).toBeNull();
  });

  test("renders nothing for a ready runtime", () => {
    setStatus("ready");
    const { container } = render(<RuntimeUpdateBanner />);
    expect(container.firstChild).toBeNull();
  });

  test("shows the restart CTA and calls restartApp when a candidate is staged", () => {
    setStatus("candidate_ready_restart_required", {
      restart_required: true,
      candidate_version: "v1.28.0",
    });
    render(<RuntimeUpdateBanner />);

    expect(
      screen.getByText("settings.runtime.banner.updateReady"),
    ).toBeTruthy();
    fireEvent.click(screen.getByText("settings.runtime.restartButton"));
    expect(mockRestartApp).toHaveBeenCalledOnce();
  });

  test("shows a dismissible warning after an activation failure", () => {
    setStatus("activation_failed_previous_restored");
    const { container } = render(<RuntimeUpdateBanner />);

    expect(
      screen.getByText("settings.runtime.banner.activationFailed"),
    ).toBeTruthy();

    fireEvent.click(screen.getByLabelText("common.close"));
    expect(container.firstChild).toBeNull();
  });

  test("missing state shows the runtime-required banner with a hint", () => {
    setStatus("missing", { active_artifact_id: null });
    render(<RuntimeUpdateBanner />);

    expect(
      screen.getByText("settings.runtime.banner.runtimeRequired"),
    ).toBeTruthy();
    expect(
      screen.getByText("settings.runtime.banner.runtimeRequiredHint"),
    ).toBeTruthy();
  });

  test("downloading state shows the downloading-runtime banner", () => {
    setStatus("downloading", {
      active_artifact_id: null,
      downloaded_bytes: 1_000_000,
      total_bytes: 6_400_000,
    });
    render(<RuntimeUpdateBanner />);

    expect(
      screen.getByText("settings.runtime.banner.downloadingRuntime"),
    ).toBeTruthy();
    expect(screen.getByRole("status").getAttribute("aria-live")).toBe("polite");
  });

  test.each([
    ["installing", "settings.runtime.banner.installingRuntime"],
    ["probing", "settings.runtime.banner.checkingCompatibility"],
    ["activating", "settings.runtime.banner.activatingRuntime"],
  ] as const)("shows the runtime %s phase", (state, message) => {
    setStatus(state);
    render(<RuntimeUpdateBanner />);

    expect(screen.getByText(message)).toBeTruthy();
    expect(screen.getByRole("status").getAttribute("aria-live")).toBe("polite");
  });

  test("failed state renders the error, a hint, and a Retry that triggers the runtime download", async () => {
    mockDownloadRuntime.mockResolvedValue({
      state: "downloading",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: 0,
      total_bytes: null,
      version: "v1.27.1",
      active_artifact_id: null,
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
      error: null,
    });
    setStatus("failed", {
      active_artifact_id: null,
      error: {
        code: "network_unavailable",
        message: "network unreachable",
        retryable: true,
        fallback: "retry",
      },
    });
    render(<RuntimeUpdateBanner />);

    expect(
      screen.getByText("settings.runtime.banner.downloadFailed"),
    ).toBeTruthy();
    expect(
      screen.getByText("settings.runtime.banner.downloadFailedHint"),
    ).toBeTruthy();
    expect(screen.getByRole("alert").getAttribute("aria-live")).toBe(
      "assertive",
    );

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.runtime.banner.retryDownload",
      }),
    );

    expect(mockDownloadRuntime).toHaveBeenCalledOnce();
    // Publishing the returned `downloading` snapshot flips the banner so
    // GlobalProgressBar can take over the byte/percent readout (mirrors #217).
    await waitFor(() => {
      expect(useRuntimeBootstrapStore.getState().status?.state).toBe(
        "downloading",
      );
    });
  });
});
