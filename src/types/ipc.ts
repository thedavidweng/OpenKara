export type ErrorCode =
  | "database_unavailable"
  | "remote_repository_unavailable"
  | "media_read_failed"
  | "song_not_found"
  | "model_unavailable"
  | "audio_decode_failed"
  | "audio_output_unavailable"
  | "karaoke_not_ready"
  | "lyrics_not_ready"
  | "network_unavailable"
  | "invalid_playback_state"
  | "execution_provider_unavailable"
  | "runtime_post_download_timeout"
  | "separation_failed"
  | "online_source_disabled"
  | "streaming_session_expired"
  | "video_source_unavailable"
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

export type CoverArtBytes = number[] | Uint8Array | ArrayBuffer | null;

export type CoverArtSize = "thumb" | "preview" | "original";

export type RemoteLibraryProvider = "google_drive" | "dropbox" | "webdav";

export type OnlineSourceId = "youtube" | "netease";

export type OnlineSourceKind = "video" | "streaming";

export interface OnlineSourceCapabilities {
  sign_in: boolean;
  browse: boolean;
  import: boolean;
  resolve_video: boolean;
}

export interface OnlineSourceSnapshot {
  id: OnlineSourceId;
  kind: OnlineSourceKind;
  enabled: boolean;
  capabilities: OnlineSourceCapabilities;
}

export interface StreamingSessionSnapshot {
  source_id: OnlineSourceId;
  signed_in: boolean;
  display_name: string | null;
  expired: boolean;
}

export interface StreamingQrChallenge {
  key: string;
  login_url: string;
  qr_svg: string;
}

export type StreamingQrStatus = "waiting" | "scanned" | "confirmed" | "expired";

export interface StreamingQrPoll {
  status: StreamingQrStatus;
  session: StreamingSessionSnapshot | null;
}

export type StreamingPasswordMethod = "phone" | "email";

export type YoutubeWatchAction =
  | { type: "play" }
  | { type: "pause" }
  | { type: "seek"; ms: number }
  | { type: "set_volume"; level: number }
  | { type: "query" }
  | { type: "navigate"; url: string };

export interface YoutubeWatchMediaState {
  ended: boolean;
  paused: boolean;
  current_time_ms: number;
  duration_ms: number | null;
}

export type ImportRefusalReason = "no_play_rights" | "trial_clip" | "empty_url";

export interface ImportRefusal {
  reason: ImportRefusalReason;
  title: string;
  artist: string;
}

export interface StreamingTrack {
  source_id: OnlineSourceId;
  remote_track_id: string;
  title: string;
  artist: string;
  album: string | null;
  duration_ms: number | null;
  refusal: ImportRefusal | null;
}

export interface StreamingPlaylistSummary {
  remote_id: string;
  name: string;
  track_count: number;
}

export interface StreamingPlaylistDetail {
  remote_id: string;
  name: string;
  tracks: StreamingTrack[];
}

export interface LibraryDecisionMeta {
  title: string | null;
  artist: string | null;
  album: string | null;
  format: string;
  bit_rate_bps: number | null;
  duration_ms: number | null;
  file_size_bytes: number;
}

export interface ImportConflictPrompt {
  source_id: OnlineSourceId;
  remote_track_id: string;
  library: LibraryDecisionMeta;
  incoming: LibraryDecisionMeta;
}

export type LibraryDecisionAction =
  | "keep"
  | "replace"
  | "apply_keep"
  | "apply_replace"
  | "cancel";

export type StreamingImportFailureReason =
  | "refusal"
  | "cancelled"
  | "import_failed";

export interface StreamingImportFailure {
  remote_track_id: string;
  title: string;
  artist: string;
  reason: StreamingImportFailureReason;
  refusal: ImportRefusal | null;
}

export type StreamingImportStatus = "awaiting_decision" | "completed";

export interface StreamingImportProgress {
  status: StreamingImportStatus;
  imported_song_ids: string[];
  failed: StreamingImportFailure[];
  playlist_id: string | null;
  conflict: ImportConflictPrompt | null;
}

