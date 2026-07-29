// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { SettingsRemoteDiagnosticsSection } from "./SettingsRemoteDiagnosticsSection";

const { mockGetRemoteDiagnostics, mockNotifyError, mockResolveRemoteConflict } =
  vi.hoisted(() => ({
    mockGetRemoteDiagnostics: vi.fn(),
    mockNotifyError: vi.fn(),
    mockResolveRemoteConflict: vi.fn(),
  }));

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (_key: string, opts?: { defaultValue?: string }) =>
        opts?.defaultValue ?? "",
    }),
  };
});

vi.mock("@/lib/tauri", () => ({
  getRemoteDiagnostics: mockGetRemoteDiagnostics,
  resolveRemoteConflict: mockResolveRemoteConflict,
}));

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
}));

describe("SettingsRemoteDiagnosticsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  test("renders nothing when no active remote library", async () => {
    mockGetRemoteDiagnostics.mockResolvedValue({
      has_active_remote: false,
      local_state: "clean",
      committed_generation: 0,
      repository_id: null,
      writer_id: null,
      local_base_generation: 0,
      local_db_digest: null,
      active_operation_id: null,
      last_success_at_ms: null,
      last_error_code: null,
      recent_operations: [],
    });

    const { container } = render(<SettingsRemoteDiagnosticsSection />);
    await waitFor(() => {
      expect(mockGetRemoteDiagnostics).toHaveBeenCalled();
    });
    expect(container.firstChild).toBeNull();
  });

  test("renders diagnostics with clean state", async () => {
    mockGetRemoteDiagnostics.mockResolvedValue({
      has_active_remote: true,
      local_state: "clean",
      committed_generation: 42,
      repository_id: "abc123def456",
      writer_id: null,
      local_base_generation: 42,
      local_db_digest: null,
      active_operation_id: null,
      last_success_at_ms: null,
      last_error_code: null,
      recent_operations: [],
    });

    render(<SettingsRemoteDiagnosticsSection />);

    await waitFor(() => {
      expect(screen.getByText("42")).toBeTruthy();
    });
    expect(screen.getByText("clean")).toBeTruthy();
    expect(screen.getByText("abc123de")).toBeTruthy();
  });

  test("renders conflicted state in destructive color", async () => {
    mockGetRemoteDiagnostics.mockResolvedValue({
      has_active_remote: true,
      local_state: "conflicted",
      committed_generation: 10,
      repository_id: null,
      writer_id: null,
      local_base_generation: 9,
      local_db_digest: null,
      active_operation_id: null,
      last_success_at_ms: null,
      last_error_code: "conflict",
      recent_operations: [],
    });

    render(<SettingsRemoteDiagnosticsSection />);

    await waitFor(() => {
      expect(screen.getByText("conflicted")).toBeTruthy();
    });
    expect(screen.getByText("conflict")).toBeTruthy();
  });

  test("offers both exits from a conflict and re-reads the state after", async () => {
    mockGetRemoteDiagnostics.mockResolvedValue({
      has_active_remote: true,
      local_state: "conflicted",
      committed_generation: 10,
      repository_id: null,
      writer_id: null,
      local_base_generation: 9,
      local_db_digest: null,
      active_operation_id: "op-1",
      last_success_at_ms: null,
      last_error_code: "remote_conflict",
      recent_operations: [],
    });
    mockResolveRemoteConflict.mockResolvedValue(undefined);

    render(<SettingsRemoteDiagnosticsSection />);
    await waitFor(() => {
      expect(screen.getByText("Keep my changes")).toBeTruthy();
    });

    screen.getByText("Keep my changes").click();
    await waitFor(() => {
      expect(mockResolveRemoteConflict).toHaveBeenCalledWith("keep_local");
    });
    await waitFor(() => {
      expect(mockGetRemoteDiagnostics).toHaveBeenCalledTimes(2);
    });

    screen.getByText("Use the remote version").click();
    await waitFor(() => {
      expect(mockResolveRemoteConflict).toHaveBeenCalledWith("use_remote");
    });
  });

  test("keeps the conflict exits hidden when the repository is clean", async () => {
    mockGetRemoteDiagnostics.mockResolvedValue({
      has_active_remote: true,
      local_state: "clean",
      committed_generation: 10,
      repository_id: null,
      writer_id: null,
      local_base_generation: 10,
      local_db_digest: null,
      active_operation_id: null,
      last_success_at_ms: null,
      last_error_code: null,
      recent_operations: [],
    });

    render(<SettingsRemoteDiagnosticsSection />);
    await waitFor(() => {
      expect(screen.getByText("clean")).toBeTruthy();
    });
    expect(screen.queryByText("Keep my changes")).toBeNull();
  });

  test("renders recent operations list", async () => {
    mockGetRemoteDiagnostics.mockResolvedValue({
      has_active_remote: true,
      local_state: "dirty",
      committed_generation: 5,
      repository_id: null,
      writer_id: null,
      local_base_generation: 4,
      local_db_digest: null,
      active_operation_id: null,
      last_success_at_ms: null,
      last_error_code: null,
      recent_operations: [
        {
          operation_id: "op-1",
          operation_kind: "publish",
          state: "completed",
          expected_generation: null,
          target_generation: null,
          attempt_count: 1,
          error_code: null,
          error_detail: null,
          created_at_ms: 0,
          updated_at_ms: 0,
        },
        {
          operation_id: "op-2",
          operation_kind: "download",
          state: "failed",
          expected_generation: null,
          target_generation: null,
          attempt_count: 1,
          error_code: "not_found",
          error_detail: null,
          created_at_ms: 0,
          updated_at_ms: 0,
        },
      ],
    });

    render(<SettingsRemoteDiagnosticsSection />);

    await waitFor(() => {
      expect(screen.getByText("publish")).toBeTruthy();
    });
    expect(screen.getByText("download")).toBeTruthy();
    expect(screen.getByText("not_found")).toBeTruthy();
  });

  test("refresh button re-fetches diagnostics", async () => {
    mockGetRemoteDiagnostics.mockResolvedValue({
      has_active_remote: true,
      local_state: "clean",
      committed_generation: 1,
      repository_id: null,
      writer_id: null,
      local_base_generation: 1,
      local_db_digest: null,
      active_operation_id: null,
      last_success_at_ms: null,
      last_error_code: null,
      recent_operations: [],
    });

    render(<SettingsRemoteDiagnosticsSection />);

    await waitFor(() => {
      expect(mockGetRemoteDiagnostics).toHaveBeenCalledOnce();
    });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Refresh/i }));
    });

    await waitFor(() => {
      expect(mockGetRemoteDiagnostics).toHaveBeenCalledTimes(2);
    });
  });

  test("surfaces error when diagnostics fetch fails", async () => {
    mockGetRemoteDiagnostics.mockRejectedValue(new Error("network"));

    render(<SettingsRemoteDiagnosticsSection />);

    await waitFor(() => {
      expect(mockNotifyError).toHaveBeenCalled();
    });
  });
});
