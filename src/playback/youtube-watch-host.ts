import type { YoutubeWatchAction, YoutubeWatchMediaState } from "@/types/ipc";

export type { YoutubeWatchAction, YoutubeWatchMediaState };

export const YOUTUBE_WATCH_WEBVIEW_LABEL = "youtube-watch";
export const YOUTUBE_WATCH_HOST_ELEMENT_ID = "openkara-youtube-host";
export const YOUTUBE_WATCH_BOUNDS_EVENT = "openkara://youtube-watch-bounds";
export const FULLSCREEN_PLAYER_WINDOW_LABEL = "fullscreen-player";

export interface YoutubeWatchBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface YoutubeWatchAttachTarget {
  windowLabel: string;
  bounds: YoutubeWatchBounds;
}

export interface YoutubeWatchHost {
  load(watchUrl: string): Promise<void>;
  play(): Promise<void>;
  pause(): Promise<void>;
  seek(ms: number): Promise<void>;
  setVolume(level: number): Promise<void>;
  relayout(): Promise<void>;
  teardown(): Promise<void>;
}

export interface RecordingYoutubeWatchHost extends YoutubeWatchHost {
  readonly loads: readonly string[];
  readonly commands: readonly string[];
  readonly createdIframe: boolean;
  emitEnded(): void;
}

export function isPublicYoutubeWatchUrl(url: string): boolean {
  if (url.includes("/player")) {
    return false;
  }
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "https:") {
      return false;
    }
    if (
      parsed.hostname !== "www.youtube.com" &&
      parsed.hostname !== "youtube.com" &&
      parsed.hostname !== "m.youtube.com"
    ) {
      return false;
    }
    if (parsed.pathname !== "/watch") {
      return false;
    }
    const videoId = parsed.searchParams.get("v");
    return !!videoId && /^[\w-]+$/.test(videoId);
  } catch {
    return false;
  }
}

export function measureYoutubeWatchHostElement(
  element: Pick<Element, "getBoundingClientRect"> | null,
): YoutubeWatchBounds | null {
  if (!element) {
    return null;
  }
  const rect = element.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) {
    return null;
  }
  return {
    x: rect.left,
    y: rect.top,
    width: rect.width,
    height: rect.height,
  };
}

export function resolveYoutubeWatchAttachTarget(input: {
  audienceActive: boolean;
  localWindowLabel: string;
  localBounds: YoutubeWatchBounds | null;
  audienceWindowLabel?: string;
  audienceFill?: YoutubeWatchBounds | null;
}): YoutubeWatchAttachTarget | null {
  const audienceWindowLabel =
    input.audienceWindowLabel ?? FULLSCREEN_PLAYER_WINDOW_LABEL;
  if (input.audienceActive) {
    if (input.localWindowLabel === audienceWindowLabel && input.localBounds) {
      return {
        windowLabel: audienceWindowLabel,
        bounds: input.localBounds,
      };
    }
    if (input.audienceFill) {
      return {
        windowLabel: audienceWindowLabel,
        bounds: input.audienceFill,
      };
    }
    return {
      windowLabel: audienceWindowLabel,
      bounds: { x: 0, y: 0, width: 1280, height: 720 },
    };
  }
  if (input.localBounds) {
    return {
      windowLabel: input.localWindowLabel,
      bounds: input.localBounds,
    };
  }
  return null;
}

export function createRecordingYoutubeWatchHost(
  onEnded?: () => void,
): RecordingYoutubeWatchHost {
  const loads: string[] = [];
  const commands: string[] = [];
  let endedHandler = onEnded;

  return {
    loads,
    commands,
    createdIframe: false,
    emitEnded() {
      endedHandler?.();
    },
    async load(watchUrl) {
      if (!isPublicYoutubeWatchUrl(watchUrl)) {
        throw new Error("YouTube host only loads a public watch URL");
      }
      loads.push(watchUrl);
      commands.push(`load:${watchUrl}`);
    },
    async play() {
      commands.push("play");
    },
    async pause() {
      commands.push("pause");
    },
    async seek(ms) {
      commands.push(`seek:${ms}`);
    },
    async setVolume(level) {
      commands.push(`volume:${level}`);
    },
    async relayout() {
      commands.push("relayout");
    },
    async teardown() {
      commands.push("teardown");
    },
  };
}

