import { invoke } from "@tauri-apps/api/core";
import type {
  ImportSongsResult,
  LyricsPayload,
  ModelBootstrapStatusSnapshot,
  PlaybackMode,
  PlaybackStateSnapshot,
  SeparationStatusSnapshot,
  Song,
} from "@/types/ipc";

// ─── Library ─────────────────────────────────────────────

export function importSongs(paths: string[]): Promise<ImportSongsResult> {
  return invoke<ImportSongsResult>("import_songs", { paths });
}

export function getLibrary(): Promise<Song[]> {
  return invoke<Song[]>("get_library");
}

export function searchLibrary(query: string): Promise<Song[]> {
  return invoke<Song[]>("search_library", { query });
}

// ─── Playback ────────────────────────────────────────────

export function play(songId: string): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("play", { songId });
}

export function pause(): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("pause");
}

export function seek(ms: number): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("seek", { ms });
}

export function setVolume(level: number): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("set_volume", { level });
}

export function setPlaybackMode(
  mode: PlaybackMode,
): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("set_playback_mode", { mode });
}

export function getPlaybackState(): Promise<PlaybackStateSnapshot> {
  return invoke<PlaybackStateSnapshot>("get_playback_state");
}

// ─── Separation ──────────────────────────────────────────

export function separate(songId: string): Promise<SeparationStatusSnapshot> {
  return invoke<SeparationStatusSnapshot>("separate", { songId });
}

export function getSeparationStatus(
  songId: string,
): Promise<SeparationStatusSnapshot> {
  return invoke<SeparationStatusSnapshot>("get_separation_status", { songId });
}

// ─── Lyrics ──────────────────────────────────────────────

export function fetchLyrics(songId: string): Promise<LyricsPayload> {
  return invoke<LyricsPayload>("fetch_lyrics", { songId });
}

export function setLyricsOffset(songId: string, ms: number): Promise<void> {
  return invoke<void>("set_lyrics_offset", { songId, ms });
}

// ─── Bootstrap ───────────────────────────────────────────

export function getModelBootstrapStatus(): Promise<ModelBootstrapStatusSnapshot> {
  return invoke<ModelBootstrapStatusSnapshot>("get_model_bootstrap_status");
}