export interface VideoQueueItem {
  id: string;
  title: string;
  channel: string;
  duration_ms: number | null;
  thumbnail_url: string | null;
  watch_url: string;
}

export type VideoUnavailableReason =
  | "invalid_url"
  | "age_restricted"
  | "private"
  | "unlisted"
  | "unavailable";

export interface RevealTarget {
  available: boolean;
  path: string | null;
}

export interface RevealTargets {
  song_file: RevealTarget;
  stems: RevealTarget;
}

export interface WebDavRemoteAuthPayload {
  type: "webdav";
  server_url: string;
  username: string;
  password: string;
  root_path: string | null;
}

export type RemoteAuthPayload = WebDavRemoteAuthPayload | null;

export interface RemoteAuthStart {
  session_id: string;
  provider: RemoteLibraryProvider;
  authorization_url: string | null;
  expires_at_ms: number | null;
}

export type RemoteAuthState = "pending" | "ready" | "failed";

export interface RemoteAuthStatus {
  session_id: string;
  provider: RemoteLibraryProvider;
  state: RemoteAuthState;
  remote_root_locator: string | null;
  display_name: string | null;
  error: CommandError | null;
}

export interface RemoteLibraryCandidate {
  provider: RemoteLibraryProvider;
  remote_root_locator: string;
  remote_path_display: string;
  display_name: string;
  account_id: string | null;
}

export interface LocalLibraryRegistration {
  id: string;
  kind: "local";
  display_name: string;
  root_path: string;
}

export interface RemoteLibraryRegistration {
  id: string;
  kind: "remote";
  display_name: string;
  provider: RemoteLibraryProvider;
  remote_root_locator: string;
  remote_path_display: string;
  account_id: string;
  connection_config: RemoteLibraryConnectionConfig | null;
  cached_db_path: string | null;
  remote_revision: string | null;
}

export type RemoteLibraryConnectionConfig =
  | {
      type: "google_drive";
      oauth_client_id: string;
    }
  | {
      type: "dropbox";
      app_key: string;
    }
  | {
      type: "webdav";
      server_url: string;
    };

export type RegisteredLibrary =
  | LocalLibraryRegistration
  | RemoteLibraryRegistration;

export interface LibraryRegistrySnapshot {
  active_library_id: string | null;
  libraries: RegisteredLibrary[];
}

export interface Song {
  hash: string;
  file_path: string | null;
  audio_source_kind: "original" | "original_remote" | "stems_remote";
  cdg_path: string | null;
  media_g_container: "paired" | "zip" | null;
  instrumental: boolean;
  language: string | null;
  title: string | null;
  artist: string | null;
  album: string | null;
  duration_ms: number;
  cover_art: CoverArtBytes;
  has_cover_art: boolean;
  artwork_thumb_path: string | null;
  imported_at: number;
  original_ext: string | null;
}

export interface SongProperties {
  format: string;
  sample_rate_hz: number | null;
  channels: number | null;
  bit_rate_bps: number | null;
  file_size_bytes: number;
  duration_ms: number;
  hash: string;
}

export interface ImportFailure {
  path: string;
  error: CommandError;
}

export interface ImportSongsResult {
  imported: Song[];
  failed: ImportFailure[];
}

export interface ExpandedImportPaths {
  paths: string[];
  song_count: number;
}

export interface ImportSongsOptions {
  explicit_cdg_by_audio_path: Record<string, string>;
  skip_cdg_for_audio_paths?: string[];
}

export interface ImportCandidateDetails {
  path: string;
  format: string;
  bit_rate_bps: number | null;
  file_size_bytes: number;
  duration_ms: number | null;
}

export interface DeleteSongsFailure {
  song_id: string;
  error: CommandError;
}

export interface DeleteSongsResult {
  deleted_song_ids: string[];
  failed: DeleteSongsFailure[];
}

export interface ExtractEmbeddedCoverArtFailure {
  song_id: string;
  error: CommandError;
}

export interface ExtractEmbeddedCoverArtResult {
  updated_songs: Song[];
  failed: ExtractEmbeddedCoverArtFailure[];
}

