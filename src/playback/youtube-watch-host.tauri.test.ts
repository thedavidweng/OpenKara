import { describe, expect, test, vi } from "vitest";

const {
  mockGetByLabel,
  mockWindowGetByLabel,
  mockGetCurrent,
  mockListen,
  mockControl,
} = vi.hoisted(() => ({
  mockGetByLabel: vi.fn(),
  mockWindowGetByLabel: vi.fn(),
  mockGetCurrent: vi.fn(),
  mockListen: vi.fn(),
  mockControl: vi.fn(),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  Webview: class {
    static getByLabel = mockGetByLabel;
    constructor() {}
    once(event: string, handler: () => void) {
      if (event === "tauri://created") {
        handler();
      }
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
  createYoutubeWatchCommands: () => ({
    controlYoutubeWatch: mockControl,
  }),
}));

vi.mock("@/lib/tauri/invoke", () => ({
  tauriInvoke: vi.fn(),
}));

describe("default YouTube watch surface", () => {
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
      await import("./youtube-watch-host");
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
    await surface!.listenBounds(() => {});
    expect(mockListen).toHaveBeenCalled();
  });
});
