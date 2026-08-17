import { describe, expect, test, vi } from "vitest";
import { createPlaybackSession } from "./session";
import {
  createYoutubeVideoTransport,
  shouldIgnoreYoutubeChrome,
  shouldRefuseAirPlayLyrics,
  youtubeWatchUrl,
} from "./youtube-transport";
import {
  createRecordingYoutubeWatchHost,
  createTauriYoutubeWatchHost,
  isPublicYoutubeWatchUrl,
  resolveYoutubeWatchAttachTarget,
  type YoutubeWatchNativeSurface,
} from "./youtube-watch-host";
import type { PlaybackStateSnapshot } from "@/types/ipc";

function snapshot(
  overrides: Partial<PlaybackStateSnapshot> = {},
): PlaybackStateSnapshot {
  return {
    song_id: null,
    transport_generation: 0,
    state: "idle",
    is_playing: false,
    position_ms: 0,
    duration_ms: null,
    buffered_ms: 0,
    volume: 1,
    stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
    has_stems: false,
    stem_mode: null,
    ...overrides,
  };
}

describe("youtube transport helpers", () => {
  test("watch url never points at /player", () => {
    expect(youtubeWatchUrl("yt:dQw4w9WgXcQ")).toBe(
      "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
    );
    expect(youtubeWatchUrl("yt:dQw4w9WgXcQ")).not.toContain("/player");
    expect(isPublicYoutubeWatchUrl(youtubeWatchUrl("yt:dQw4w9WgXcQ"))).toBe(
      true,
    );
    expect(
      isPublicYoutubeWatchUrl(
        "https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
      ),
    ).toBe(false);
  });

  test("chrome and AirPlay lyrics refuse video queue ids", () => {
    expect(shouldIgnoreYoutubeChrome("yt:abc")).toBe(true);
    expect(shouldRefuseAirPlayLyrics("yt:abc")).toBe(true);
    expect(shouldIgnoreYoutubeChrome("hash")).toBe(false);
  });
});

