import type { PlaybackBackend } from "@/lib/backend/types";
import type {
  AudioPeakSnapshot,
  PlaybackStateSnapshot,
  WaveformData,
} from "@/types/ipc";
import type { InvokeCommand } from "./invoke";

export function createPlaybackCommands(invoke: InvokeCommand): PlaybackBackend {
  return {
    play: (songId) => invoke<PlaybackStateSnapshot>("play", { songId }),

    resume: () => invoke<PlaybackStateSnapshot>("resume"),

    pause: () => invoke<PlaybackStateSnapshot>("pause"),

    seek: (ms) => invoke<PlaybackStateSnapshot>("seek", { ms: Math.round(ms) }),

    setVolume: (level) =>
      invoke<PlaybackStateSnapshot>("set_volume", { level }),

    setStemVolume: (stem, level) =>
      invoke<PlaybackStateSnapshot>("set_stem_volume", { stem, level }),

    loadStems: () => invoke<PlaybackStateSnapshot>("load_stems"),

    getPlaybackState: () => invoke<PlaybackStateSnapshot>("get_playback_state"),

    getAudioPeaks: () => invoke<AudioPeakSnapshot>("get_audio_peaks"),

    getWaveform: async (hash, buckets): Promise<WaveformData> => {
      const peaks = await invoke<number[]>("get_waveform", { hash, buckets });
      return { peaks, buckets: peaks.length };
    },

    setPreloadCandidate: (songId) =>
      invoke<void>("set_preload_candidate", { songId }),

    syncAirPlayRoutePicker: (bounds) =>
      invoke<void>("sync_airplay_route_picker", { bounds }),

    syncAirPlayAudienceState: (payload) =>
      invoke<void>("sync_airplay_audience_state", { payload }),

    stepAirPlayPlainTextPage: (direction) =>
      invoke<void>("step_airplay_plain_text_page", { direction }),
  };
}
