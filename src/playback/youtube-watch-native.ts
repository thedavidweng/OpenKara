import { listen } from "@tauri-apps/api/event";
import { Webview } from "@tauri-apps/api/webview";
import { LogicalPosition, LogicalSize, Window } from "@tauri-apps/api/window";
import { tauriInvoke } from "@/lib/tauri/invoke";
import { createYoutubeWatchCommands } from "@/lib/tauri/youtube-watch";
import {
  FULLSCREEN_PLAYER_WINDOW_LABEL,
  YOUTUBE_WATCH_BOUNDS_EVENT,
  type YoutubeWatchAttachTarget,
  type YoutubeWatchNativeSurface,
} from "./youtube-watch-host";

export async function createDefaultYoutubeWatchNativeSurface(): Promise<YoutubeWatchNativeSurface | null> {
  try {
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
