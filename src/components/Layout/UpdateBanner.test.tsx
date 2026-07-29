// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { DownloadEvent } from "@tauri-apps/plugin-updater";
import { UpdateBanner } from "./UpdateBanner";

const { mockInvoke, mockCheck, mockRelaunch, mockDownloadAndInstall } =
  vi.hoisted(() => ({
    mockInvoke: vi.fn().mockResolvedValue(true),
    mockCheck: vi.fn(),
    mockRelaunch: vi.fn().mockResolvedValue(undefined),
    mockDownloadAndInstall: vi.fn().mockResolvedValue(undefined),
  }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mockCheck }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: mockRelaunch }));

function updateAvailable(version = "1.2.3") {
  return { version, downloadAndInstall: mockDownloadAndInstall };
}

async function flushCheck() {
  await waitFor(() => expect(mockCheck).toHaveBeenCalled());
  await Promise.resolve();
}

describe("UpdateBanner", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue(true);
    mockRelaunch.mockResolvedValue(undefined);
    mockDownloadAndInstall.mockResolvedValue(undefined);
  });

  afterEach(cleanup);

  test("renders the available banner when an update is found", async () => {
    mockCheck.mockResolvedValue(updateAvailable());
    render(<UpdateBanner />);

    expect(await screen.findByText("updater.available")).toBeTruthy();
    expect(screen.getByRole("button", { name: "updater.update" })).toBeTruthy();
  });

  test("renders nothing when there is no update", async () => {
    mockCheck.mockResolvedValue(null);
    const { container } = render(<UpdateBanner />);

    await flushCheck();
    expect(container.firstChild).toBeNull();
  });

  test("stays silent and does not crash when the check rejects", async () => {
    mockCheck.mockRejectedValue(new Error("offline"));
    const { container } = render(<UpdateBanner />);

    await flushCheck();
    expect(container.firstChild).toBeNull();
  });

  test("stays silent and never checks on a non-updatable install", async () => {
    // e.g. a Linux .deb/Flatpak: self_update_supported resolves false.
    mockInvoke.mockResolvedValue(false);
    mockCheck.mockResolvedValue(updateAvailable());
    const { container } = render(<UpdateBanner />);

    await waitFor(() => expect(mockInvoke).toHaveBeenCalled());
    await Promise.resolve();
    expect(mockCheck).not.toHaveBeenCalled();
    expect(container.firstChild).toBeNull();
  });

  test("can be dismissed, hiding the banner", async () => {
    mockCheck.mockResolvedValue(updateAvailable());
    const { container } = render(<UpdateBanner />);

    await screen.findByText("updater.available");
    fireEvent.click(screen.getByLabelText("common.close"));
    expect(container.firstChild).toBeNull();
  });

  test("install flow downloads, then offers a restart that relaunches", async () => {
    mockCheck.mockResolvedValue(updateAvailable());
    render(<UpdateBanner />);

    await screen.findByText("updater.available");
    fireEvent.click(screen.getByRole("button", { name: "updater.update" }));

    expect(mockDownloadAndInstall).toHaveBeenCalledOnce();

    fireEvent.click(
      await screen.findByRole("button", { name: "updater.restart" }),
    );
    await waitFor(() => expect(mockRelaunch).toHaveBeenCalledOnce());
  });

  test("reports download progress and reaches the restart state", async () => {
    const events: DownloadEvent[] = [
      { event: "Started", data: { contentLength: 100 } },
      { event: "Progress", data: { chunkLength: 40 } },
      { event: "Progress", data: { chunkLength: 60 } },
      { event: "Finished" },
    ];
    mockDownloadAndInstall.mockImplementation(
      async (onEvent?: (event: DownloadEvent) => void) => {
        for (const event of events) onEvent?.(event);
      },
    );
    mockCheck.mockResolvedValue(updateAvailable());
    render(<UpdateBanner />);

    await screen.findByText("updater.available");
    fireEvent.click(screen.getByRole("button", { name: "updater.update" }));

    expect(
      await screen.findByRole("button", { name: "updater.restart" }),
    ).toBeTruthy();
  });

  test("shows a dismissible failure message when install throws", async () => {
    mockDownloadAndInstall.mockRejectedValue(new Error("bad signature"));
    mockCheck.mockResolvedValue(updateAvailable());
    const { container } = render(<UpdateBanner />);

    await screen.findByText("updater.available");
    fireEvent.click(screen.getByRole("button", { name: "updater.update" }));

    expect(await screen.findByText("updater.failed")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("common.close"));
    expect(container.firstChild).toBeNull();
  });
});
