import type { InvokeCommand } from "@/lib/tauri/invoke";
import type {
  SourceSnapshot,
  StreamingImportJobSnapshot,
  StreamingImportProgress,
} from "@/types/ipc";

export interface MockData {
  songs: MockSong[];
  lyrics: MockLyrics;
  lyricsBySongId?: Record<string, MockLyrics>;
  primarySongHash: string;
  sidebarWidth: number;
  libraryRegistry: {
    active_library_id: string | null;
    libraries: Array<{
      id: string;
      kind: string;
      display_name: string;
      root_path: string;
    }>;
  };
  activeLibrary: {
    id: string;
    kind: string;
    display_name: string;
    root_path: string;
  };
  libraryPath: string;
  windowShellState: {
    chrome_variant: string;
    tier: string;
    toolbar_height_px: number;
    traffic_light_inset_leading: number;
    sidebar_header_height_px: number;
    sidebar_width_px: number;
  };
  settings: Record<string, unknown>;
  playbackSnapshot: Record<string, unknown>;
  bootstrapStatus: Record<string, unknown>;
  playlists: Array<{
    id: string;
    name: string;
    song_count: number;
    created_at: number;
    updated_at: number;
  }>;
  playlistSongs: Record<string, string[]>;
  rotationState: {
    singer_names: string[];
    current_index: number;
    mode: string;
    active: boolean;
  };
  loopPlayback?: boolean;
  loopStartPositionMs?: number;
  playStartPositionMs?: number;
  playStartPositionBySongId?: Record<string, number>;
  coverArtUrls?: Record<string, string>;
  stemsCompleted?: boolean;
}

export interface MockSong {
  hash: string;
  file_path: string | null;
  audio_source_kind: string;
  cdg_path: string | null;
  media_g_container: string | null;
  instrumental: boolean;
  language: string | null;
  title: string | null;
  artist: string | null;
  album: string | null;
  duration_ms: number;
  cover_art: number[] | null;
  imported_at: number;
  original_ext: string | null;
}

export interface MockLyrics {
  raw_lrc: string;
  lines: Array<{
    time_ms: number;
    text: string;
    words: unknown;
    bg_words: unknown;
    section: unknown;
    roman: unknown;
  }>;
  offset_ms: number;
  source: string;
}

export interface TauriMockHelpers {
  emitEvent: (eventName: string, payload: unknown) => void;
  setCommandDelayMs: (cmd: string, delayMs: number) => void;
  setMockSongs: (songs: MockSong[]) => void;
  setMockLyrics: (lyrics: MockLyrics | null) => void;
  setLargeLibrary: (count: number) => void;
  getInvokeCalls: () => Array<{ cmd: string; args: unknown }>;
  getLastNativeMenu: () => unknown;
  clickNativeMenuItem: (label: string) => Promise<void>;
  clickNativeSubmenuItem: (parentLabel: string, label: string) => Promise<void>;
  setPlaybackSnapshot: (
    patch: Record<string, unknown>,
  ) => Record<string, unknown>;
  setSeparationCompleted: (songHash: string) => void;
  getPlaybackSnapshot: () => Record<string, unknown>;
}

export interface TauriMockResult {
  internals: {
    metadata: {
      currentWindow: { label: string };
      currentWebview: { label: string };
    };
    invoke: InvokeCommand;
    transformCallback: (
      callback: (...args: unknown[]) => void,
      once?: boolean,
    ) => number;
    unregisterCallback: (id: number) => void;
  };
  eventPluginInternals: { unregisterListener: () => void };
  helpers: TauriMockHelpers;
}

