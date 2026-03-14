// All types mirror Rust struct serialization exactly.
// Struct fields: snake_case (no rename_all on structs).
// Enum variants: snake_case (via #[serde(rename_all = "snake_case")]).

// ─── Error ───────────────────────────────────────────────

export type ErrorCode =
  | "database_unavailable"
  | "media_read_failed"
  | "song_not_found"
  | "model_unavailable"
  | "audio_decode_failed"
  | "audio_output_unavailable"
  | "karaoke_not_ready"
  | "lyrics_not_ready"
  | "network_unavailable"
  | "invalid_playback_state"
  | "separation_failed"
  | "internal";

export type FallbackAction =
  | "retry"
  | "refresh_library"
  | "reimport_song"
  | "check_audio_output_device"
  | "stay_in_original_mode"
  | "show_empty_state"
  | "keep_current_state";

export interface CommandError {
  code: ErrorCode;
  message: string;
  retryable: boolean;
  fallback: FallbackAction;
}

// ─── Library ─────────────────────────────────────────────

export interface Song {
  hash: string;
  file_path: string;
  title: string | null;
  artist: string | null;
  album: string | null;
  duration_ms: number;
  cover_art: number[] | null;
  imported_at: number;
}

export interface ImportFailure {
  path: string;
  error: CommandError;
}

export interface ImportSongsResult {
  imported: Song[];
  failed: ImportFailure[];
}

// ─── Playback ────────────────────────────────────────────

export type PlaybackMode = "original" | "karaoke";

export interface PlaybackStateSnapshot {
  song_id: string | null;
  is_playing: boolean;
  position_ms: number;
  duration_ms: number | null;
  volume: number;
  mode: PlaybackMode;
}

export interface PlaybackPositionEvent {
  ms: number;
}

// ─── Separation ──────────────────────────────────────────

export type SeparationState = "idle" | "running" | "completed" | "failed";

export interface SeparationStatusSnapshot {
  song_id: string;
  state: SeparationState;
  percent: number;
  cache_hit: boolean;
  vocals_path: string | null;
  accomp_path: string | null;
  error: CommandError | null;
}

export interface SeparationProgressEvent {
  song_id: string;
  percent: number;
}

export interface SeparationCompleteEvent {
  song_id: string;
}

export interface SeparationErrorEvent {
  song_id: string;
  error: CommandError;
}

// ─── Lyrics ──────────────────────────────────────────────

export type LyricsSource = "lrc_lib" | "embedded" | "sidecar";

export interface LyricLine {
  time_ms: number;
  text: string;
}

export interface LyricsPayload {
  song_id: string;
  lines: LyricLine[];
  source: LyricsSource | null;
  offset_ms: number;
}

// ─── Model Bootstrap ────────────────────────────────────

export type ModelBootstrapState =
  | "pending"
  | "downloading"
  | "ready"
  | "failed";

export interface ModelBootstrapStatusSnapshot {
  state: ModelBootstrapState;
  model_path: string;
  downloaded_bytes: number | null;
  total_bytes: number | null;
  error: CommandError | null;
}