interface YoutubeWatchNativeHandle {
  reparent(windowLabel: string): Promise<void>;
  setPosition(x: number, y: number): Promise<void>;
  setSize(width: number, height: number): Promise<void>;
  close(): Promise<void>;
}

export interface YoutubeWatchNativeSurface {
  getByLabel(label: string): Promise<YoutubeWatchNativeHandle | null>;
  create(
    windowLabel: string,
    label: string,
    options: {
      url: string;
      x: number;
      y: number;
      width: number;
      height: number;
      incognito: boolean;
    },
  ): Promise<void>;
  currentWindowLabel(): Promise<string>;
  audienceFillBounds(): Promise<YoutubeWatchBounds | null>;
  control(action: YoutubeWatchAction): Promise<YoutubeWatchMediaState>;
  listenBounds(
    listener: (target: YoutubeWatchAttachTarget) => void,
  ): Promise<() => void>;
}

export function createTauriYoutubeWatchHost(options: {
  surface: YoutubeWatchNativeSurface;
  audienceActive: () => boolean;
  onEnded?: () => void;
  onTime?: (positionMs: number, durationMs: number | null) => void;
  pollMs?: number;
}): YoutubeWatchHost {
  let activeUrl: string | null = null;
  let pollTimer: ReturnType<typeof setInterval> | null = null;
  let unlistenBounds: (() => void) | null = null;
  let lastTarget: YoutubeWatchAttachTarget | null = null;
  let endedNotified = false;

  const stopPoll = () => {
    if (pollTimer !== null) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  };

  const queryAndNotify = async () => {
    try {
      const state = await options.surface.control({ type: "query" });
      options.onTime?.(state.current_time_ms, state.duration_ms);
      if (state.ended && !endedNotified) {
        endedNotified = true;
        stopPoll();
        options.onEnded?.();
      }
    } catch {
      // The watch page may not have a <video> yet.
    }
  };

  const startPoll = () => {
    stopPoll();
    pollTimer = setInterval(() => {
      void queryAndNotify();
    }, options.pollMs ?? 500);
  };

  const attach = async (url: string) => {
    const localLabel = await options.surface.currentWindowLabel();
    const localEl =
      typeof document === "undefined"
        ? null
        : document.getElementById(YOUTUBE_WATCH_HOST_ELEMENT_ID);
    const target = lastTarget ??
      resolveYoutubeWatchAttachTarget({
        audienceActive: options.audienceActive(),
        localWindowLabel: localLabel,
        localBounds: measureYoutubeWatchHostElement(localEl),
        audienceFill: await options.surface.audienceFillBounds(),
      }) ?? {
        windowLabel: localLabel,
        bounds: { x: 0, y: 0, width: 1280, height: 720 },
      };
    lastTarget = target;
    const existing = await options.surface.getByLabel(
      YOUTUBE_WATCH_WEBVIEW_LABEL,
    );
    if (existing) {
      await existing.reparent(target.windowLabel);
      await existing.setPosition(target.bounds.x, target.bounds.y);
      await existing.setSize(target.bounds.width, target.bounds.height);
      if (activeUrl !== url) {
        await options.surface.control({ type: "navigate", url });
      }
      return;
    }
    await options.surface.create(
      target.windowLabel,
      YOUTUBE_WATCH_WEBVIEW_LABEL,
      {
        url,
        x: target.bounds.x,
        y: target.bounds.y,
        width: target.bounds.width,
        height: target.bounds.height,
        incognito: true,
      },
    );
  };

  return {
    async load(watchUrl) {
      if (!isPublicYoutubeWatchUrl(watchUrl)) {
        throw new Error("YouTube host only loads a public watch URL");
      }
      endedNotified = false;
      if (!unlistenBounds) {
        unlistenBounds = await options.surface.listenBounds((target) => {
          lastTarget = target;
          if (activeUrl) {
            void attach(activeUrl);
          }
        });
      }
      await attach(watchUrl);
      activeUrl = watchUrl;
    },
    async play() {
      for (let attempt = 0; attempt < 20; attempt += 1) {
        await options.surface.control({ type: "play" });
        const state = await options.surface.control({ type: "query" });
        options.onTime?.(state.current_time_ms, state.duration_ms);
        if (!state.paused || state.duration_ms != null || state.ended) {
          break;
        }
        await new Promise((resolve) => {
          setTimeout(resolve, 250);
        });
      }
      startPoll();
    },
    async pause() {
      stopPoll();
      await options.surface.control({ type: "pause" });
    },
    async seek(ms) {
      await options.surface.control({ type: "seek", ms });
    },
    async setVolume(level) {
      await options.surface.control({ type: "set_volume", level });
    },
    async relayout() {
      if (!activeUrl) {
        return;
      }
      await attach(activeUrl);
    },
    async teardown() {
      stopPoll();
      unlistenBounds?.();
      unlistenBounds = null;
      lastTarget = null;
      activeUrl = null;
      endedNotified = false;
      const existing = await options.surface.getByLabel(
        YOUTUBE_WATCH_WEBVIEW_LABEL,
      );
      await existing?.close();
    },
  };
}