export interface LyricsMatch {
  song_id: string;
  lrc_path: string;
  song_title: string | null;
  song_artist: string | null;
}

export interface ImportLyricsResult {
  matched: LyricsMatch[];
  unmatched: string[];
}

export type StemMode = "two_stem" | "four_stem";
export type ModelVariant = "htdemucs" | "htdemucs_ft";
export type ExecutionProvider = "cpu" | "xnnpack" | "coreml" | "directml";
export type LibrarySortMode = "recently_imported" | "title_asc" | "artist_asc";
export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";
export type UpdatePolicy = "manual" | "notify" | "auto_download";

export interface AppSettings {
  stem_mode: StemMode;
  model_variant: ModelVariant;
  language: string | null;
  hide_batch_separate: boolean;
  cover_art_backdrop: boolean;
  lyrics_blur_inactive: boolean;
  hide_upgrade_all: boolean;
  lyrics_font_step: number;
  execution_provider: ExecutionProvider;
  available_execution_providers: ExecutionProvider[];
  compatible_execution_providers: ExecutionProvider[];
  eq_enabled: boolean;
  eq_gains_db: [number, number, number, number, number];
  crossfade_enabled: boolean;
  crossfade_duration_ms: number;
  library_sort_mode: LibrarySortMode;
  theme_preference: ThemePreference;
  update_policy: UpdatePolicy;
  youtube_source_enabled: boolean;
  netease_source_enabled: boolean;
}

export interface DebugInfo {
  app_version: string;
  build_sha: string;
  os: string;
  arch: string;
  catalog_generation: number;
  catalog_release_id: string;
  model_variant: string;
  model_state: string;
  model_installed: boolean;
  model_installed_version: string | null;
  model_pinned_version: string;
  model_path: string;
  runtime_state: string;
  runtime_version: string;
  runtime_artifact_id: string | null;
  runtime_target_triple: string;
  runtime_path: string;
  execution_provider: string;
  directml_available: boolean;
  language: string | null;
  log_file: string;
}

export interface ModelStatusSnapshot {
  variant: string;
  downloaded: boolean;
  legacy_install_present: boolean;
  file_size_bytes: number | null;
  installed_version: string | null;
  pinned_version: string;
}

export type ModelUpdateState =
  | "not_installed"
  | "up_to_date"
  | "update_available"
  | "installed_without_identity";

export interface ModelUpdateCheckSnapshot {
  variant: string;
  state: ModelUpdateState;
  installed_version: string | null;
  available_version: string;
  available_bytes: number;
}

export interface ModelUpdateReport {
  generation: number;
  release_id: string;
  models: ModelUpdateCheckSnapshot[];
}

export type WindowShellChromeVariant = "desktop" | "mac";
export type WindowShellTier = "desktop" | "mac";

export interface WindowShellStateSnapshot {
  chrome_variant: WindowShellChromeVariant;
  tier: WindowShellTier;
  toolbar_height_px: number;
  traffic_light_inset_leading: number;
  sidebar_header_height_px: number;
  sidebar_width_px: number;
}

export type StemName = "vocals" | "drums" | "bass" | "other";
export type PlaybackTransportState =
  | "idle"
  | "loading"
  | "playing"
  | "buffering";

export interface StemVolumes {
  vocals: number;
  drums: number;
  bass: number;
  other: number;
}

export interface PlaybackStateSnapshot {
  song_id: string | null;
  transport_generation: number;
  state: PlaybackTransportState;
  is_playing: boolean;
  position_ms: number;
  duration_ms: number | null;
  buffered_ms: number;
  volume: number;
  stem_volumes: StemVolumes;
  has_stems: boolean;
  stem_mode: StemMode | null;
}

export interface PlaybackPositionEvent {
  ms: number;
  transport_generation: number;
  snapshot: PlaybackStateSnapshot;
}

export interface PlaybackEndedEvent {
  song_id: string;
}

export interface TrackTransitionedEvent {
  transition_serial: number;
  from_song_id: string;
  to_song_id: string;
}

export interface AudioPeakSnapshot {
  writeIndex: number;
  peaks: Array<[left: number, right: number]>;
}

