// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { renderWithBackend } from "@/test-utils/backend";
import { SettingsRemoteCacheSection } from "./SettingsRemoteCacheSection";

const { mockGetRemoteCacheUsage, mockClearRemoteCache, mockNotifyError } =
  vi.hoisted(() => ({
    mockGetRemoteCacheUsage: vi.fn(),
    mockClearRemoteCache: vi.fn(),
    mockNotifyError: vi.fn(),
  }));

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, opts?: { count?: number }) =>
        key === "settings.remoteCache.evicted"
          ? `Evicted ${opts?.count ?? 0} entries.`
          : key === "settings.remoteCache.clearButton"
            ? "Clear Cache"
            : key === "settings.remoteCache.clearing"
              ? "Clearing…"
              : key,
    }),
  };
});

const backend = createMockBackend({
  overrides: {
    remoteRepository: {
      getRemoteCacheUsage: mockGetRemoteCacheUsage,
      clearRemoteCache: mockClearRemoteCache,
    },
  },
});

function render(ui: ReactElement) {
  return renderWithBackend(ui, backend);
}

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
}));

describe("SettingsRemoteCacheSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  test("renders cache usage and clear button after loading", async () => {
    mockGetRemoteCacheUsage.mockResolvedValue({
      used_bytes: 1_073_741_824,
      limit_bytes: 2_147_483_648,
      entry_count: 5,
      pinned_count: 1,
    });

    render(<SettingsRemoteCacheSection />);

    await waitFor(() => {
      expect(screen.getByText(/1\.0[0 ]?GB/)).toBeTruthy();
    });
    expect(screen.getByText("5")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Clear Cache/i })).toBeTruthy();
  });

  test("clears cache and shows evicted count", async () => {
    mockGetRemoteCacheUsage
      .mockResolvedValueOnce({
        used_bytes: 500_000_000,
        limit_bytes: 2_147_483_648,
        entry_count: 3,
        pinned_count: 0,
      })
      .mockResolvedValueOnce({
        used_bytes: 0,
        limit_bytes: 2_147_483_648,
        entry_count: 0,
        pinned_count: 0,
      });
    mockClearRemoteCache.mockResolvedValue(3);

    render(<SettingsRemoteCacheSection />);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Clear Cache/i })).toBeTruthy();
    });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Clear Cache/i }));
    });

    await waitFor(() => {
      expect(screen.getByText(/Evicted/)).toBeTruthy();
    });
    expect(mockClearRemoteCache).toHaveBeenCalledOnce();
  });

  test("disables clear button when cache is empty", async () => {
    mockGetRemoteCacheUsage.mockResolvedValue({
      used_bytes: 0,
      limit_bytes: 2_147_483_648,
      entry_count: 0,
      pinned_count: 0,
    });

    render(<SettingsRemoteCacheSection />);

    await waitFor(() => {
      const btn = screen.getByRole("button", { name: /Clear Cache/i });
      expect((btn as HTMLButtonElement).disabled).toBe(true);
    });
  });

  test("surfaces error when usage fetch fails", async () => {
    mockGetRemoteCacheUsage.mockRejectedValue(new Error("network"));

    render(<SettingsRemoteCacheSection />);

    await waitFor(() => {
      expect(mockNotifyError).toHaveBeenCalled();
    });
  });

  test("surfaces error when clear fails", async () => {
    mockGetRemoteCacheUsage.mockResolvedValue({
      used_bytes: 500_000_000,
      limit_bytes: 2_147_483_648,
      entry_count: 3,
      pinned_count: 0,
    });
    mockClearRemoteCache.mockRejectedValue(new Error("disk"));

    render(<SettingsRemoteCacheSection />);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Clear Cache/i })).toBeTruthy();
    });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Clear Cache/i }));
    });

    await waitFor(() => {
      expect(mockNotifyError).toHaveBeenCalled();
    });
  });
});
