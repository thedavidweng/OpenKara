import {
  PREVIEW_LYRICS,
  PREVIEW_SONGS,
  PRIMARY_PREVIEW_SONG_HASH,
} from "@/mock/preview-songs";
import type { MockData } from "@/mock/tauri-mock-impl";

export const MOCK_SIDEBAR_WIDTH = 280;

export const MOCK_PLAYLIST_SONGS: Record<string, string[]> = {
  favorites: ["earfquake", "see-you-again", "counting-stars", "feel-good-inc"],
  party: ["all-the-love", "three-empty-words", "earfquake", "see-you-again"],
};

export const MOCK_PLAYLISTS = [
  {
    id: "favorites",
    name: "Favorites",
    song_count: MOCK_PLAYLIST_SONGS.favorites.length,
    created_at: 1,
    updated_at: 1,
  },
  {
    id: "party",
    name: "Friday night",
    song_count: MOCK_PLAYLIST_SONGS.party.length,
    created_at: 1,
    updated_at: 1,
  },
];

export const PREVIEW_FROZEN_POSITION_MS = 59560;

const PRIMARY_PREVIEW_DURATION_MS =
  PREVIEW_SONGS.find((song) => song.hash === PRIMARY_PREVIEW_SONG_HASH)
    ?.duration_ms ?? 0;

export const MOCK_DATA: MockData = {
  songs: PREVIEW_SONGS.map(({ mbid: _mbid, cover_art, ...rest }) => ({
    ...rest,
    cover_art: Array.from(cover_art),
  })),

  lyrics: PREVIEW_LYRICS[PRIMARY_PREVIEW_SONG_HASH],

  primarySongHash: PRIMARY_PREVIEW_SONG_HASH,

  sidebarWidth: MOCK_SIDEBAR_WIDTH,

  libraryRegistry: {
    active_library_id: "mock-lib-1",
    libraries: [
      {
        id: "mock-lib-1",
        kind: "local",
        display_name: "Test Library",
        root_path: "/tmp/openkara-test-lib",
      },
    ],
  },

  activeLibrary: {
    id: "mock-lib-1",
    kind: "local",
    display_name: "Test Library",
    root_path: "/tmp/openkara-test-lib",
  },

  libraryPath: "/tmp/openkara-test-lib",

  windowShellState: {
    chrome_variant: "desktop",
    tier: "desktop",
    toolbar_height_px: 48,
    traffic_light_inset_leading: 0,
    sidebar_header_height_px: 0,
    sidebar_width_px: MOCK_SIDEBAR_WIDTH,
  },

  settings: {
    // E2E fixtures pin two_stem: the Playwright geometry specs cover the
    // two-stem slider layout. Four-stem is the product default (#182).
    stem_mode: "two_stem",
    model_variant: "htdemucs",
    language: "en",
    hide_batch_separate: false,
    cover_art_backdrop: false,
    hide_upgrade_all: false,
    lyrics_font_step: 0,
    execution_provider: "cpu",
    available_execution_providers: ["cpu"],
    compatible_execution_providers: ["cpu"],
    eq_enabled: false,
    eq_gains_db: [0, 0, 0, 0, 0],
    crossfade_enabled: false,
    crossfade_duration_ms: 3000,
    library_sort_mode: "recently_imported",
    theme_preference: "dark",
    update_policy: "notify",
  },

  playbackSnapshot: {
    transport_generation: 0,
    song_id: PRIMARY_PREVIEW_SONG_HASH,
    state: "playing",
    is_playing: true,
    position_ms: PREVIEW_FROZEN_POSITION_MS,
    duration_ms: PRIMARY_PREVIEW_DURATION_MS,
    buffered_ms: PRIMARY_PREVIEW_DURATION_MS,
    volume: 0.8,
    stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
    has_stems: true,
    stem_mode: "two_stem",
  },

  bootstrapStatus: {
    state: "ready",
    model_path: "/tmp/model.onnx",
    downloaded_bytes: null,
    total_bytes: null,
    variant: "htdemucs",
  },

  playlists: MOCK_PLAYLISTS,
  playlistSongs: MOCK_PLAYLIST_SONGS,

  rotationState: {
    singer_names: [],
    current_index: 0,
    mode: "round_robin",
    active: false,
  },

  loopPlayback: true,
  loopStartPositionMs: 0,
};

export const E2E_MOCK_DATA: MockData = {
  ...MOCK_DATA,
  playlists: [],
  playlistSongs: {},
  loopPlayback: false,
  playbackSnapshot: {
    transport_generation: 0,
    song_id: null,
    state: "idle",
    is_playing: false,
    position_ms: 0,
    duration_ms: 0,
    buffered_ms: 0,
    volume: 0.8,
    stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
    has_stems: false,
    stem_mode: null,
  },
};
