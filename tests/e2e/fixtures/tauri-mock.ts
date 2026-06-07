/**
 * Mock for Tauri IPC used by Playwright E2E tests.
 *
 * In a real Tauri build the Rust backend owns the database, audio pipeline,
 * and filesystem.  During browser-based E2E runs (against the Vite dev
 * server) none of that exists, so we stub `window.__TAURI_INTERNALS__` --
 * the single entry-point that every `invoke()` call from
 * `@tauri-apps/api/core` funnels through.
 *
 * The mock is injected via `page.addInitScript()` *before* the app bundle
 * executes, so it must be self-contained (no imports).
 */

// This script text is passed to page.addInitScript().
// It is intentionally self-contained and contains no imports or
// JSON.stringify of functions (which would be stripped).
export const TAURI_MOCK_SCRIPT = `
(() => {
  // ── Fixture songs ──
  const MOCK_SONGS = [
    {
      hash: "aaa111", file_path: "/music/Bohemian_Rhapsody.mp3",
      audio_source_kind: "original", cdg_path: null, media_g_container: null,
      instrumental: false, language: "en",
      title: "Bohemian Rhapsody", artist: "Queen", album: "A Night at the Opera",
      duration_ms: 354000, cover_art: null, imported_at: Date.now(), original_ext: ".mp3",
    },
    {
      hash: "bbb222", file_path: "/music/Hotel_California.mp3",
      audio_source_kind: "original", cdg_path: null, media_g_container: null,
      instrumental: false, language: "en",
      title: "Hotel California", artist: "Eagles", album: "Hotel California",
      duration_ms: 391000, cover_art: null, imported_at: Date.now(), original_ext: ".mp3",
    },
    {
      hash: "ccc333", file_path: "/music/Imagine.mp3",
      audio_source_kind: "original", cdg_path: null, media_g_container: null,
      instrumental: false, language: "en",
      title: "Imagine", artist: "John Lennon", album: "Imagine",
      duration_ms: 187000, cover_art: null, imported_at: Date.now(), original_ext: ".mp3",
    },
  ];

  // ── Command response table ──
  // Static payloads for most commands; functions for context-sensitive ones.
  const COMMANDS = {
    // Library registry & settings
    get_library_registry: {
      active_library_id: "mock-lib-1",
      libraries: [{
        id: "mock-lib-1", kind: "local",
        display_name: "Test Library", root_path: "/tmp/openkara-test-lib",
      }],
    },
    get_settings: {
      stem_mode: "two_stem", model_variant: "htdemucs", language: "en",
      hide_batch_separate: false, cover_art_backdrop: false,
      lyrics_font_step: 0, execution_provider: "cpu",
      available_execution_providers: ["cpu"],
    },
    get_window_shell_state: {
      chrome_variant: "desktop", tier: "desktop", toolbar_height: 48,
      traffic_light_inset_leading: 0, sidebar_header_height: 0, sidebar_width: 280,
    },
    get_library_path: "/tmp/openkara-test-lib",

    // Library songs
    get_active_library: {
      id: "mock-lib-1", kind: "local",
      display_name: "Test Library", root_path: "/tmp/openkara-test-lib",
    },
    get_library: MOCK_SONGS,
    search_library: (args) => {
      const q = ((args && args.query) || "").toLowerCase();
      if (!q) return MOCK_SONGS;
      return MOCK_SONGS.filter(
        (s) => s.title.toLowerCase().includes(q) || s.artist.toLowerCase().includes(q)
      );
    },
    get_all_separation_statuses: {},
    get_all_upload_statuses: {},

    // Playback
    get_playback_state: {
      song_id: null, state: "idle", is_playing: false,
      position_ms: 0, duration_ms: 0, buffered_ms: 0, volume: 0.8,
      stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
      has_stems: false, stem_mode: null,
    },
    play: (args) => {
      const song = MOCK_SONGS.find((s) => s.hash === (args && args.song_id));
      return {
        song_id: (args && args.song_id) || "aaa111",
        state: "playing", is_playing: true, position_ms: 0,
        duration_ms: song ? song.duration_ms : 300000, buffered_ms: 0, volume: 0.8,
        stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
        has_stems: false, stem_mode: null,
      };
    },
    resume: {
      song_id: "aaa111", state: "playing", is_playing: true,
      position_ms: 0, duration_ms: 354000, buffered_ms: 0, volume: 0.8,
      stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
      has_stems: false, stem_mode: null,
    },
    pause: {
      song_id: "aaa111", state: "idle", is_playing: false,
      position_ms: 5000, duration_ms: 354000, buffered_ms: 0, volume: 0.8,
      stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
      has_stems: false, stem_mode: null,
    },
    seek: (args) => ({
      song_id: "aaa111", state: "playing", is_playing: true,
      position_ms: (args && args.ms) || 0, duration_ms: 354000, buffered_ms: 0, volume: 0.8,
      stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
      has_stems: false, stem_mode: null,
    }),
    set_volume: (args) => ({
      song_id: "aaa111", state: "playing", is_playing: true,
      position_ms: 0, duration_ms: 354000, buffered_ms: 0,
      volume: (args && args.level) || 0.8,
      stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
      has_stems: false, stem_mode: null,
    }),
    set_stem_volume: {
      song_id: "aaa111", state: "playing", is_playing: true,
      position_ms: 0, duration_ms: 354000, buffered_ms: 0, volume: 0.8,
      stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
      has_stems: true, stem_mode: "two_stem",
    },

    // Lyrics
    fetch_lyrics: {
      raw_lrc: "[00:05.00]Is this the real life?\\n[00:10.00]Is this just fantasy?\\n[00:15.00]Caught in a landslide\\n[00:20.00]No escape from reality",
      lines: [
        { time_ms: 5000, text: "Is this the real life?" },
        { time_ms: 10000, text: "Is this just fantasy?" },
        { time_ms: 15000, text: "Caught in a landslide" },
        { time_ms: 20000, text: "No escape from reality" },
      ],
      offset_ms: 0, source: "manual",
    },
    fetch_lyrics_online: {
      raw_lrc: "[00:05.00]Is this the real life?\\n[00:10.00]Is this just fantasy?",
      lines: [
        { time_ms: 5000, text: "Is this the real life?" },
        { time_ms: 10000, text: "Is this just fantasy?" },
      ],
      offset_ms: 0, source: "lrclib",
    },
    save_manual_lyrics: (args) => ({
      raw_lrc: (args && args.text) || "", lines: [], offset_ms: 0, source: "manual",
    }),
    set_lyrics_offset: undefined,

    // Playlists
    list_playlists: [],
    create_playlist: (args) => ({
      id: "pl-" + Date.now(), name: (args && args.name) || "New Playlist",
      song_count: 0, created_at: Date.now(), updated_at: Date.now(),
    }),
    rename_playlist: undefined,
    delete_playlist: undefined,
    add_songs_to_playlist: undefined,
    remove_songs_from_playlist: undefined,
    get_playlist_songs: [],

    // Rotation / Queue
    get_rotation_state: {
      singer_names: [], current_index: 0, mode: "round_robin", active: false,
    },
    set_rotation_state: undefined,
    advance_rotation: {
      singer_names: [], current_index: 0, mode: "round_robin", active: false,
    },

    // Bootstrap / model
    get_model_bootstrap_status: {
      state: "ready", model_path: "/tmp/model.onnx",
      downloaded_bytes: null, total_bytes: null, variant: "htdemucs",
    },

    // Misc
    window_ready: undefined,
    set_language: (args) => ({
      stem_mode: "two_stem", model_variant: "htdemucs",
      language: (args && args.language) || "en",
      hide_batch_separate: false, cover_art_backdrop: false,
      lyrics_font_step: 0, execution_provider: "cpu",
      available_execution_providers: ["cpu"],
    }),
    set_stem_mode: (args) => ({
      stem_mode: (args && args.mode) || "two_stem", model_variant: "htdemucs",
      language: "en", hide_batch_separate: false, cover_art_backdrop: false,
      lyrics_font_step: 0, execution_provider: "cpu",
      available_execution_providers: ["cpu"],
    }),
    create_library: undefined,
    open_library: undefined,
    batch_separate: undefined,
    cancel_remote_auth: undefined,
  };

  function invoke(cmd, args) {
    const handler = COMMANDS[cmd];
    if (handler === undefined) {
      return Promise.resolve(undefined);
    }
    if (typeof handler === "function") {
      try { return Promise.resolve(handler(args)); }
      catch (e) { return Promise.reject(e); }
    }
    return Promise.resolve(handler);
  }

  let callbackId = 0;
  const callbacks = new Map();

  window.__TAURI_INTERNALS__ = {
    invoke: invoke,
    transformCallback: function(callback, once) {
      var id = ++callbackId;
      callbacks.set(id, callback);
      return id;
    },
    unregisterCallback: function(id) {
      callbacks.delete(id);
    },
  };
})();
`;
