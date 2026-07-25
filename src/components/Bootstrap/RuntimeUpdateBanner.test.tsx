// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { RuntimeUpdateBanner } from "./RuntimeUpdateBanner";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import type {
  RuntimeBootstrapState,
  RuntimeBootstrapStatusSnapshot,
} from "@/types/ipc";

const { mockRestartApp, mockNotifyError } = vi.hoisted(() => ({
  mockRestartApp: vi.fn().mockResolvedValue(undefined),
  mockNotifyError: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

vi.mock("@/lib/tauri", () => ({ restartApp: mockRestartApp }));
vi.mock("@/lib/errors", () => ({ notifyError: mockNotifyError }));

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
});
