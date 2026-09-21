import { beforeEach, describe, expect, test, vi } from "vitest";

const {
  mockGetByLabel,
  mockWindowGetByLabel,
  mockGetCurrent,
  mockListen,
  mockControl,
  mockCreateCommands,
  mockWebviewOnce,
} = vi.hoisted(() => ({
  mockGetByLabel: vi.fn(),
  mockWindowGetByLabel: vi.fn(),
  mockGetCurrent: vi.fn(),
  mockListen: vi.fn(),
  mockControl: vi.fn(),
  mockCreateCommands: vi.fn(),
  mockWebviewOnce: vi.fn(),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  Webview: class {
    static getByLabel = mockGetByLabel;
    constructor() {}
    once(event: string, handler: (event?: { payload: unknown }) => void) {
      mockWebviewOnce(event, handler);
      return Promise.resolve();
    }
    reparent() {
      return Promise.resolve();
    }
    setPosition() {
      return Promise.resolve();
    }
    setSize() {
      return Promise.resolve();
    }
    close() {
      return Promise.resolve();
    }
  },
}));

vi.mock("@tauri-apps/api/window", () => ({
  Window: {
    getByLabel: mockWindowGetByLabel,
    getCurrent: mockGetCurrent,
  },
  LogicalPosition: class {
    constructor(
      public x: number,
      public y: number,
    ) {}
  },
  LogicalSize: class {
    constructor(
      public width: number,
      public height: number,
    ) {}
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: mockListen,
}));

vi.mock("@/lib/tauri/youtube-watch", () => ({
  createYoutubeWatchCommands: mockCreateCommands,
}));

vi.mock("@/lib/tauri/invoke", () => ({
  tauriInvoke: vi.fn(),
}));

describe("default YouTube watch surface", () => {
  beforeEach(() => {
    mockGetByLabel.mockReset();
    mockWindowGetByLabel.mockReset();
    mockGetCurrent.mockReset();
    mockListen.mockReset();
    mockControl.mockReset();
    mockCreateCommands.mockReset();
    mockWebviewOnce.mockReset();
    mockCreateCommands.mockReturnValue({
      controlYoutubeWatch: mockControl,
    });
    mockWebviewOnce.mockImplementation(
      (event: string, handler: (event?: { payload: unknown }) => void) => {
        if (event === "tauri://created") {
          handler();
        }
      },
    );
  });

  test("creates an incognito watch webview and reports audience bounds", async () => {
    mockGetCurrent.mockReturnValue({ label: "main" });
    mockWindowGetByLabel.mockImplementation(async (label: string) => {
      if (label === "fullscreen-player") {
        return {
          innerSize: async () => ({ width: 1920, height: 1080 }),
          scaleFactor: async () => 2,
        };
      }
      return { label };
    });
    mockGetByLabel.mockResolvedValue({
      reparent: vi.fn(),
      setPosition: vi.fn(),
      setSize: vi.fn(),
      close: vi.fn(),
    });
    mockListen.mockResolvedValue(() => {});
    mockControl.mockResolvedValue({
      ended: false,
      paused: false,
      current_time_ms: 0,
      duration_ms: 1000,
    });

    const { createDefaultYoutubeWatchNativeSurface } =
      await import("./youtube-watch-native");
    const surface = await createDefaultYoutubeWatchNativeSurface();
    expect(surface).not.toBeNull();
    await surface!.create("main", "youtube-watch", {
      url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
      x: 0,
      y: 0,
      width: 640,
      height: 360,
      incognito: true,
    });
    const handle = await surface!.getByLabel("youtube-watch");
    expect(handle).not.toBeNull();
    await handle!.reparent("fullscreen-player");
    await handle!.setPosition(1, 2);
    await handle!.setSize(3, 4);
    await handle!.close();
    expect(await surface!.currentWindowLabel()).toBe("main");
    expect(await surface!.audienceFillBounds()).toEqual({
      x: 0,
      y: 0,
      width: 960,
      height: 452,
    });
    await surface!.control({ type: "play" });
    expect(mockControl).toHaveBeenCalledWith({ type: "play" });
    const bounds: Array<{ windowLabel: string }> = [];
    mockListen.mockImplementation(async (_event, listener) => {
      listener({
        payload: {
          windowLabel: "main",
          bounds: { x: 0, y: 0, width: 1, height: 1 },
        },
      });
      return () => {};
    });
    await surface!.listenBounds((target) => {
      bounds.push(target);
    });
    expect(bounds).toEqual([
      {
        windowLabel: "main",
        bounds: { x: 0, y: 0, width: 1, height: 1 },
      },
    ]);
  });

  test("returns null when the labeled webview or audience window is missing", async () => {
    mockGetByLabel.mockResolvedValue(null);
    mockWindowGetByLabel.mockResolvedValue(null);
    mockGetCurrent.mockReturnValue({ label: "main" });
    const { createDefaultYoutubeWatchNativeSurface } =
      await import("./youtube-watch-native");
    const surface = await createDefaultYoutubeWatchNativeSurface();
    expect(surface).not.toBeNull();
    expect(await surface!.getByLabel("youtube-watch")).toBeNull();
    expect(await surface!.audienceFillBounds()).toBeNull();
    await expect(
      surface!.create("missing", "youtube-watch", {
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        x: 0,
        y: 0,
        width: 640,
        height: 360,
        incognito: true,
      }),
    ).rejects.toThrow("YouTube host window missing is missing");
  });

  test("rejects when the watch webview fails to create", async () => {
    mockWindowGetByLabel.mockResolvedValue({ label: "main" });
    mockWebviewOnce.mockImplementation(
      (event: string, handler: (event?: { payload: unknown }) => void) => {
        if (event === "tauri://error") {
          handler({ payload: "create failed" });
        }
      },
    );
    const { createDefaultYoutubeWatchNativeSurface } =
      await import("./youtube-watch-native");
    const surface = await createDefaultYoutubeWatchNativeSurface();
    expect(surface).not.toBeNull();
    await expect(
      surface!.create("main", "youtube-watch", {
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        x: 0,
        y: 0,
        width: 640,
        height: 360,
        incognito: true,
      }),
    ).rejects.toBe("create failed");
  });

  test("returns null when the native Tauri command surface cannot be created", async () => {
    mockCreateCommands.mockImplementation(() => {
      throw new Error("no tauri runtime");
    });
    const { createDefaultYoutubeWatchNativeSurface } =
      await import("./youtube-watch-native");
    expect(await createDefaultYoutubeWatchNativeSurface()).toBeNull();
  });
});
