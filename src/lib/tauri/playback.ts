import { invoke } from "@tauri-apps/api/core";
import type {
  AirPlayAudienceStatePayload,
  AirPlayRoutePickerBounds,
  AudioPeakSnapshot,
  PlaybackStateSnapshot,
  StemName,
  WaveformData,
} from "@/types/ipc";

export function play(songId: string): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("play", { songId });
}

export function resume(): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("resume");
}

export function pause(): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("pause");
}

export function seek(ms: number): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("seek", { ms: Math.round(ms) });
}

export function setVolume(level: number): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("set_volume", { level });
}

export function setStemVolume(
  stem: StemName,
  level: number,
): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("set_stem_volume", { stem, level });
}

export function loadStems(): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("load_stems");
}

export function getPlaybackState(): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("get_playback_state");
}

export function getAudioPeaks(): Promise<AudioPeakSnapshot> {
  return invoke<AudioPeakSnapshot>("get_audio_peaks");
}

export async function getWaveform(
  hash: string,
  buckets?: number,
): Promise<WaveformData> {
  const peaks = await invoke<number[]>("get_waveform", { hash, buckets });
  return { peaks, buckets: peaks.length };
}

export function setPreloadCandidate(songId: string | null): Promise<void> {
  return invoke<void>("set_preload_candidate", { songId });
}

export function syncAirPlayRoutePicker(
  bounds: AirPlayRoutePickerBounds | null,
): Promise<void> {
  return invoke<void>("sync_airplay_route_picker", { bounds });
}

export function syncAirPlayAudienceState(
  payload: AirPlayAudienceStatePayload,
): Promise<void> {
  return invoke<void>("sync_airplay_audience_state", { payload });
}

export function stepAirPlayPlainTextPage(
  direction: "prev" | "next",
): Promise<void> {
  return invoke<void>("step_airplay_plain_text_page", { direction });
}
