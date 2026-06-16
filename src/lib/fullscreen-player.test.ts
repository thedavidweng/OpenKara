import { beforeEach, describe, expect, test, vi } from "vitest";
import {
  closeFullscreenPlayer,
  openFullscreenPlayer,
} from "./fullscreen-player";

const {
  mockCloseCurrent,
  mockCloseByLabel,
  mockGetByLabel,
  mockGetCurrentWebviewWindow,
  mockAvailableMonitors,
  mockWebviewWindowConstructor,
} = vi.hoisted(() => ({
  mockCloseCurrent: vi.fn(),
  mockCloseByLabel: vi.fn(),
  mockGetByLabel: vi.fn(),
  mockGetCurrentWebviewWindow: vi.fn(() => ({
    label: "fullscreen-player",
    close: mockCloseCurrent,
  })),
  mockAvailableMonitors: vi.fn(),
  mockWebviewWindowConstructor: vi.fn(),
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: Object.assign(mockWebviewWindowConstructor, {
    getByLabel: mockGetByLabel,
  }),
  getCurrentWebviewWindow: mockGetCurrentWebviewWindow,
}));

vi.mock("@tauri-apps/api/window", () => ({
  availableMonitors: mockAvailableMonitors,
}));

const monitors = [
  {
    position: { x: 0, y: 0 },
    size: { width: 1920, height: 1080 },
  },
  {
    position: { x: 1920, y: 0 },
    size: { width: 2560, height: 1440 },
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  // Restore default implementation (overridden by some tests)
  mockGetCurrentWebviewWindow.mockReturnValue({
    label: "fullscreen-player",
    close: mockCloseCurrent,
  });
});

describe("closeFullscreenPlayer", () => {
  test("closes the current fullscreen window directly", async () => {
    mockGetCurrentWebviewWindow.mockReturnValue({
      label: "fullscreen-player",
      close: mockCloseCurrent,
    });
    mockGetByLabel.mockResolvedValue({ close: mockCloseByLabel });

    await closeFullscreenPlayer();

    expect(mockCloseCurrent).toHaveBeenCalledOnce();
    expect(mockCloseByLabel).not.toHaveBeenCalled();
  });

  test("finds and closes by label when current window is not the fullscreen player", async () => {
    mockGetCurrentWebviewWindow.mockReturnValue({
      label: "main",
      close: mockCloseCurrent,
    });
    mockGetByLabel.mockResolvedValue({ close: mockCloseByLabel });

    await closeFullscreenPlayer();

    expect(mockCloseCurrent).not.toHaveBeenCalled();
    expect(mockGetByLabel).toHaveBeenCalledWith("fullscreen-player");
    expect(mockCloseByLabel).toHaveBeenCalledOnce();
  });

  test("does nothing when no fullscreen-player window exists and current is different", async () => {
    mockGetCurrentWebviewWindow.mockReturnValue({
      label: "main",
      close: mockCloseCurrent,
    });
    mockGetByLabel.mockResolvedValue(null);

    // Should not throw
    await closeFullscreenPlayer();

    expect(mockCloseCurrent).not.toHaveBeenCalled();
    expect(mockGetByLabel).toHaveBeenCalledWith("fullscreen-player");
  });

  test("handles errors gracefully", async () => {
    mockGetCurrentWebviewWindow.mockImplementation(() => {
      throw new Error("no window");
    });
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    // Should not throw
    await closeFullscreenPlayer();

    expect(consoleSpy).toHaveBeenCalledWith(
      "Failed to close fullscreen player:",
      expect.any(Error),
    );

    consoleSpy.mockRestore();
  });
});

describe("openFullscreenPlayer", () => {
  test("closes existing fullscreen-player window before creating a new one", async () => {
    mockGetByLabel.mockResolvedValue({ close: mockCloseByLabel });
    mockAvailableMonitors.mockResolvedValue(monitors);

    await openFullscreenPlayer();

    expect(mockGetByLabel).toHaveBeenCalledWith("fullscreen-player");
    expect(mockCloseByLabel).toHaveBeenCalledOnce();
  });

  test("creates window on secondary monitor by default when multiple monitors exist", async () => {
    mockGetByLabel.mockResolvedValue(null);
    mockAvailableMonitors.mockResolvedValue(monitors);

    await openFullscreenPlayer();

    expect(mockWebviewWindowConstructor).toHaveBeenCalledWith(
      "fullscreen-player",
      expect.objectContaining({
        url: "index.html?mode=fullscreen-player",
        title: "OpenKara Player",
        x: 1920,
        y: 0,
        width: 2560,
        height: 1440,
      }),
    );
  });

  test("creates window on primary monitor when only one monitor exists", async () => {
    mockGetByLabel.mockResolvedValue(null);
    mockAvailableMonitors.mockResolvedValue([monitors[0]]);

    await openFullscreenPlayer();

    expect(mockWebviewWindowConstructor).toHaveBeenCalledWith(
      "fullscreen-player",
      expect.objectContaining({
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
      }),
    );
  });

  test("uses specified monitorIndex when provided", async () => {
    mockGetByLabel.mockResolvedValue(null);
    mockAvailableMonitors.mockResolvedValue(monitors);

    await openFullscreenPlayer(1);

    expect(mockWebviewWindowConstructor).toHaveBeenCalledWith(
      "fullscreen-player",
      expect.objectContaining({
        x: 1920,
        y: 0,
        width: 2560,
        height: 1440,
      }),
    );
  });

  test("uses first monitor when monitorIndex is 0", async () => {
    mockGetByLabel.mockResolvedValue(null);
    mockAvailableMonitors.mockResolvedValue(monitors);

    await openFullscreenPlayer(0);

    expect(mockWebviewWindowConstructor).toHaveBeenCalledWith(
      "fullscreen-player",
      expect.objectContaining({
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
      }),
    );
  });

  test("skips close when no existing fullscreen-player window", async () => {
    mockGetByLabel.mockResolvedValue(null);
    mockAvailableMonitors.mockResolvedValue(monitors);

    await openFullscreenPlayer();

    expect(mockCloseByLabel).not.toHaveBeenCalled();
    expect(mockWebviewWindowConstructor).toHaveBeenCalledOnce();
  });
});
