// Mock data payload for the shared Tauri IPC mock.  Both the website preview
// and the Playwright E2E fixture import this object and pass it to
// `createTauriMock()`.  Song and lyrics data is sourced from
// `preview-songs.ts` so the mock catalog stays in sync with the generator
// script.
//
// The E2E fixture serializes this object via `JSON.stringify()` and injects
// it alongside the `createTauriMock` function source.  All fields must be
// JSON-serializable (no Uint8Array, no functions, no undefined).

import {
  PREVIEW_LYRICS,
  PREVIEW_SONGS,
  PRIMARY_PREVIEW_SONG_HASH,
} from "@/mock/preview-songs";
import type { MockData } from "@/mock/tauri-mock-impl";

/**
 * Sidebar width returned by the mock's `get_window_shell_state`. The app
 * applies this at runtime via `--window-shell-sidebar-width`, so E2E
 * geometry helpers must use the same value when deriving the playback-bar
 * container width (viewport minus sidebar). Exported so specs stay in sync
 * with the mock instead of hard-coding a separate copy.
 */
export const MOCK_SIDEBAR_WIDTH = 280;

export const MOCK_PLAYLIST_SONGS: Record<string, string[]> = {
  favorites: ["earfquake", "see-you-again", "counting-stars", "feel-good-inc"],
  party: ["all-the-love", "three-empty-words", "earfquake", "see-you-again"],
};

/**
 * Playlists for the website preview.  `song_count` must match membership in
 * {@link MOCK_PLAYLIST_SONGS} — the preview seeds the playlist store directly
 * and never re-counts via IPC.  E2E tests use an empty list — tests that need
 * playlists create them via the mock IPC's `create_playlist` command at runtime.
 */
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

/**
 * Freeze the website mock on this Earfquake lyric timestamp so the seekbar
 * and lyrics panel show a real mid-song line instead of an idle blank state.
 * Matches `Don't leave, it's my fault (Girl)` in `PREVIEW_LYRICS.earfquake`.
 */
export const PREVIEW_FROZEN_POSITION_MS = 59560;

const PRIMARY_PREVIEW_DURATION_MS =
  PREVIEW_SONGS.find((song) => song.hash === PRIMARY_PREVIEW_SONG_HASH)
    ?.duration_ms ?? 0;

/**
 * The shared mock data payload.  Both the website preview and E2E fixture
 * use this as the base; E2E may override fields (e.g. set `playlists` to
 * `[]`) before serialization.
 */
export const MOCK_DATA: MockData = {
  // Songs — serialize Uint8Array cover_art to number[] for the IPC contract.
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
    toolbar_height: 48,
    traffic_light_inset_leading: 0,
    sidebar_header_height: 0,
    sidebar_width: MOCK_SIDEBAR_WIDTH,
  },

  settings: {
    stem_mode: "two_stem",
    model_variant: "htdemucs",
    language: "en",
    hide_batch_separate: false,
    cover_art_backdrop: false,
    lyrics_font_step: 0,
    execution_provider: "cpu",
    available_execution_providers: ["cpu"],
    eq_enabled: false,
    eq_gains_db: [0, 0, 0, 0, 0],
    crossfade_enabled: false,
    crossfade_duration_ms: 3000,
    library_sort_mode: "recently_imported",
    theme_preference: "dark",
  },

  // Website preview freezes on the primary song at a real lyric timestamp so
  // the seekbar and lyrics panel look mid-session. E2E overrides this back to
  // idle via {@link E2E_MOCK_DATA} so playback specs start from a clean slate.
  playbackSnapshot: {
    transport_generation: 0,
    song_id: PRIMARY_PREVIEW_SONG_HASH,
    // Pause is `is_playing: false` with a non-idle transport state — keeps the
    // player chrome populated while the clock stays frozen for the mock.
    state: "playing",
    is_playing: false,
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
};

/**
 * E2E-specific mock data: same as {@link MOCK_DATA} but with empty playlists
 * (E2E tests create playlists at runtime via mock IPC commands) and an idle
 * playback snapshot so specs control transport from a known starting state.
 */
export const E2E_MOCK_DATA: MockData = {
  ...MOCK_DATA,
  playlists: [],
  playlistSongs: {},
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