export interface WaveformData {
  peaks: number[];
  buckets: number;
}

export interface PlaybackErrorEvent {
  song_id: string;
  error: CommandError;
}

export interface AirPlayRoutePickerBounds {
  left_px: number;
  top_px: number;
  width_px: number;
  height_px: number;
}

export type AirPlayAudienceMode = "idle" | "lyrics" | "cdg";
export type AirPlayOutputPhase =
  | "idle"
  | "route_selected"
  | "buffering"
  | "playing"
  | "failed";

export interface AirPlayViewport {
  width_px: number;
  height_px: number;
  bottom_inset_px: number;
}

export interface AirPlayColor {
  red: number;
  green: number;
  blue: number;
  alpha: number;
}

export interface AudiencePresentationSpec {
  contentWidthRatio: number;
  contentMaxWidthPx: number;
  horizontalPaddingPx: number;
  verticalPaddingPx: number;
  lineGapPx: number;
  fontSizePx: number;
  lineHeightMultiple: number;
  activeScale: number;
  statusFontSizePx: number;
  activeGlowBlurPx: number;
  activeTextColor: AirPlayColor;
  pastTextColor: AirPlayColor;
  futureTextColor: AirPlayColor;
  plainTextColor: AirPlayColor;
  statusTextColor: AirPlayColor;
  activeGlowColor: AirPlayColor;
}

export interface AirPlayAudienceMessages {
  selectSong: string;
  loadingLyrics: string;
  noLyrics: string;
  addLyrics: string;
}

export interface AirPlayAudienceStatePayload {
  mode: AirPlayAudienceMode;
  songId: string | null;
  lines: LyricLine[];
  offsetMs: number;
  isLoading: boolean;
  lyricsFontStep: number;
  messages: AirPlayAudienceMessages;
  viewport: AirPlayViewport;
  presentationSpec: AudiencePresentationSpec;
}

export interface AirPlayOutputStateEvent {
  active: boolean;
  audioActive: boolean;
  routeName: string | null;
  mode: AirPlayAudienceMode;
  phase: AirPlayOutputPhase;
  detail: string | null;
  displayedPositionMs: number | null;
  streamGeneration: number;
  latencyMs: number | null;
}

export type SeparationState = "idle" | "running" | "completed" | "failed";

export interface SeparationStatusSnapshot {
  song_id: string;
  state: SeparationState;
  percent: number;
  cache_hit: boolean;
  vocals_path: string | null;
  accomp_path: string | null;
  drums_path: string | null;
  bass_path: string | null;
  other_path: string | null;
  model_variant: string | null;
  error: CommandError | null;
}

export interface SeparationProgressEvent {
  song_id: string;
  percent: number;
}

export interface SeparationCompleteEvent {
  song_id: string;
  status: SeparationStatusSnapshot;
}

export interface SeparationErrorEvent {
  song_id: string;
  error: CommandError;
}

export interface SeparationCancelledEvent {
  song_id: string;
}

export type UploadState = "idle" | "running" | "completed" | "failed";

export interface UploadStatusSnapshot {
  song_id: string;
  state: UploadState;
  percent: number;
  remote_library_id?: string | null;
  detail?: string | null;
  error: CommandError | null;
}

export interface UploadProgressEvent {
  song_id: string;
  percent: number;
  remote_library_id?: string | null;
  detail?: string | null;
}

export interface UploadCompleteEvent {
  song_id: string;
  remote_library_id?: string | null;
}

export interface UploadErrorEvent {
  song_id: string;
  remote_library_id?: string | null;
  error: CommandError;
}

export type LyricsSource =
  | "lrc_lib"
  | "lrc_api"
  | "lrc_api_ttml"
  | "amll"
  | "embedded"
  | "sidecar"
  | "sidecar_ttml"
  | "sidecar_lys"
  | "manual"
  | "manual_ttml"
  | "manual_lys";

export type LyricsOnlineFetchIntent = "automatic_upgrade" | "user_replace";

