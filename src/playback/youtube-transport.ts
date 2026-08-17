import type { PlaybackStateSnapshot } from "@/types/ipc";
import type { VideoPlaybackTransport } from "./session";
import {
  createDefaultYoutubeWatchNativeSurface,
  createTauriYoutubeWatchHost,
  type YoutubeWatchHost,
} from "./youtube-watch-host";

export function youtubeVideoIdFromQueueId(queueId: string): string {
  return queueId.startsWith("yt:") ? queueId.slice(3) : queueId;
}

export function youtubeWatchUrl(queueId: string): string {
  return `https://www.youtube.com/watch?v=${youtubeVideoIdFromQueueId(queueId)}`;
}

export interface CreateYoutubeVideoTransportOptions {
  host?: YoutubeWatchHost;
  onEnded?: (queueId: string) => void;
  audienceActive?: () => boolean;
  onTime?: (positionMs: number, durationMs: number | null) => void;
}

function emptySnapshot(
  songId: string | null,
  generation: number,
  playing: boolean,
  positionMs: number,
  durationMs: number | null,
  volume: number,
): PlaybackStateSnapshot {
  return {
    song_id: songId,
    transport_generation: generation,
    state: playing ? "playing" : songId ? "idle" : "idle",
    is_playing: playing,
    position_ms: positionMs,
    duration_ms: durationMs,
    buffered_ms: durationMs ?? 0,
    volume,
    stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
    has_stems: false,
    stem_mode: null,
  };
}

function createLazyTauriHost(
  onEnded: () => void,
  audienceActive: () => boolean,
  onTime?: (positionMs: number, durationMs: number | null) => void,
): YoutubeWatchHost {
  let inner: YoutubeWatchHost | null = null;
  let starting: Promise<YoutubeWatchHost | null> | null = null;

  const resolve = async (): Promise<YoutubeWatchHost | null> => {
    if (inner) {
      return inner;
    }
    if (!starting) {
      starting = createDefaultYoutubeWatchNativeSurface().then((surface) => {
        if (!surface) {
          return null;
        }
        inner = createTauriYoutubeWatchHost({
          surface,
          audienceActive,
          onEnded,
          onTime,
        });
        return inner;
      });
    }
    return starting;
  };

  const withHost = async (
    run: (host: YoutubeWatchHost) => Promise<void>,
  ): Promise<void> => {
    const host = await resolve();
    if (host) {
      await run(host);
    }
  };

  return {
    load: (watchUrl) => withHost((host) => host.load(watchUrl)),
    play: () => withHost((host) => host.play()),
    pause: () => withHost((host) => host.pause()),
    seek: (ms) => withHost((host) => host.seek(ms)),
    setVolume: (level) => withHost((host) => host.setVolume(level)),
    relayout: () => withHost((host) => host.relayout()),
    teardown: () => withHost((host) => host.teardown()),
  };
}

export function createYoutubeVideoTransport(
  options: CreateYoutubeVideoTransportOptions = {},
): VideoPlaybackTransport {
  let activeId: string | null = null;
  let generation = 0;
  let positionMs = 0;
  let durationMs: number | null = null;
  let volume = 1;
  let playing = false;

  const notifyEnded = () => {
    if (!activeId) {
      return;
    }
    const endedId = activeId;
    playing = false;
    options.onEnded?.(endedId);
  };

  const notifyTime = (
    nextPositionMs: number,
    nextDurationMs: number | null,
  ) => {
    positionMs = nextPositionMs;
    durationMs = nextDurationMs;
    options.onTime?.(nextPositionMs, nextDurationMs);
  };

  const host =
    options.host ??
    createLazyTauriHost(
      notifyEnded,
      options.audienceActive ?? (() => false),
      notifyTime,
    );

  const snapshot = (songId: string | null): PlaybackStateSnapshot =>
    emptySnapshot(songId, generation, playing, positionMs, durationMs, volume);

  return {
    async play(videoId) {
      generation += 1;
      activeId = videoId;
      playing = true;
      positionMs = 0;
      durationMs = null;
      const watchUrl = youtubeWatchUrl(videoId);
      if (watchUrl.includes("/player")) {
        throw new Error("YouTube /player stream URLs are not used");
      }
      await host.load(watchUrl);
      await host.play();
      return snapshot(videoId);
    },
    async pause() {
      playing = false;
      await host.pause();
      return snapshot(activeId);
    },
    async resume() {
      playing = true;
      await host.play();
      return snapshot(activeId);
    },
    async seek(ms) {
      positionMs = Math.max(0, ms);
      await host.seek(positionMs);
      return snapshot(activeId);
    },
    async setVolume(level) {
      volume = level;
      await host.setVolume(level);
      return snapshot(activeId);
    },
    async relayout() {
      await host.relayout();
    },
    async teardown() {
      activeId = null;
      playing = false;
      positionMs = 0;
      durationMs = null;
      await host.teardown();
    },
    isActive() {
      return activeId !== null;
    },
  };
}

export function shouldIgnoreYoutubeChrome(
  songId: string | null | undefined,
): boolean {
  return !!songId && songId.startsWith("yt:");
}

export function shouldRefuseAirPlayLyrics(
  songId: string | null | undefined,
): boolean {
  return shouldIgnoreYoutubeChrome(songId);
}