// Everything below is a single self-contained function.  All logic lives
// inside the function body so `toString()` produces a valid standalone script.

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- must be `any` for toString() serialization
export function createTauriMock(data: any): TauriMockResult {
  // Mutable so tests can override the library, lyrics, playback, etc.
  let mockSongs = data.songs;
  let mockLyrics = data.lyrics;
  const mockLyricsBySongId = { ...(data.lyricsBySongId || {}) };
  let lyricsOverride: typeof mockLyrics | null = null;
  const invokeCalls: Array<{ cmd: string; args: any }> = [];
  const commandDelayMs = new Map<string, number>();
  const playlists: any[] = (data.playlists || []).map((p: any) => ({ ...p }));
  const playlistSongs = new Map<string, any[]>();
  const menuResources = new Map<number, any>();
  const separationStatuses: Record<string, any> = {};
  function completedFourStemStatus(songHash: any) {
    const stemDir = "/tmp/openkara-stems/" + songHash;
    return {
      song_id: songHash,
      state: "completed",
      percent: 100,
      cache_hit: true,
      vocals_path: stemDir + "/vocals.ogg",
      accomp_path: stemDir + "/accomp.ogg",
      drums_path: stemDir + "/drums.ogg",
      bass_path: stemDir + "/bass.ogg",
      other_path: stemDir + "/other.ogg",
      model_variant: "htdemucs",
      error: null,
    };
  }
  if (data.stemsCompleted) {
    for (const song of mockSongs) {
      separationStatuses[song.hash] = completedFourStemStatus(song.hash);
    }
  }
  let nextPlaylistId = 1;
  let nextEventId = 1;
  let nextMenuRid = 1;
  let lastNativeMenu: any = null;
  let transportGeneration = 0;
  let rotationState = { ...data.rotationState };
  let settingsSnapshot = { ...data.settings };
  const sourceSnapshots = ["demo-web", "demo-plugin"].map<SourceSnapshot>(
    (source_id, index) => ({
      source_id,
      owner_binding: `demo/${source_id}`,
      name: index ? "Plugin Library" : "Web Library",
      description: "Deterministic UI-only source",
      license: "MIT",
      installed_version: "1.0.0",
      available_version: "1.1.0",
      staged_version: null,
      pinned_version: null,
      enabled: true,
      runtime_ready: true,
      running: false,
      state: "ready",
      progress: null,
      last_error: null,
      active_work: [],
      manifest: {
        manifest_version: 1,
        source_id,
        package_id: source_id,
        publisher: "demo",
        version: "1.0.0",
        minimum_host_version: "1.0.0",
        protocol: { majors: [1], minor: 0 },
        runtime: { kind: "node", major: 22 },
        entry: "index.js",
        name: index ? "Plugin Library" : "Web Library",
        description: "UI-only fixture",
        license: "MIT",
        capabilities: index
          ? ["search", "playlists", "playlist_tracks", "import"]
          : [
              "liked_tracks",
              "playlists",
              "playlist_tracks",
              "search",
              "preview",
              "import",
            ],
        authentication: { flow: index ? "plugin" : "web" },
        requires_authentication: ["liked_tracks", "playlists"],
        resource_origins: [],
        media_origins: [],
      },
    }),
  );
  const sessions = new Map<string, boolean>();
  const challenges = new Map<
    string,
    { sourceId: string; state: string; polls: number }
  >();
  let importSourceId: string | null = null;
  let importJob: StreamingImportJobSnapshot | null = null;
  let importCancelled = false;
  const installations = new Map<string, { cancelled: boolean }>();
  function source(sourceId: string) {
    const snapshot = sourceSnapshots.find(
      (entry) => entry.source_id === sourceId,
    );
    if (!snapshot)
      throw {
        code: "streaming_source_not_found",
        message: "Unknown source",
        retryable: false,
      };
    return snapshot;
  }
  function changed(sourceId: string) {
    const snapshot = source(sourceId);
    emitMockEvent("streaming-source-status", clone(snapshot));
    return clone(snapshot);
  }
  function stopWork(sourceId: string, stop: boolean) {
    const snapshot = source(sourceId);
    if (snapshot.active_work.length && !stop)
      throw {
        code: "streaming_work_active",
        message: "Stop active work before continuing",
        retryable: true,
      };
    if (stop) {
      if (
        String(currentPlaybackSnapshot.song_id ?? "").startsWith(
          `stream:${sourceId}:`,
        )
      ) {
        clearPlaybackEnd();
        currentPlaybackSnapshot = {
          ...currentPlaybackSnapshot,
          state: "idle",
          is_playing: false,
          song_id: null,
        };
      }
      for (const challenge of challenges.values())
        if (challenge.sourceId === sourceId) challenge.state = "cancelled";
      if (importSourceId === sourceId) importCancelled = true;
      snapshot.active_work = [];
      snapshot.running = false;
    }
  }
  function session(sourceId: string) {
    source(sourceId);
    return {
      source_id: sourceId,
      signed_in: sessions.get(sourceId) ?? false,
      display_name: sessions.get(sourceId) ? "Demo Listener" : null,
      expired: false,
    };
  }
  function tracks(sourceId: string) {
    const snapshot = source(sourceId);
    if (!snapshot.enabled || !snapshot.installed_version)
      throw {
        code: "online_source_disabled",
        message: "Enable the installed source",
        retryable: false,
      };
    return ["Shared Track", "Second Track", "Final Track"].map(
      (title, index) => ({
        source_id: sourceId,
        remote_track_id: index ? String(index + 1) : "shared:/track",
        title: `${snapshot.name}: ${title}`,
        artist: "Demo Artist",
        album: null,
        duration_ms: 180000,
        refusal: null,
        library_song_id: null,
        cover_url: null,
      }),
    );
  }
  function page(sourceId: string, cursor?: string | null, query = "") {
    const values = tracks(sourceId).filter((track) =>
      track.title.toLowerCase().includes(query.toLowerCase()),
    );
    if (cursor && cursor !== `${sourceId}:next`)
      throw {
        code: "streaming_invalid_cursor",
        message: "Cursor belongs to another source or listing",
        retryable: false,
      };
    return {
      tracks: cursor ? values.slice(2) : values.slice(0, 2),
      next_cursor: !cursor && values.length > 2 ? `${sourceId}:next` : null,
    };
  }
  let currentPlaybackSnapshot = { ...data.playbackSnapshot };

  let playbackEndTimer: ReturnType<typeof setTimeout> | null = null;
  let playheadAnchorMs: number | null = null;
  let playheadPositionMs = Number(currentPlaybackSnapshot.position_ms) || 0;

  function nowMs(): number {
    return typeof performance !== "undefined" &&
      typeof performance.now === "function"
      ? performance.now()
      : Date.now();
  }

  function isPlayheadRunning(): boolean {
    const snap = currentPlaybackSnapshot as {
      is_playing?: boolean;
      state?: string;
    };
    return Boolean(
      snap.is_playing &&
      snap.state !== "buffering" &&
      playheadAnchorMs !== null,
    );
  }

  function livePositionMs(): number {
    const snap = currentPlaybackSnapshot as { duration_ms?: number | null };
    const duration = snap.duration_ms ?? Number.POSITIVE_INFINITY;
    const position = isPlayheadRunning()
      ? playheadPositionMs + (nowMs() - (playheadAnchorMs as number))
      : playheadPositionMs;
    return Math.max(0, Math.min(position, duration));
  }

  function writePositionMs(positionMs: number): void {
    playheadPositionMs = positionMs;
    currentPlaybackSnapshot = {
      ...currentPlaybackSnapshot,
      position_ms: positionMs,
    };
  }

  function startPlayhead(positionMs: number): void {
    writePositionMs(positionMs);
    playheadAnchorMs = nowMs();
  }

  function stopPlayhead(): void {
    writePositionMs(livePositionMs());
    playheadAnchorMs = null;
  }

  function snapshotWithLivePosition(): typeof currentPlaybackSnapshot {
    if (isPlayheadRunning()) {
      writePositionMs(livePositionMs());
      playheadAnchorMs = nowMs();
    }
    return currentPlaybackSnapshot;
  }

  // Initialize playlist songs from data
  if (data.playlistSongs) {
    for (const [playlistId, songHashes] of Object.entries(data.playlistSongs)) {
      playlistSongs.set(
        playlistId,
        (songHashes as string[]).map((songHash, index) => ({
          song_hash: songHash,
          added_at: index + 1,
          sort_order: index,
          singer: null,
        })),
      );
    }
  }

  function clone(value: any): any {
    if (value == null) return value;
    return JSON.parse(JSON.stringify(value));
  }

  function bumpTransportGeneration(): number {
    transportGeneration += 1;
    (currentPlaybackSnapshot as any).transport_generation = transportGeneration;
    return transportGeneration;
  }

  function resolveLyrics(songId: string | undefined): any {
    if (lyricsOverride) return lyricsOverride;
    if (songId && mockLyricsBySongId[songId]) return mockLyricsBySongId[songId];
    return mockLyrics;
  }

  function resolvePlayStartMs(songId: string, durationMs: number): number {
    const requested =
      data.playStartPositionBySongId?.[songId] ??
      data.playStartPositionMs ??
      data.loopStartPositionMs ??
      0;
    return Math.min(Math.max(0, requested), Math.max(0, durationMs - 1));
  }

  function schedulePlaybackEnd(): void {
    if (playbackEndTimer) {
      clearTimeout(playbackEndTimer);
      playbackEndTimer = null;
    }
    const snap = currentPlaybackSnapshot as any;
    if (!snap.is_playing || snap.state === "buffering") return;
    const remaining = (snap.duration_ms || 0) - livePositionMs();
    if (remaining <= 0) return;
    playbackEndTimer = setTimeout(() => {
      playbackEndTimer = null;
      const gen = (currentPlaybackSnapshot as any).transport_generation;
      if (gen !== transportGeneration) return;
      if (data.loopPlayback) {
        const loopMs = resolvePlayStartMs(
          snap.song_id || data.primarySongHash,
          snap.duration_ms || 0,
        );
        currentPlaybackSnapshot = {
          ...currentPlaybackSnapshot,
          state: "playing",
          is_playing: true,
          position_ms: loopMs,
        };
        startPlayhead(loopMs);
        emitMockEvent("playback-position", {
          ms: loopMs,
          transport_generation: transportGeneration,
          snapshot: clone(currentPlaybackSnapshot),
        });
        schedulePlaybackEnd();
      } else {
        const endedMs = Number(
          (currentPlaybackSnapshot as { duration_ms?: number }).duration_ms,
        );
        writePositionMs(Number.isFinite(endedMs) ? endedMs : 0);
        playheadAnchorMs = null;
        currentPlaybackSnapshot = {
          ...currentPlaybackSnapshot,
          state: "idle",
          is_playing: false,
        };
        emitMockEvent("playback-position", {
          ms: currentPlaybackSnapshot.position_ms,
          transport_generation: transportGeneration,
          snapshot: clone(currentPlaybackSnapshot),
        });
      }
    }, remaining);
  }

  function clearPlaybackEnd(): void {
    if (playbackEndTimer) {
      clearTimeout(playbackEndTimer);
      playbackEndTimer = null;
    }
  }

  function resolveCommandResult(cmd: string, result: any): Promise<any> {
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

  function playlistSnapshot(): any[] {
    return playlists.map((playlist) => ({
      ...playlist,
      song_count: playlistSongs.get(playlist.id)?.length || 0,
    }));
  }

  function menuResourceSnapshot(resource: any): any {
    return {
      label: resource.label,
      children: resource.items
        ? resource.items.map((child: any) => menuResourceSnapshot(child))
        : undefined,
    };
  }

  function readMenuItemReference(reference: any): any {
    const rid = Array.isArray(reference) ? reference[0] : reference?.rid;
    const resource = menuResources.get(rid);
    if (!resource) {
      throw new Error("Unknown native menu resource in mock: " + rid);
    }
    return resource;
  }

  const eventListeners = new Map<string, Map<number, number>>();
  let callbackId = 0;
  const callbacks = new Map<number, (...args: any[]) => void>();

  function emitMockEvent(eventName: string, payload: any): void {
    const handlers = eventListeners.get(eventName);
    if (!handlers) return;
    for (const [id, callbackIdNum] of handlers) {
      const callback = callbacks.get(callbackIdNum);
      if (typeof callback === "function") {
        callback({ event: eventName, id, payload: clone(payload) });
      }
    }
  }

  function handleTauriEventCommand(
    cmd: string,
    args: any,
  ): Promise<any> | null {
    if (cmd === "plugin:event|listen") {
      const id = nextEventId++;
      if (args && args.event && typeof args.handler === "number") {
        const handlers =
          eventListeners.get(args.event) || new Map<number, number>();
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

  function handleTauriResourceCommand(
    cmd: string,
    args: any,
  ): Promise<any> | null {
    if (cmd === "plugin:resources|close") {
      menuResources.delete(args.rid);
      return Promise.resolve(undefined);
    }
    return null;
  }

  function handleTauriWindowCommand(cmd: string): Promise<any> | null {
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

  const COMMANDS: Record<string, any> = {
    get_library_registry: data.libraryRegistry,
    get_settings: () => settingsSnapshot,
    get_window_shell_state: data.windowShellState,
    get_library_path: data.libraryPath,

    get_active_library: data.activeLibrary,
    get_library: () => mockSongs,
    search_library: (args: any) => {
      const q = ((args && args.query) || "").toLowerCase();
      if (!q) return mockSongs;
      return mockSongs.filter(
        (s: any) =>
          (s.title ?? "").toLowerCase().includes(q) ||
          (s.artist ?? "").toLowerCase().includes(q),
      );
    },
    get_song_properties: (args: any) => {
      const song = mockSongs.find(
        (candidate: any) => args?.songId === candidate.hash,
      );
      if (!song || !song.original_ext) {
        throw new Error(`Song properties are missing for ${args?.songId}`);
      }
      return {
        format: song.original_ext.toUpperCase(),
        sample_rate_hz: 44_100,
        channels: 2,
        bit_rate_bps: 320_000,
        file_size_bytes: 7_300_000,
        duration_ms: song.duration_ms,
        hash: song.hash,
      };
    },
    get_all_separation_statuses: () => clone(Object.values(separationStatuses)),
    get_all_upload_statuses: {},
    get_remote_cache_usage: () => ({
      used_bytes: 512 * 1024 * 1024,
      limit_bytes: 2 * 1024 * 1024 * 1024,
      entry_count: 3,
      pinned_count: 0,
    }),
    clear_remote_cache: () => 0,
    get_remote_diagnostics: () => ({
      has_active_remote: false,
      repository_id: null,
      writer_id: null,
      committed_generation: 0,
      local_base_generation: 0,
      local_state: "clean",
      local_db_digest: null,
      active_operation_id: null,
      last_success_at_ms: null,
      last_error_code: null,
      recent_operations: [],
    }),

    get_cover_art: async (args: any) => {
      const hash = args && (args.hash || args.song_id || args.songId);
      const song = mockSongs.find((entry: any) => entry.hash === hash);
      if (song && song.cover_art && song.cover_art.length > 0) {
        return clone(song.cover_art);
      }
      const url = hash ? (data.coverArtUrls || {})[hash] : null;
      if (!url || typeof fetch !== "function") {
        return null;
      }
      try {
        const response = await fetch(url);
        if (!response.ok) {
          return null;
        }
        return Array.from(new Uint8Array(await response.arrayBuffer()));
      } catch {
        return null;
      }
    },
    get_playback_state: () => clone(snapshotWithLivePosition()),
    play: (args: any) => {
      const songId =
        (args && (args.songId || args.song_id)) || data.primarySongHash;
      for (const snapshot of sourceSnapshots)
        snapshot.active_work = snapshot.active_work.filter(
          (work) => work.kind !== "preview",
        );
      if (String(songId).startsWith("stream:")) {
        const match = /^stream:([a-z0-9-]+):(.+)$/.exec(songId);
        if (!match)
          throw {
            code: "invalid_preview",
            message: "Invalid preview identity",
            retryable: false,
          };
        const remoteId = decodeURIComponent(match[2]);
        if (
          !tracks(match[1]).some((track) => track.remote_track_id === remoteId)
        )
          throw {
            code: "unknown_track",
            message: "Unknown preview track",
            retryable: false,
          };
        source(match[1]).active_work.push({
          id: 1000,
          kind: "preview",
          cancelling: false,
        });
        source(match[1]).running = true;
        changed(match[1]);
      }
      const song = mockSongs.find((s: any) => s.hash === songId);
      const durationMs = song ? song.duration_ms : 300000;
      const positionMs = resolvePlayStartMs(songId, durationMs);
      bumpTransportGeneration();
      currentPlaybackSnapshot = {
        ...currentPlaybackSnapshot,
        song_id: songId,
        state: "playing",
        is_playing: true,
        position_ms: positionMs,
        duration_ms: durationMs,
        buffered_ms: durationMs,
      };
      startPlayhead(positionMs);
      schedulePlaybackEnd();
      return clone(currentPlaybackSnapshot);
    },
    resume: () => {
      bumpTransportGeneration();
      const positionMs = livePositionMs();
      currentPlaybackSnapshot = {
        ...currentPlaybackSnapshot,
        state: "playing",
        is_playing: true,
        position_ms: positionMs,
      };
      startPlayhead(positionMs);
      schedulePlaybackEnd();
      return clone(currentPlaybackSnapshot);
    },
    pause: () => {
      bumpTransportGeneration();
      clearPlaybackEnd();
      stopPlayhead();
      currentPlaybackSnapshot = {
        ...currentPlaybackSnapshot,
        state: "idle",
        is_playing: false,
      };
      return clone(currentPlaybackSnapshot);
    },
    seek: (args: any) => {
      bumpTransportGeneration();
      clearPlaybackEnd();
      const targetMs = (args && args.ms) || 0;
      writePositionMs(targetMs);
      playheadAnchorMs = null;
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
        buffered_ms: (currentPlaybackSnapshot as any).duration_ms || 354000,
      };
      queueMicrotask(() => {
        emitMockEvent("playback-position", {
          ms: bufferingSnapshot.position_ms,
          transport_generation: (bufferingSnapshot as any).transport_generation,
          snapshot: clone(bufferingSnapshot),
        });
      });
      setTimeout(() => {
        currentPlaybackSnapshot = playingSnapshot;
        startPlayhead(playingSnapshot.position_ms);
        emitMockEvent("playback-position", {
          ms: playingSnapshot.position_ms,
          transport_generation: (playingSnapshot as any).transport_generation,
          snapshot: clone(playingSnapshot),
        });
        schedulePlaybackEnd();
      }, 80);
      return clone(bufferingSnapshot);
    },
    set_volume: (args: any) => {
      const level = args && args.level != null ? args.level : 0.8;
      currentPlaybackSnapshot = { ...currentPlaybackSnapshot, volume: level };
      return clone(currentPlaybackSnapshot);
    },
    set_stem_volume: (args: any) => {
      const stem = args && args.stem;
      const level = args && args.level != null ? args.level : 1;
      if (stem && (currentPlaybackSnapshot as any).stem_volumes) {
        currentPlaybackSnapshot = {
          ...currentPlaybackSnapshot,
          stem_volumes: {
            ...(currentPlaybackSnapshot as any).stem_volumes,
            [stem]: level,
          },
        };
      }
      return clone(currentPlaybackSnapshot);
    },
    load_stems: () => clone(currentPlaybackSnapshot),

    fetch_lyrics: (args: any) => {
      const songId = args && args.songId;
      return { ...resolveLyrics(songId), song_id: songId };
    },
    fetch_lyrics_online: (args: any) => {
      const songId = args && args.songId;
      return { ...resolveLyrics(songId), song_id: songId };
    },
    save_manual_lyrics: (args: any) => ({
      raw_lrc: (args && args.text) || "",
      lines: [],
      offset_ms: 0,
      source: "manual",
    }),
    set_lyrics_offset: undefined,

    list_playlists: () => playlistSnapshot(),
    create_playlist: (args: any) => {
      const pl = {
        id: "pl-" + nextPlaylistId++,
        name: (args && args.name) || "New Playlist",
        song_count: 0,
        created_at: Date.now(),
        updated_at: Date.now(),
      };
      return pl;
    },
    rename_playlist: undefined,
    delete_playlist: undefined,
    add_songs_to_playlist: (args: any) => {
      const current = playlistSongs.get(args.playlistId) || [];
      const existing = new Set(current.map((entry: any) => entry.song_hash));
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
    remove_songs_from_playlist: (args: any) => {
      const remove = new Set(args.songHashes || []);
      const next = (playlistSongs.get(args.playlistId) || [])
        .filter((entry: any) => !remove.has(entry.song_hash))
        .map((entry: any, index: number) => ({ ...entry, sort_order: index }));
      playlistSongs.set(args.playlistId, next);
      return undefined;
    },
    get_playlist_songs: (args: any) => playlistSongs.get(args.playlistId) || [],

    get_rotation_state: () => rotationState,
    set_rotation_state: (args: any) => {
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

    get_model_bootstrap_status: data.bootstrapStatus,

    window_ready: undefined,
    set_language: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        language: (args && args.language) || "en",
      };
      return settingsSnapshot;
    },
    set_stem_mode: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        stem_mode: (args && args.mode) || "two_stem",
      };
      return settingsSnapshot;
    },
    set_lyrics_font_step: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        lyrics_font_step: (args && args.step) || 0,
      };
      return settingsSnapshot;
    },
    set_execution_provider: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        execution_provider: (args && args.provider) || "cpu",
      };
      return settingsSnapshot;
    },
    set_hide_batch_separate: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        hide_batch_separate: (args && args.value) || false,
      };
      return settingsSnapshot;
    },
    set_cover_art_backdrop: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        cover_art_backdrop: (args && args.value) || false,
      };
      return settingsSnapshot;
    },
    set_lyrics_blur_inactive: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        lyrics_blur_inactive: !!(args && args.value),
      };
      return settingsSnapshot;
    },
    set_hide_upgrade_all: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        hide_upgrade_all: (args && args.value) || false,
      };
      return settingsSnapshot;
    },
    set_eq_enabled: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        eq_enabled: !!(args && args.enabled),
      };
      return settingsSnapshot;
    },
    set_eq_gains: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        eq_gains_db: (args && args.gainsDb) || [0, 0, 0, 0, 0],
      };
      return settingsSnapshot;
    },
    set_library_sort_mode: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        library_sort_mode: (args && args.mode) || "recently_imported",
      };
      return settingsSnapshot;
    },
    set_theme_preference: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        theme_preference: (args && args.preference) || "dark",
      };
      return settingsSnapshot;
    },
    set_update_policy: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        update_policy: (args && args.policy) || "notify",
      };
      return settingsSnapshot;
    },
    list_online_sources: () => [
      {
        id: "youtube",
        kind: "video",
        enabled: !!settingsSnapshot.youtube_source_enabled,
        capabilities: {
          sign_in: false,
          browse: false,
          import: false,
          resolve_video: !!settingsSnapshot.youtube_source_enabled,
        },
      },
    ],
    list_streaming_sources: () => clone(sourceSnapshots),
    refresh_streaming_sources: () => clone(sourceSnapshots),
    install_streaming_source: async (args: {
      sourceId: string;
      version?: string;
    }) => {
      const s = source(args.sourceId);
      if (installations.has(args.sourceId))
        throw {
          code: "install_in_progress",
          message: "Installation already running",
          retryable: true,
        };
      const operation = { cancelled: false };
      installations.set(args.sourceId, operation);
      s.state = "installing";
      s.progress = { phase: "download", completed: 0, total: 100 };
      changed(args.sourceId);
      await new Promise<void>((resolve) => setTimeout(resolve, 20));
      installations.delete(args.sourceId);
      if (operation.cancelled)
        throw {
          code: "streaming_cancelled",
          message: "Installation cancelled",
          retryable: true,
        };
      s.installed_version =
        args.version ?? s.pinned_version ?? s.available_version;
      s.state = s.enabled ? "ready" : "disabled";
      s.runtime_ready = true;
      s.progress = null;
      return changed(args.sourceId);
    },
    cancel_streaming_source_install: (args: { sourceId: string }) => {
      const s = source(args.sourceId);
      const operation = installations.get(args.sourceId);
      if (operation) operation.cancelled = true;
      s.progress = null;
      s.state = s.installed_version
        ? s.enabled
          ? "ready"
          : "disabled"
        : "not_installed";
      changed(args.sourceId);
    },
    set_streaming_source_enabled: (args: {
      sourceId: string;
      enabled: boolean;
      stopWork?: boolean;
    }) => {
      const s = source(args.sourceId);
      if (!s.installed_version)
        throw {
          code: "not_installed",
          message: "Install the source first",
          retryable: false,
        };
      if (!args.enabled) stopWork(args.sourceId, !!args.stopWork);
      s.enabled = args.enabled;
      s.state = args.enabled ? "ready" : "disabled";
      return changed(args.sourceId);
    },
    uninstall_streaming_source: (args: {
      sourceId: string;
      stopWork?: boolean;
      removeCredentials: boolean;
    }) => {
      if (typeof args.removeCredentials !== "boolean")
        throw {
          code: "credential_choice_required",
          message: "Choose whether to remove saved sign-in",
          retryable: false,
        };
      stopWork(args.sourceId, !!args.stopWork);
      const s = source(args.sourceId);
      s.installed_version = null;
      s.enabled = false;
      s.state = "not_installed";
      if (args.removeCredentials) sessions.delete(args.sourceId);
      return changed(args.sourceId);
    },
    pin_streaming_source: (args: {
      sourceId: string;
      version: string | null;
    }) => {
      source(args.sourceId).pinned_version = args.version;
      return changed(args.sourceId);
    },
    get_streaming_session: (args: { sourceId: string }) =>
      session(args.sourceId),
    open_streaming_sign_in: (args: { sourceId: string }) => {
      const s = source(args.sourceId);
      tracks(args.sourceId);
      const challenge = `${args.sourceId}:auth:${challenges.size + 1}`;
      challenges.set(challenge, {
        sourceId: args.sourceId,
        state: "pending",
        polls: 0,
      });
      s.active_work.push({
        id: challenges.size,
        kind: "authentication",
        cancelling: false,
      });
      s.running = true;
      changed(args.sourceId);
      return {
        source_id: args.sourceId,
        challenge,
        expires_at: "2099-01-01T00:00:00Z",
        poll_after_ms: 10,
        presentation:
          args.sourceId === "demo-web"
            ? { kind: "web" }
            : {
                kind: "qr",
                content: "https://example.invalid/demo-auth",
                text: "Scan the UI-only demo code",
              },
      };
    },
    poll_streaming_sign_in: (args: { sourceId: string; challenge: string }) => {
      source(args.sourceId);
      const entry = challenges.get(args.challenge);
      if (!entry || entry.sourceId !== args.sourceId)
        throw {
          code: "unknown_challenge",
          message: "Challenge does not belong to this source",
          retryable: false,
        };
      if (entry.state === "pending" && ++entry.polls >= 2) {
        entry.state = "complete";
        sessions.set(args.sourceId, true);
        source(args.sourceId).active_work = [];
        source(args.sourceId).running = false;
        emitMockEvent("streaming-sign-in-complete", session(args.sourceId));
        changed(args.sourceId);
      }
      return entry.state === "complete"
        ? { state: "complete", session: session(args.sourceId) }
        : { state: entry.state, poll_after_ms: 10 };
    },
    cancel_streaming_sign_in: (args: {
      sourceId: string;
      challenge: string;
    }) => {
      const entry = challenges.get(args.challenge);
      if (!entry || entry.sourceId !== args.sourceId)
        throw {
          code: "unknown_challenge",
          message: "Unknown challenge",
          retryable: false,
        };
      entry.state = "cancelled";
      source(args.sourceId).active_work = [];
      source(args.sourceId).running = false;
      changed(args.sourceId);
    },
    sign_out_streaming_source: (args: {
      sourceId: string;
      stopWork?: boolean;
    }) => {
      stopWork(args.sourceId, !!args.stopWork);
      sessions.delete(args.sourceId);
      return session(args.sourceId);
    },
    list_streaming_liked_tracks: (args: {
      sourceId: string;
      cursor?: string;
    }) => {
      if (
        !source(args.sourceId).manifest?.capabilities.includes("liked_tracks")
      )
        throw {
          code: "streaming_unsupported_operation",
          message: "This source does not offer liked tracks",
          retryable: false,
        };
      return page(args.sourceId, args.cursor);
    },
    get_streaming_tracks: (args: {
      sourceId: string;
      remoteTrackIds: string[];
    }) =>
      args.remoteTrackIds.flatMap((id) =>
        tracks(args.sourceId).filter((track) => track.remote_track_id === id),
      ),
    list_streaming_playlists: (args: { sourceId: string; cursor?: string }) => {
      tracks(args.sourceId);
      if (args.cursor && args.cursor !== `${args.sourceId}:playlists`)
        throw {
          code: "streaming_invalid_cursor",
          message: "Invalid playlist cursor",
          retryable: false,
        };
      return {
        items: [
          {
            source_id: args.sourceId,
            remote_id: args.cursor ? "second" : "shared-playlist",
            name: args.cursor ? "Second playlist" : "Shared playlist",
            track_count: 3,
            cover_url: null,
          },
        ],
        next_cursor: args.cursor ? null : `${args.sourceId}:playlists`,
      };
    },
    get_streaming_playlist: (args: {
      sourceId: string;
      remotePlaylistId: string;
      cursor?: string;
    }) => ({
      source_id: args.sourceId,
      remote_id: args.remotePlaylistId,
      name: "Shared playlist",
      ...page(args.sourceId, args.cursor),
      cover_url: null,
      description: "UI-only fixture",
      track_count: 3,
      creator: null,
      updated_at_ms: null,
    }),
    search_streaming_source: (args: {
      sourceId: string;
      query: string;
      cursor?: string;
    }) => page(args.sourceId, args.cursor, args.query),
    cancel_streaming_import: () => {
      importCancelled = true;
    },
    get_streaming_import_status: () => clone(importJob),
    start_streaming_import: async (args: {
      sourceId: string;
      remoteTrackIds: string[];
      jobId: string;
    }) => {
      if (importSourceId)
        throw {
          code: "import_in_progress",
          message: "An import is already running",
          retryable: true,
        };
      const selected = args.remoteTrackIds.flatMap((id) =>
        tracks(args.sourceId).filter((track) => track.remote_track_id === id),
      );
      importSourceId = args.sourceId;
      importCancelled = false;
      const job: StreamingImportJobSnapshot = {
        source_id: args.sourceId,
        job_id: args.jobId,
        phase: "running",
        completed: 0,
        total: selected.length,
        tracks: selected.map((track) => ({
          remote_track_id: track.remote_track_id,
          title: track.title,
          artist: track.artist,
          state: { kind: "queued" },
        })),
      };
      importJob = job;
      source(args.sourceId).active_work.push({
        id: 2000,
        kind: "import",
        cancelling: false,
      });
      changed(args.sourceId);
      await new Promise<void>((resolve) => setTimeout(resolve, 20));
      for (const track of job.tracks) {
        track.state = {
          kind: "failed",
          reason: importCancelled ? "cancelled" : "import_failed",
        };
        job.completed++;
        emitMockEvent("streaming-import-progress", {
          source_id: args.sourceId,
          job_id: job.job_id,
          ...track,
          completed: job.completed,
          total: job.total,
        });
      }
      job.phase = "completed";
      importSourceId = null;
      source(args.sourceId).active_work = source(
        args.sourceId,
      ).active_work.filter((work) => work.kind !== "import");
      changed(args.sourceId);
      const progress: StreamingImportProgress = {
        source_id: args.sourceId,
        status: "completed",
        imported_song_ids: [],
        failed: selected.map((track) => ({
          remote_track_id: track.remote_track_id,
          title: track.title,
          artist: track.artist,
          reason: importCancelled ? "cancelled" : "import_failed",
          refusal: null,
        })),
        playlist_id: null,
        conflict: null,
      };
      emitMockEvent("streaming-import-complete", {
        source_id: args.sourceId,
        job_id: job.job_id,
        progress,
      });
      return progress;
    },
    continue_streaming_import: () => {
      throw {
        code: "no_active_decision",
        message: "No import decision is pending",
        retryable: false,
      };
    },
    resolve_video_source_url: (args: { url: string }) => {
      if (String(args.url).includes("/player")) {
        throw new Error("YouTube /player stream URLs are not used");
      }
      return [];
    },
    control_youtube_watch: (args: {
      action?: { type?: string; url?: string };
    }) => {
      const url = args.action?.url;
      if (url?.includes("/player")) {
        throw new Error("YouTube /player stream URLs are not used");
      }
      return {
        ended: false,
        paused: args.action?.type !== "play",
        current_time_ms: 0,
        duration_ms: null,
      };
    },
    get_reveal_targets: () => ({
      song_file: { available: false, path: null },
      stems: { available: false, path: null },
    }),
    reveal_in_folder: () => {},
    set_online_source_enabled: (args: {
      sourceId?: string;
      enabled?: boolean;
    }) => {
      const sourceId = args.sourceId;
      const enabled = !!args.enabled;
      settingsSnapshot = {
        ...settingsSnapshot,
        youtube_source_enabled:
          sourceId === "youtube"
            ? enabled
            : !!settingsSnapshot.youtube_source_enabled,
      };
      return settingsSnapshot;
    },
    check_runtime_updates: () => ({
      generation: 1,
      release_id: "mock-release",
      target_triple: "aarch64-apple-darwin",
      state: "up_to_date",
      installed_version: "v1.27.1",
      available_version: "v1.27.1",
      available_bytes: 0,
      restart_required: true,
    }),
    set_crossfade_enabled: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        crossfade_enabled: !!(args && args.enabled),
      };
      return settingsSnapshot;
    },
    set_crossfade_duration_ms: (args: any) => {
      settingsSnapshot = {
        ...settingsSnapshot,
        crossfade_duration_ms: (args && args.durationMs) || 3000,
      };
      return settingsSnapshot;
    },

    get_audio_peaks: { writeIndex: 0, peaks: [] },
    set_preload_candidate: undefined,
    get_waveform: (args: any) => {
      const buckets = (args && args.buckets) || 200;
      return Array.from({ length: buckets }, () => 0);
    },
    restart_app: undefined,
    create_library: undefined,
    open_library: undefined,
    batch_separate: undefined,
    cancel_remote_auth: undefined,
  };

  function invoke(cmd: string, args?: any): Promise<any> {
    invokeCalls.push({ cmd, args: clone(args) });

    const eventResult = handleTauriEventCommand(cmd, args);
    if (eventResult) return eventResult;

    const resourceResult = handleTauriResourceCommand(cmd, args || {});
    if (resourceResult) return resourceResult;

    const windowResult = handleTauriWindowCommand(cmd);
    if (windowResult) return windowResult;

    if (cmd === "plugin:menu|new") {
      const rid = nextMenuRid++;
      const options = (args && args.options) || {};
      const items = (options.items || []).map(readMenuItemReference);
      const resource = {
        rid,
        kind: args.kind,
        label: options.text || options.item || "",
        action: options.handler?.onmessage || args?.handler?.onmessage || null,
        items,
        popupPosition: null,
      };
      menuResources.set(rid, resource);
      return Promise.resolve([rid, String(rid)]);
    }
    if (cmd === "plugin:menu|popup") {
      const resource = readMenuItemReference({ rid: args.rid });
      resource.popupPosition = args.at ? { x: args.at.x, y: args.at.y } : null;
      lastNativeMenu = resource;
      return Promise.resolve(undefined);
    }

    const handler = COMMANDS[cmd];
    if (handler === undefined) {
      if (Object.prototype.hasOwnProperty.call(COMMANDS, cmd)) {
        return Promise.resolve(undefined);
      }
      return Promise.reject(
        new Error("Unhandled Tauri invoke in mock: " + cmd),
      );
    }
    if (typeof handler === "function") {
      try {
        const result = handler(args);
        if (cmd === "create_playlist" && result) {
          playlists.push(result);
          playlistSongs.set(result.id, []);
        }
        return resolveCommandResult(cmd, result);
      } catch (e) {
        return Promise.reject(e);
      }
    }
    return resolveCommandResult(cmd, handler);
  }

  function menuSnapshot(menu: any): any {
    return {
      items: menu.items.map((item: any) => menuResourceSnapshot(item)),
      popupPosition: menu.popupPosition,
    };
  }

  async function clickMenuItem(menu: any, label: string): Promise<void> {
    const item = menu.items.find((candidate: any) => candidate.label === label);
    if (!item || typeof item.action !== "function") {
      throw new Error("Native menu item not found in mock: " + label);
    }
    await item.action();
  }

  async function clickSubmenuItem(
    menu: any,
    parentLabel: string,
    label: string,
  ): Promise<void> {
    const parent = menu.items.find(
      (candidate: any) => candidate.label === parentLabel,
    );
    const item = parent?.items?.find(
      (candidate: any) => candidate.label === label,
    );
    if (!item || typeof item.action !== "function") {
      throw new Error(
        "Native submenu item not found in mock: " + parentLabel + " > " + label,
      );
    }
    await item.action();
  }

  function generateLargeLibrary(count: number): any[] {
    const songs = [];
    const letters = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    for (let i = 0; i < count; i++) {
      const letter = letters[i % 26];
      songs.push({
        hash: "large-" + i,
        file_path: "/music/song-" + i + ".mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: "en",
        title: letter + " Song " + i,
        artist: letter + " Artist " + i,
        album: null,
        duration_ms: 180000,
        cover_art: null,
        imported_at: count - i,
        original_ext: ".mp3",
      });
    }
    return songs;
  }

  // If the initial snapshot is already playing (website preview auto-play),
  // schedule the end-of-song timer so playback loops correctly.
  if ((currentPlaybackSnapshot as any).is_playing) {
    startPlayhead(Number(currentPlaybackSnapshot.position_ms) || 0);
    schedulePlaybackEnd();
  }

  // ── Return ──
  return {
    internals: {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      invoke,
      transformCallback: function (callback: (...args: any[]) => void): number {
        const id = ++callbackId;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback: function (id: number): void {
        callbacks.delete(id);
      },
    },
    eventPluginInternals: {
      unregisterListener: function () {},
    },
    helpers: {
      emitEvent: (eventName: string, payload: any) =>
        emitMockEvent(eventName, payload),
      setCommandDelayMs: (cmd: string, delayMs: number) => {
        commandDelayMs.set(cmd, Math.max(0, delayMs));
      },
      setMockSongs: (songs: any[]) => {
        mockSongs = songs;
      },
      setMockLyrics: (lyrics: any) => {
        if (lyrics == null) {
          lyricsOverride = null;
          return;
        }
        mockLyrics = lyrics;
        lyricsOverride = lyrics;
      },
      setLargeLibrary: (count: number) => {
        mockSongs = generateLargeLibrary(count);
      },
      getInvokeCalls: () => clone(invokeCalls),
      getLastNativeMenu: () =>
        lastNativeMenu ? menuSnapshot(lastNativeMenu) : null,
      clickNativeMenuItem: async (label: string) => {
        if (!lastNativeMenu) throw new Error("No native menu has been opened");
        await clickMenuItem(lastNativeMenu, label);
      },
      clickNativeSubmenuItem: async (parentLabel: string, label: string) => {
        if (!lastNativeMenu) throw new Error("No native menu has been opened");
        await clickSubmenuItem(lastNativeMenu, parentLabel, label);
      },
      setPlaybackSnapshot: (patch: any) => {
        const next = { ...currentPlaybackSnapshot, ...patch };
        if (
          patch &&
          patch.stem_volumes &&
          (currentPlaybackSnapshot as any).stem_volumes
        ) {
          next.stem_volumes = {
            ...(currentPlaybackSnapshot as any).stem_volumes,
            ...patch.stem_volumes,
          };
        }
        currentPlaybackSnapshot = next;
        if (next.is_playing && next.state !== "buffering") {
          startPlayhead(Number(next.position_ms) || 0);
        } else {
          writePositionMs(Number(next.position_ms) || 0);
          playheadAnchorMs = null;
        }
        emitMockEvent("playback-position", {
          ms: next.position_ms,
          transport_generation: next.transport_generation,
          snapshot: clone(next),
        });
        if (
          patch &&
          (patch.is_playing !== undefined ||
            patch.state !== undefined ||
            patch.position_ms !== undefined ||
            patch.duration_ms !== undefined)
        ) {
          if (next.is_playing && next.state !== "buffering") {
            schedulePlaybackEnd();
          } else {
            clearPlaybackEnd();
          }
        }
        return clone(next);
      },
      setSeparationCompleted: (songHash: string) => {
        separationStatuses[songHash] = {
          song_id: songHash,
          state: "completed",
        };
        emitMockEvent("separation-complete", {
          song_id: songHash,
          status: { song_id: songHash, state: "completed" },
        });
      },
      getPlaybackSnapshot: () => clone(snapshotWithLivePosition()),
    },
  };
}