describe("youtube watch host", () => {
  test("play loads a watch page through the host and does not create an iframe", async () => {
    const host = createRecordingYoutubeWatchHost();
    const transport = createYoutubeVideoTransport({ host });
    const next = await transport.play("yt:dQw4w9WgXcQ");
    expect(next.song_id).toBe("yt:dQw4w9WgXcQ");
    expect(host.loads).toEqual(["https://www.youtube.com/watch?v=dQw4w9WgXcQ"]);
    expect(host.loads[0]).not.toContain("/player");
    expect(host.createdIframe).toBe(false);
    if (typeof document !== "undefined") {
      expect(document.querySelectorAll("iframe")).toHaveLength(0);
    }
    await transport.pause();
    await transport.seek(1500);
    await transport.setVolume(0.4);
    await transport.teardown();
    expect(host.commands).toEqual([
      "load:https://www.youtube.com/watch?v=dQw4w9WgXcQ",
      "play",
      "pause",
      "seek:1500",
      "volume:0.4",
      "teardown",
    ]);
    expect(transport.isActive()).toBe(false);
  });

  test("audience attach target owns the single player", () => {
    const audience = resolveYoutubeWatchAttachTarget({
      audienceActive: true,
      localWindowLabel: "main",
      localBounds: { x: 10, y: 10, width: 400, height: 300 },
      audienceFill: { x: 0, y: 0, width: 1920, height: 1000 },
    });
    expect(audience).toEqual({
      windowLabel: "fullscreen-player",
      bounds: { x: 0, y: 0, width: 1920, height: 1000 },
    });

    const local = resolveYoutubeWatchAttachTarget({
      audienceActive: false,
      localWindowLabel: "main",
      localBounds: { x: 12, y: 48, width: 640, height: 360 },
    });
    expect(local).toEqual({
      windowLabel: "main",
      bounds: { x: 12, y: 48, width: 640, height: 360 },
    });
  });

  test("tauri host loads the watch URL and tears down the labeled webview", async () => {
    const created: Array<Record<string, unknown>> = [];
    const controls: string[] = [];
    let closed = false;
    let paused = true;
    const surface: YoutubeWatchNativeSurface = {
      async getByLabel(label) {
        if (label !== "youtube-watch" || created.length === 0 || closed) {
          return null;
        }
        return {
          reparent: async () => {
            controls.push("reparent");
          },
          setPosition: async () => {
            controls.push("position");
          },
          setSize: async () => {
            controls.push("size");
          },
          close: async () => {
            closed = true;
            controls.push("close");
          },
        };
      },
      async create(windowLabel, label, options) {
        created.push({ windowLabel, label, ...options });
      },
      async currentWindowLabel() {
        return "main";
      },
      async audienceFillBounds() {
        return null;
      },
      async control(action) {
        controls.push(action.type);
        if (action.type === "play") {
          paused = false;
        }
        if (action.type === "pause") {
          paused = true;
        }
        return {
          ended: false,
          paused,
          current_time_ms: 0,
          duration_ms: paused ? null : 180_000,
        };
      },
      async listenBounds() {
        return () => {};
      },
    };

    const host = createTauriYoutubeWatchHost({
      surface,
      audienceActive: () => false,
    });
    await host.load("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    await host.play();
    await host.teardown();

    expect(created).toEqual([
      {
        windowLabel: "main",
        label: "youtube-watch",
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
        incognito: true,
      },
    ]);
    expect(created[0]?.url).not.toContain("/player");
    expect(controls.filter((item) => item === "play").length).toBeGreaterThan(
      0,
    );
    expect(controls).toContain("query");
    expect(controls).toContain("close");
  });

  test("tauri host retries play, reports time, and relayouts the same watch URL", async () => {
    const times: number[] = [];
    let created = false;
    const surface: YoutubeWatchNativeSurface = {
      async getByLabel(label) {
        if (label !== "youtube-watch" || !created) {
          return null;
        }
        return {
          reparent: async () => {},
          setPosition: async () => {},
          setSize: async () => {},
          close: async () => {},
        };
      },
      async create() {
        created = true;
      },
      async currentWindowLabel() {
        return "main";
      },
      async audienceFillBounds() {
        return { x: 0, y: 0, width: 800, height: 450 };
      },
      async control(action) {
        if (action.type === "query" || action.type === "play") {
          return {
            ended: false,
            paused: false,
            current_time_ms: 1200,
            duration_ms: 10_000,
          };
        }
        return {
          ended: false,
          paused: true,
          current_time_ms: 0,
          duration_ms: null,
        };
      },
      async listenBounds() {
        return () => {};
      },
    };
    const host = createTauriYoutubeWatchHost({
      surface,
      audienceActive: () => true,
      onTime: (positionMs) => {
        times.push(positionMs);
      },
    });
    await host.load("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    await host.play();
    await host.relayout();
    await host.seek(500);
    await host.setVolume(0.2);
    await host.pause();
    expect(times[0]).toBe(1200);
    expect(
      resolveYoutubeWatchAttachTarget({
        audienceActive: true,
        localWindowLabel: "fullscreen-player",
        localBounds: { x: 1, y: 2, width: 3, height: 4 },
      })?.windowLabel,
    ).toBe("fullscreen-player");
  });
});

describe("youtube session wiring", () => {
  test("play of yt: asks the host and does not invoke local decode", async () => {
    const host = createRecordingYoutubeWatchHost();
    const transport = createYoutubeVideoTransport({ host });
    const localPlay = vi
      .fn()
      .mockRejectedValue(new Error("local play must not run"));
    const session = createPlaybackSession({
      transport: {
        play: localPlay,
        resume: vi.fn(),
        pause: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
        setStemVolume: vi.fn(),
        loadStems: vi.fn(),
        getPlaybackState: vi.fn().mockResolvedValue(snapshot()),
      },
      queue: {
        addToQueue: vi.fn(),
        dequeue: vi.fn().mockReturnValue(null),
        pushToHistory: vi.fn(),
        popFromHistory: vi.fn().mockReturnValue(null),
        removeSongIds: vi.fn(),
      },
      getSeparationStatus: () => undefined,
      onClockChange: vi.fn(),
      nowMs: () => 1000,
      videoTransport: transport,
      stopLocalAndCancelPreload: vi.fn().mockResolvedValue(undefined),
    });

    await session.playNow("yt:dQw4w9WgXcQ");
    expect(localPlay).not.toHaveBeenCalled();
    expect(host.loads[0]).toBe("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
  });

  test("play of a library hash after YouTube tears down the webview", async () => {
    const host = createRecordingYoutubeWatchHost();
    const transport = createYoutubeVideoTransport({ host });
    await transport.play("yt:abc");
    const localPlay = vi
      .fn()
      .mockResolvedValue(snapshot({ song_id: "hash-1", is_playing: true }));
    const session = createPlaybackSession({
      transport: {
        play: localPlay,
        resume: vi.fn(),
        pause: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
        setStemVolume: vi.fn(),
        loadStems: vi.fn(),
        getPlaybackState: vi.fn().mockResolvedValue(snapshot()),
      },
      queue: {
        addToQueue: vi.fn(),
        dequeue: vi.fn().mockReturnValue(null),
        pushToHistory: vi.fn(),
        popFromHistory: vi.fn().mockReturnValue(null),
        removeSongIds: vi.fn(),
      },
      getSeparationStatus: () => undefined,
      onClockChange: vi.fn(),
      nowMs: () => 1000,
      videoTransport: transport,
    });

    await session.playNow("hash-1");
    expect(host.commands).toContain("teardown");
    expect(localPlay).toHaveBeenCalledWith("hash-1");
  });

  test("ended dequeues the next id through the session", async () => {
    const host = createRecordingYoutubeWatchHost();
    const transport = createYoutubeVideoTransport({ host });
    const session = createPlaybackSession({
      transport: {
        play: vi.fn().mockRejectedValue(new Error("local play must not run")),
        resume: vi.fn(),
        pause: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
        setStemVolume: vi.fn(),
        loadStems: vi.fn(),
        getPlaybackState: vi.fn().mockResolvedValue(snapshot()),
      },
      queue: {
        addToQueue: vi.fn(),
        dequeue: vi.fn().mockReturnValue("yt:next"),
        pushToHistory: vi.fn(),
        popFromHistory: vi.fn().mockReturnValue(null),
        removeSongIds: vi.fn(),
      },
      getSeparationStatus: () => undefined,
      onClockChange: vi.fn(),
      nowMs: () => 1000,
      videoTransport: transport,
      stopLocalAndCancelPreload: vi.fn().mockResolvedValue(undefined),
    });
    await session.playNow("yt:abc");
    await session.onEnded("yt:abc");
    expect(host.loads).toEqual([
      "https://www.youtube.com/watch?v=abc",
      "https://www.youtube.com/watch?v=next",
    ]);
  });
});