export async function createDefaultYoutubeWatchNativeSurface(): Promise<YoutubeWatchNativeSurface | null> {
  try {
    const [{ Webview }, { Window, LogicalPosition, LogicalSize }, { listen }] =
      await Promise.all([
        import("@tauri-apps/api/webview"),
        import("@tauri-apps/api/window"),
        import("@tauri-apps/api/event"),
      ]);
    const { createYoutubeWatchCommands } =
      await import("@/lib/tauri/youtube-watch");
    const { tauriInvoke } = await import("@/lib/tauri/invoke");
    const commands = createYoutubeWatchCommands(tauriInvoke);

    return {
      async getByLabel(label) {
        const webview = await Webview.getByLabel(label);
        if (!webview) {
          return null;
        }
        return {
          reparent: (windowLabel) => webview.reparent(windowLabel),
          setPosition: (x, y) => webview.setPosition(new LogicalPosition(x, y)),
          setSize: (width, height) =>
            webview.setSize(new LogicalSize(width, height)),
          close: () => webview.close(),
        };
      },
      async create(windowLabel, label, options) {
        const parent = await Window.getByLabel(windowLabel);
        if (!parent) {
          throw new Error(`YouTube host window ${windowLabel} is missing`);
        }
        const webview = new Webview(parent, label, {
          url: options.url,
          x: options.x,
          y: options.y,
          width: options.width,
          height: options.height,
          incognito: options.incognito,
          focus: false,
          backgroundColor: "#000000",
        });
        await new Promise<void>((resolve, reject) => {
          void webview.once("tauri://created", () => resolve());
          void webview.once("tauri://error", (event) => {
            reject(event.payload);
          });
        });
      },
      async currentWindowLabel() {
        return Window.getCurrent().label;
      },
      async audienceFillBounds() {
        const audience = await Window.getByLabel(
          FULLSCREEN_PLAYER_WINDOW_LABEL,
        );
        if (!audience) {
          return null;
        }
        const size = await audience.innerSize();
        const scale = await audience.scaleFactor();
        const safeScale = scale > 0 ? scale : 1;
        return {
          x: 0,
          y: 0,
          width: size.width / safeScale,
          height: Math.max(120, size.height / safeScale - 88),
        };
      },
      control: (action) => commands.controlYoutubeWatch(action),
      async listenBounds(listener) {
        return listen<YoutubeWatchAttachTarget>(
          YOUTUBE_WATCH_BOUNDS_EVENT,
          (event) => {
            listener(event.payload);
          },
        );
      },
    };
  } catch {
    return null;
  }
}
