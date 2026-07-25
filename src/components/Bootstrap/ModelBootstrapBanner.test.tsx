// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { ModelBootstrapBanner } from "./ModelBootstrapBanner";
import { useBootstrapStore } from "@/stores/bootstrap-store";
import { useSettingsStore } from "@/stores/settings-store";
import type { ModelBootstrapStatusSnapshot } from "@/types/ipc";

const { mockDownloadModel, mockNotifyError } = vi.hoisted(() => ({
  mockDownloadModel: vi.fn(),
  mockNotifyError: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

vi.mock("@/lib/tauri", () => ({ downloadModel: mockDownloadModel }));
vi.mock("@/lib/errors", () => ({ notifyError: mockNotifyError }));

function setStatus(
  state: ModelBootstrapStatusSnapshot["state"],
  overrides: Partial<ModelBootstrapStatusSnapshot> = {},
) {
  const status: ModelBootstrapStatusSnapshot = {
    state,
    model_path: "/tmp/model",
    downloaded_bytes: null,
    total_bytes: null,
    error: null,
    ...overrides,
  };
  useBootstrapStore.setState({ status });
}

describe("ModelBootstrapBanner", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useBootstrapStore.setState({ status: null });
    useSettingsStore.setState({ modelVariant: "htdemucs" });
  });

  afterEach(cleanup);

  test("renders nothing when there is no model status", () => {
    const { container } = render(<ModelBootstrapBanner />);
    expect(container.firstChild).toBeNull();
  });

  test("renders nothing for a ready model", () => {
    setStatus("ready");
    const { container } = render(<ModelBootstrapBanner />);
    expect(container.firstChild).toBeNull();
  });

  test("failed state renders the error, an actionable hint, and a Retry control", () => {
    setStatus("failed", {
      error: {
        code: "network_unavailable",
        message: "network unreachable",
        retryable: true,
        fallback: "retry",
      },
    });
    render(<ModelBootstrapBanner />);

    expect(screen.getByText("bootstrap.downloadFailed")).toBeTruthy();
    // Copy no longer asserts "separation unavailable" when a retry path exists.
    expect(screen.queryByText("bootstrap.separationUnavailable")).toBeNull();
    expect(screen.getByText("bootstrap.downloadFailedHint")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "bootstrap.retryDownload" }),
    ).toBeTruthy();
  });

  test("Retry invokes the model-download command for the active variant", () => {
    mockDownloadModel.mockResolvedValue({
      state: "downloading",
      model_path: "/tmp/model",
      downloaded_bytes: 0,
      total_bytes: null,
      error: null,
    });
    useSettingsStore.setState({ modelVariant: "htdemucs_ft" });
    setStatus("failed");
    render(<ModelBootstrapBanner />);

    fireEvent.click(
      screen.getByRole("button", { name: "bootstrap.retryDownload" }),
    );

    expect(mockDownloadModel).toHaveBeenCalledExactlyOnceWith("htdemucs_ft");
  });

  test("after Retry the banner transitions to the downloading state", async () => {
    mockDownloadModel.mockResolvedValue({
      state: "downloading",
      model_path: "/tmp/model",
      downloaded_bytes: 0,
      total_bytes: 209_000_000,
      error: null,
    });
    setStatus("failed");
    render(<ModelBootstrapBanner />);

    fireEvent.click(
      screen.getByRole("button", { name: "bootstrap.retryDownload" }),
    );

    await waitFor(() => {
      expect(useBootstrapStore.getState().status?.state).toBe("downloading");
    });
    expect(screen.getByText("bootstrap.downloadingModel")).toBeTruthy();
  });

  test("surfaces a download failure without leaving the failed banner", async () => {
    mockDownloadModel.mockRejectedValue(new Error("still offline"));
    setStatus("failed");
    render(<ModelBootstrapBanner />);

    fireEvent.click(
      screen.getByRole("button", { name: "bootstrap.retryDownload" }),
    );

    await waitFor(() => {
      expect(mockNotifyError).toHaveBeenCalledOnce();
    });
    expect(useBootstrapStore.getState().status?.state).toBe("failed");
  });
});