export interface WordToken {
  time_ms: number;
  end_ms: number;
  text: string;
  roman?: string | null;
}

export interface LyricLine {
  time_ms: number;
  text: string;
  words: WordToken[] | null;
  bg_words: WordToken[] | null;
  section: string | null;
  roman: string | null;
}

export interface LyricsPayload {
  song_id: string;
  lines: LyricLine[];
  source: LyricsSource | null;
  offset_ms: number;
  raw_lrc: string;
}

export interface DeleteStemsResult {
  deleted_count: number;
  freed_bytes: number;
}

export interface DowngradeResult {
  downgraded_count: number;
  freed_bytes: number;
}

export interface BatchSeparationProgress {
  total: number;
  completed: number;
  skipped: number;
  failed: number;
  current_song_id: string | null;
  current_percent: number;
}

export type ModelBootstrapState =
  | "pending"
  | "downloading"
  | "outdated"
  | "ready"
  | "failed";

export interface ModelBootstrapStatusSnapshot {
  state: ModelBootstrapState;
  model_path: string;
  downloaded_bytes: number | null;
  total_bytes: number | null;
  error: CommandError | null;
}

export type RuntimeBootstrapState =
  | "missing"
  | "downloading"
  | "installing"
  | "probing"
  | "activating"
  | "ready"
  | "update_available"
  | "downloading_candidate"
  | "candidate_ready_restart_required"
  | "activation_failed_previous_restored"
  | "corrupt"
  | "failed";

export type RuntimeBootstrapFailurePhase =
  | "download"
  | "install"
  | "probe"
  | "activate";

export interface RuntimeBootstrapStatusSnapshot {
  state: RuntimeBootstrapState;
  runtime_path: string;
  downloaded_bytes: number | null;
  total_bytes: number | null;
  version: string;
  active_artifact_id: string | null;
  target_triple: string;
  candidate_version: string | null;
  restart_required: boolean;
  error: CommandError | null;
  failure_phase?: RuntimeBootstrapFailurePhase | null;
  cpu_fallback_notice?: string | null;
}

export interface RuntimeUpdateReport {
  generation: number;
  release_id: string;
  target_triple: string;
  state: ModelUpdateState;
  installed_version: string | null;
  available_version: string;
  available_bytes: number;
  restart_required: boolean;
}

export interface ManagedAssetIssue {
  song_hash: string;
  asset_type: string;
  path: string;
}

export interface IntegrityReport {
  checked_local_songs: number;
  skipped_remote_songs: number;
  missing_primary_media: ManagedAssetIssue[];
  empty_primary_media: ManagedAssetIssue[];
  missing_optional_assets: ManagedAssetIssue[];
  empty_optional_assets: ManagedAssetIssue[];
  orphaned_managed_files: string[];
}

export interface IntegrityCleanupResult {
  deleted_song_hashes: string[];
  skipped_song_hashes: string[];
}

export interface CacheUsage {
  used_bytes: number;
  limit_bytes: number;
  entry_count: number;
  pinned_count: number;
}

export interface RemoteOperationDiagnostic {
  operation_id: string;
  operation_kind: string;
  state: string;
  expected_generation: number | null;
  target_generation: number | null;
  attempt_count: number;
  error_code: string | null;
  error_detail: string | null;
  created_at_ms: number;
  updated_at_ms: number;
}

export interface RemoteDiagnostics {
  has_active_remote: boolean;
  repository_id: string | null;
  writer_id: string | null;
  committed_generation: number;
  local_base_generation: number;
  local_state: string;
  local_db_digest: string | null;
  active_operation_id: string | null;
  last_success_at_ms: number | null;
  last_error_code: string | null;
  recent_operations: RemoteOperationDiagnostic[];
}

export interface RemotePlaybackReconnectEvent {
  song_id: string;
  request_id: number;
  attempt: number;
  max_attempts: number;
  reason: string;
}

export interface RemotePlaybackResyncEvent {
  song_id: string;
  requested_position_ms: number;
  actual_position_ms: number;
}

export interface RemotePlaybackFailedEvent {
  song_id: string;
  request_id: number;
  reason: string;
}
