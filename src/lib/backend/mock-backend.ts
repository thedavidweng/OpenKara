import type { InvokeCommand } from "@/lib/tauri/invoke";
import {
  createTauriMock,
  type MockData,
  type TauriMockHelpers,
} from "@/mock/tauri-mock-impl";
import type { AppSettings, PlaybackStateSnapshot } from "@/types/ipc";
import { createTauriBackend } from "./tauri-backend";
import type { Backend } from "./types";

const DEFAULT_SETTINGS = {
  stem_mode: "four_stem",
  model_variant: "htdemucs",
  language: "en",
  hide_batch_separate: false,
  cover_art_backdrop: true,
  lyrics_blur_inactive: false,
  hide_upgrade_all: false,
  lyrics_font_step: 0,
  execution_provider: "cpu",
  available_execution_providers: ["cpu"],
  compatible_execution_providers: ["cpu"],
  eq_enabled: false,
  eq_gains_db: [0, 0, 0, 0, 0],
  crossfade_enabled: false,
  crossfade_duration_ms: 3_000,
  library_sort_mode: "recently_imported",
  theme_preference: "dark",
  update_policy: "notify",
  youtube_source_enabled: false,
  netease_source_enabled: false,
} satisfies AppSettings;

const DEFAULT_PLAYBACK_SNAPSHOT = {
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
} satisfies PlaybackStateSnapshot;

/**
 * Empty-library defaults. E2E and the website preview pass their own richer
 * fixtures to the same fake; unit tests start from nothing and override the
 * few reads they care about.
 */
export const DEFAULT_MOCK_DATA: MockData = {
  songs: [],
  lyrics: { raw_lrc: "", lines: [], offset_ms: 0, source: "manual" },
  primarySongHash: "",
  sidebarWidth: 280,
  libraryRegistry: { active_library_id: null, libraries: [] },
  activeLibrary: {
    id: "local:/library",
    kind: "local",
    display_name: "Library",
    root_path: "/library",
  },
  libraryPath: "/library",
  windowShellState: {
    chrome_variant: "desktop",
    tier: "desktop",
    toolbar_height_px: 48,
    traffic_light_inset_leading: 0,
    sidebar_header_height_px: 0,
    sidebar_width_px: 280,
  },
  settings: DEFAULT_SETTINGS,
  playbackSnapshot: DEFAULT_PLAYBACK_SNAPSHOT,
  bootstrapStatus: {
    state: "ready",
    model_path: "/models/htdemucs.onnx",
    downloaded_bytes: null,
    total_bytes: null,
    error: null,
  },
  playlists: [],
  playlistSongs: {},
  rotationState: {
    singer_names: [],
    current_index: 0,
    mode: "round_robin",
    active: false,
  },
};

export type BackendOverrides = {
  [Group in keyof Backend]?: Partial<Backend[Group]>;
};

export interface MockBackendOptions {
  data?: MockData;
  overrides?: BackendOverrides;
}

export interface MockBackend extends Backend {
  helpers: TauriMockHelpers;
}

/**
 * A `Backend` whose transport is the in-memory Tauri fake shared with the
 * Playwright fixtures and the website preview. Per-method `overrides` let a
 * test replace only the calls it asserts on without hand-rolling the rest.
 */
export function createMockBackend({
  data = DEFAULT_MOCK_DATA,
  overrides = {},
}: MockBackendOptions = {}): MockBackend {
  const mock = createTauriMock(data);
  const invoke: InvokeCommand = mock.internals.invoke;
  const base = createTauriBackend(invoke);

  return {
    playback: { ...base.playback, ...overrides.playback },
    library: { ...base.library, ...overrides.library },
    librarySetup: { ...base.librarySetup, ...overrides.librarySetup },
    remoteRepository: {
      ...base.remoteRepository,
      ...overrides.remoteRepository,
    },
    settings: { ...base.settings, ...overrides.settings },
    lyrics: { ...base.lyrics, ...overrides.lyrics },
    separation: { ...base.separation, ...overrides.separation },
    maintenance: { ...base.maintenance, ...overrides.maintenance },
    playlist: { ...base.playlist, ...overrides.playlist },
    cdg: { ...base.cdg, ...overrides.cdg },
    catalog: { ...base.catalog, ...overrides.catalog },
    helpers: mock.helpers,
  };
}
