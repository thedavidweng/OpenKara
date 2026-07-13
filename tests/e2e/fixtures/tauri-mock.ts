/**
 * Mock for Tauri IPC used by Playwright UI smoke tests.
 *
 * In a real Tauri build the Rust backend owns the database, audio pipeline,
 * and filesystem.  During browser-based UI smoke runs (against the Vite dev
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
  // Mutable so tests can override the library with larger fixtures (e.g. the
  // scrollbar spec needs enough rows to produce real overflow in the song
  // list).  get_library is a function that reads the current value.
  let mockSongs = [
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

  // ── Fixture lyrics ──
  // Mutable so tests can override with larger fixtures (e.g. the scrollbar
  // spec needs enough lines to produce real overflow in the lyrics panel).
  let mockLyrics = {
    raw_lrc: "[00:05.00]Is this the real life?\\n[00:10.00]Is this just fantasy?\\n[00:15.00]Caught in a landslide\\n[00:20.00]No escape from reality",
    lines: [
      lyricLine(5000, "Is this the real life?"),
      lyricLine(10000, "Is this just fantasy?"),
      lyricLine(15000, "Caught in a landslide"),
      lyricLine(20000, "No escape from reality"),
    ],
    offset_ms: 0, source: "manual",
  };

  const invokeCalls = [];
  const commandDelayMs = new Map();
  const playlists = [];
  const playlistSongs = new Map();
  const menuResources = new Map();
  const separationStatuses = {};
  let nextPlaylistId = 1;
  let nextEventId = 1;
  let nextMenuRid = 1;
  let lastNativeMenu = null;
  // Matches Rust PlaybackStateSnapshot.transport_generation — load / resume /
  // pause / seek bump it so frontend stale-event filtering can be exercised.
  let transportGeneration = 0;
  let rotationState = {
    singer_names: [], current_index: 0, mode: "round_robin", active: false,
  };

  // Mutable playback snapshot — every get/set command reads or updates this
  // single source of truth so geometry/pressed E2E tests can exercise real
  // vocals/accompaniment mute transitions by patching stem volumes.
  let currentPlaybackSnapshot = {
    transport_generation: transportGeneration,
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
  };

  function bumpTransportGeneration() {
    transportGeneration += 1;
    currentPlaybackSnapshot.transport_generation = transportGeneration;
    return transportGeneration;
  }

  function playbackSnapshot(fields) {
    return { ...currentPlaybackSnapshot, ...fields };
  }

  function clone(value) {
    return value == null ? value : JSON.parse(JSON.stringify(value));
  }

  function resolveCommandResult(cmd, result) {
    return Promise.resolve(result)
      .then((value) => clone(value))
      .then((value) => {
        const delayMs = commandDelayMs.get(cmd) || 0;
        if (delayMs <= 0) return value;
        return new Promise((resolve) => {
          setTimeout(() => resolve(value), delayMs);
        });
      });
  }

  function playlistSnapshot() {
    return playlists.map((playlist) => ({
      ...playlist,
      song_count: playlistSongs.get(playlist.id)?.length || 0,
    }));
  }

  function menuResourceSnapshot(resource) {
    return {
      label: resource.label,
      children: resource.items
        ? resource.items.map((child) => menuResourceSnapshot(child))
        : undefined,
    };
  }

  function readMenuItemReference(reference) {
    const rid = Array.isArray(reference) ? reference[0] : reference?.rid;
    const resource = menuResources.get(rid);
    if (!resource) {
      throw new Error("Unknown native menu resource in E2E mock: " + rid);
    }
    return resource;
  }

  function lyricLine(timeMs, text) {
    return {
      time_ms: timeMs,
      text,
      words: null,
      bg_words: null,
      section: null,
    };
  }

  // Backend → frontend event bridge. Tests use __OPENKARA_E2E__.emitEvent to
  // simulate Rust-emitted events (e.g. the 33ms playback-position stream).
  const eventListeners = new Map();

  function handleTauriEventCommand(cmd, args) {
    if (cmd === "plugin:event|listen") {
      const id = nextEventId++;
      if (args && args.event && typeof args.handler === "number") {
        const handlers = eventListeners.get(args.event) || new Map();
        handlers.set(id, args.handler);
        eventListeners.set(args.event, handlers);
      }
      return Promise.resolve(id);
    }
    if (cmd === "plugin:event|unlisten") {
      if (args && args.event) {
        const handlers = eventListeners.get(args.event);
        if (handlers) handlers.delete(args.eventId);
      }
      return Promise.resolve(undefined);
    }
    if (cmd === "plugin:event|emit" || cmd === "plugin:event|emit_to") {
      return Promise.resolve(undefined);
    }
    return null;
  }

  function emitMockEvent(eventName, payload) {
    const handlers = eventListeners.get(eventName);
    if (!handlers) return;
    for (const [id, callbackId] of handlers) {
      const callback = callbacks.get(callbackId);
      if (typeof callback === "function") {
        callback({ event: eventName, id, payload: clone(payload) });
      }
    }
  }

  function handleTauriResourceCommand(cmd, args) {
    if (cmd === "plugin:resources|close") {
      menuResources.delete(args.rid);
      return Promise.resolve(undefined);
    }
    return null;
  }

  function handleTauriWindowCommand(cmd) {
    if (cmd === "plugin:window|is_maximized") {
      return Promise.resolve(false);
    }
    if (
      cmd === "plugin:window|close" ||
      cmd === "plugin:window|minimize" ||
      cmd === "plugin:window|start_dragging" ||
      cmd === "plugin:window|start_resize_dragging" ||
      cmd === "plugin:window|toggle_maximize"
    ) {
      return Promise.resolve(undefined);
    }
    return null;
  }

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
      theme_preference: "dark",
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
    get_library: () => mockSongs,
    search_library: (args) => {
      const q = ((args && args.query) || "").toLowerCase();
      if (!q) return mockSongs;
      return mockSongs.filter(
        (s) => s.title.toLowerCase().includes(q) || s.artist.toLowerCase().includes(q)
      );
    },
    get_all_separation_statuses: () => clone(separationStatuses),
    get_all_upload_statuses: {},

    // Playback — every snapshot carries transport_generation (IPC contract).
    // The mock owns one mutable currentPlaybackSnapshot; get_* returns a
    // clone and set_* updates the source so E2E tests can exercise real
    // volume/stem transitions by clicking actual buttons.
    get_playback_state: () => clone(currentPlaybackSnapshot),
    play: (args) => {
      const songId = (args && (args.songId || args.song_id)) || "aaa111";
      const song = mockSongs.find((s) => s.hash === songId);
      bumpTransportGeneration();
      currentPlaybackSnapshot = {
        ...currentPlaybackSnapshot,
        song_id: songId,
        state: "playing",
        is_playing: true,
        position_ms: 0,
        duration_ms: song ? song.duration_ms : 300000,
        buffered_ms: 0,
      };
      return clone(currentPlaybackSnapshot);
    },
    resume: () => {
      bumpTransportGeneration();
      currentPlaybackSnapshot = {
        ...currentPlaybackSnapshot,
        state: "playing",
        is_playing: true,
      };
      return clone(currentPlaybackSnapshot);
    },
    pause: () => {
      bumpTransportGeneration();
      currentPlaybackSnapshot = {
        ...currentPlaybackSnapshot,
        state: "idle",
        is_playing: false,
      };
      return clone(currentPlaybackSnapshot);
    },
    seek: (args) => {
      bumpTransportGeneration();
      const targetMs = (args && args.ms) || 0;
      const bufferingSnapshot = {
        ...currentPlaybackSnapshot,
        state: "buffering",
        is_playing: true,
        position_ms: targetMs,
        buffered_ms: targetMs,
      };
      currentPlaybackSnapshot = bufferingSnapshot;
      const playingSnapshot = {
        ...bufferingSnapshot,
        state: "playing",
        position_ms: targetMs + 50,
        buffered_ms: currentPlaybackSnapshot.duration_ms || 354000,
      };
      // Match the streaming Rust path, not the old optimistic browser fixture:
      // seek first publishes the target as buffering. The audio thread can
      // recover and publish playing before a delayed invoke response arrives.
      // The session must keep that newer same-generation event rather than
      // replaying the older buffering response and freezing the lyrics clock.
      queueMicrotask(() => {
        emitMockEvent("playback-position", {
          ms: bufferingSnapshot.position_ms,
          transport_generation: bufferingSnapshot.transport_generation,
          snapshot: clone(bufferingSnapshot),
        });
      });
      setTimeout(() => {
        currentPlaybackSnapshot = playingSnapshot;
        emitMockEvent("playback-position", {
          ms: playingSnapshot.position_ms,
          transport_generation: playingSnapshot.transport_generation,
          snapshot: clone(playingSnapshot),
        });
      }, 80);
      return clone(bufferingSnapshot);
    },
    set_volume: (args) => {
      // Use nullish handling so volume=0 is preserved (not replaced by 0.8).
      const level = args && args.level != null ? args.level : 0.8;
      currentPlaybackSnapshot = {
        ...currentPlaybackSnapshot,
        volume: level,
      };
      return clone(currentPlaybackSnapshot);
    },
    set_stem_volume: (args) => {
      // Update the named stem, preserving has_stems/stem_mode so the frontend
      // keeps its operational state across real mute/unmute transitions.
      const stem = args && args.stem;
      const level = args && args.level != null ? args.level : 1;
      if (stem && currentPlaybackSnapshot.stem_volumes) {
        currentPlaybackSnapshot = {
          ...currentPlaybackSnapshot,
          stem_volumes: {
            ...currentPlaybackSnapshot.stem_volumes,
            [stem]: level,
          },
        };
      }
      return clone(currentPlaybackSnapshot);
    },
    load_stems: () => clone(currentPlaybackSnapshot),

    // Lyrics — mutable so tests can override with larger fixtures (e.g. the
    // scrollbar spec needs enough lines to produce real overflow).
    fetch_lyrics: () => mockLyrics,
    fetch_lyrics_online: {
      raw_lrc: "[00:05.00]Is this the real life?\\n[00:10.00]Is this just fantasy?",
      lines: [
        lyricLine(5000, "Is this the real life?"),
        lyricLine(10000, "Is this just fantasy?"),
      ],
      offset_ms: 0, source: "lrclib",
    },
    save_manual_lyrics: (args) => ({
      raw_lrc: (args && args.text) || "", lines: [], offset_ms: 0, source: "manual",
    }),
    set_lyrics_offset: undefined,

    // Playlists
    list_playlists: () => playlistSnapshot(),
    create_playlist: (args) => ({
      id: "pl-" + nextPlaylistId++,
      name: (args && args.name) || "New Playlist",
      song_count: 0,
      created_at: Date.now(),
      updated_at: Date.now(),
    }),
    rename_playlist: undefined,
    delete_playlist: undefined,
    add_songs_to_playlist: (args) => {
      const current = playlistSongs.get(args.playlistId) || [];
      const existing = new Set(current.map((entry) => entry.song_hash));
      const next = [...current];
      for (const songHash of args.songHashes || []) {
        if (!existing.has(songHash)) {
          next.push({
            song_hash: songHash,
            added_at: Date.now(),
            sort_order: next.length,
            singer: null,
          });
        }
      }
      playlistSongs.set(args.playlistId, next);
      return undefined;
    },
    remove_songs_from_playlist: (args) => {
      const remove = new Set(args.songHashes || []);
      const next = (playlistSongs.get(args.playlistId) || [])
        .filter((entry) => !remove.has(entry.song_hash))
        .map((entry, index) => ({ ...entry, sort_order: index }));
      playlistSongs.set(args.playlistId, next);
      return undefined;
    },
    get_playlist_songs: (args) => playlistSongs.get(args.playlistId) || [],

    // Rotation / Queue
    get_rotation_state: () => rotationState,
    set_rotation_state: (args) => {
      rotationState = args.rotation;
      return undefined;
    },
    advance_rotation: () => {
      if (rotationState.singer_names.length > 0) {
        rotationState = {
          ...rotationState,
          current_index:
            (rotationState.current_index + 1) %
            rotationState.singer_names.length,
        };
      }
      return rotationState;
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
    set_lyrics_font_step: (args) => ({
      stem_mode: "two_stem", model_variant: "htdemucs",
      language: "en", hide_batch_separate: false, cover_art_backdrop: false,
      lyrics_font_step: (args && args.step) || 0, execution_provider: "cpu",
      available_execution_providers: ["cpu"],
    }),
    set_execution_provider: (args) => ({
      stem_mode: "two_stem", model_variant: "htdemucs",
      language: "en", hide_batch_separate: false, cover_art_backdrop: false,
      lyrics_font_step: 0, execution_provider: (args && args.provider) || "cpu",
      available_execution_providers: ["cpu"],
    }),
    set_hide_batch_separate: (args) => ({
      stem_mode: "two_stem", model_variant: "htdemucs",
      language: "en", hide_batch_separate: (args && args.value) || false, cover_art_backdrop: false,
      lyrics_font_step: 0, execution_provider: "cpu",
      available_execution_providers: ["cpu"],
    }),
    set_cover_art_backdrop: (args) => ({
      stem_mode: "two_stem", model_variant: "htdemucs",
      language: "en", hide_batch_separate: false, cover_art_backdrop: (args && args.value) || false,
      lyrics_font_step: 0, execution_provider: "cpu",
      available_execution_providers: ["cpu"],
    }),
    restart_app: undefined,
    create_library: undefined,
    open_library: undefined,
    batch_separate: undefined,
    cancel_remote_auth: undefined,
  };

  function invoke(cmd, args) {
    invokeCalls.push({ cmd, args: clone(args) });
    const eventResult = handleTauriEventCommand(cmd, args);
    if (eventResult) {
      return eventResult;
    }
    const resourceResult = handleTauriResourceCommand(cmd, args || {});
    if (resourceResult) {
      return resourceResult;
    }
    const windowResult = handleTauriWindowCommand(cmd);
    if (windowResult) {
      return windowResult;
    }
    if (cmd === "plugin:menu|new") {
      const rid = nextMenuRid++;
      const options = args.options || {};
      const items = (options.items || []).map(readMenuItemReference);
      const resource = {
        rid,
        kind: args.kind,
        label: options.text || options.item || "",
        action: options.handler?.onmessage || args.handler?.onmessage || null,
        items,
        popupPosition: null,
      };
      menuResources.set(rid, resource);
      return Promise.resolve([rid, String(rid)]);
    }
    if (cmd === "plugin:menu|popup") {
      const resource = readMenuItemReference({ rid: args.rid });
      resource.popupPosition = args.at
        ? { x: args.at.x, y: args.at.y }
        : null;
      lastNativeMenu = resource;
      return Promise.resolve(undefined);
    }

    const handler = COMMANDS[cmd];
    if (handler === undefined) {
      if (Object.prototype.hasOwnProperty.call(COMMANDS, cmd)) {
        return Promise.resolve(undefined);
      }
      return Promise.reject(new Error("Unhandled Tauri invoke in E2E mock: " + cmd));
    }
    if (typeof handler === "function") {
      try {
        const result = handler(args);
        if (cmd === "create_playlist" && result) {
          playlists.push(result);
          playlistSongs.set(result.id, []);
        }
        return resolveCommandResult(cmd, result);
      }
      catch (e) { return Promise.reject(e); }
    }
    return resolveCommandResult(cmd, handler);
  }

  let callbackId = 0;
  const callbacks = new Map();

  function menuSnapshot(menu) {
    return {
      items: menu.items.map((item) => menuResourceSnapshot(item)),
      popupPosition: menu.popupPosition,
    };
  }

  async function clickMenuItem(menu, label) {
    const item = menu.items.find((candidate) => candidate.label === label);
    if (!item || typeof item.action !== "function") {
      throw new Error("Native menu item not found in E2E mock: " + label);
    }
    await item.action();
  }

  async function clickSubmenuItem(menu, parentLabel, label) {
    const parent = menu.items.find((candidate) => candidate.label === parentLabel);
    const item = parent?.items?.find((candidate) => candidate.label === label);
    if (!item || typeof item.action !== "function") {
      throw new Error(
        "Native submenu item not found in E2E mock: " + parentLabel + " > " + label,
      );
    }
    await item.action();
  }

  window.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main" },
    },
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

  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: function() {},
  };

  window.__OPENKARA_E2E__ = {
    emitEvent: (eventName, payload) => emitMockEvent(eventName, payload),
    setCommandDelayMs: (cmd, delayMs) => {
      commandDelayMs.set(cmd, Math.max(0, delayMs));
    },
    // Override the mock library songs.  Call before navigating (via
    // addInitScript) or followed by a library reload to take effect.
    setMockSongs: (songs) => { mockSongs = songs; },
    // Override the mock lyrics payload returned by fetch_lyrics.
    setMockLyrics: (lyrics) => { mockLyrics = lyrics; },
    getInvokeCalls: () => clone(invokeCalls),
    getLastNativeMenu: () => lastNativeMenu ? menuSnapshot(lastNativeMenu) : null,
    clickNativeMenuItem: async (label) => {
      if (!lastNativeMenu) throw new Error("No native menu has been opened");
      await clickMenuItem(lastNativeMenu, label);
    },
    clickNativeSubmenuItem: async (parentLabel, label) => {
      if (!lastNativeMenu) throw new Error("No native menu has been opened");
      await clickSubmenuItem(lastNativeMenu, parentLabel, label);
    },
    // Merge a patch into the mutable playback snapshot and emit an
    // authoritative playback-position event so the frontend store picks up
    // the new state. Nested stem_volumes are merged per-stem.
    setPlaybackSnapshot: (patch) => {
      const next = { ...currentPlaybackSnapshot, ...patch };
      if (patch && patch.stem_volumes && currentPlaybackSnapshot.stem_volumes) {
        next.stem_volumes = {
          ...currentPlaybackSnapshot.stem_volumes,
          ...patch.stem_volumes,
        };
      }
      currentPlaybackSnapshot = next;
      emitMockEvent("playback-position", {
        ms: next.position_ms,
        transport_generation: next.transport_generation,
        snapshot: clone(next),
      });
      return clone(next);
    },
    // Mark a song as having completed stem separation so the frontend's
    // stemsAvailable gate opens when has_stems is also set. Emits a
    // separation-complete event so useSeparationEvents updates the store.
    setSeparationCompleted: (songHash) => {
      separationStatuses[songHash] = {
        song_id: songHash,
        state: "completed",
      };
      emitMockEvent("separation-complete", {
        song_id: songHash,
        status: { song_id: songHash, state: "completed" },
      });
    },
    getPlaybackSnapshot: () => clone(currentPlaybackSnapshot),
  };
})();
`;
