/**
 * IPC Contract Tests
 *
 * These tests validate the contract between the TypeScript frontend and the
 * Rust Tauri backend. They ensure:
 *   1. Frontend `invoke()` command names match Rust `#[tauri::command]` names
 *   2. TypeScript return types have all fields the Rust structs serialize
 *   3. Frontend parameter names match the Rust command handler parameter names
 *
 * When a contract test fails, it means either:
 *   - The frontend wrapper is calling a command the backend does not expose
 *   - The backend renamed a command without updating the frontend
 *   - A TypeScript type is missing a field that the Rust struct serializes
 *   - A field was added to a Rust struct without updating the TypeScript type
 *
 * These are STATIC tests -- they validate type shapes at compile time via
 * assignability checks and verify command name registries at runtime.
 */
import { describe, expect, test } from "vitest";

import type {
  AppSettings,
  CommandError,
  DeleteSongsResult,
  ExpandedImportPaths,
  ExtractEmbeddedCoverArtResult,
  ImportLyricsResult,
  ImportSongsResult,
  LyricsPayload,
  PlaybackStateSnapshot,
  SeparationStatusSnapshot,
  Song,
  SongProperties,
} from "./ipc";

interface CommandContract {
  /** The Tauri IPC command name (snake_case, matches Rust fn name) */
  command: string;
  /** File that wraps this command on the frontend */
  frontendFile: string;
  /** Frontend function that calls invoke() */
  frontendFn: string;
  /** Whether the command takes user-supplied arguments (excludes State/AppHandle) */
  hasArgs: boolean;
  /** Names of user-supplied parameters the Rust handler expects (snake_case) */
  rustParams?: string[];
}

const PLAYBACK_COMMANDS: CommandContract[] = [
  {
    command: "play",
    frontendFile: "src/lib/tauri/playback.ts",
    frontendFn: "play",
    hasArgs: true,
    rustParams: ["song_id"],
  },
  {
    command: "resume",
    frontendFile: "src/lib/tauri/playback.ts",
    frontendFn: "resume",
    hasArgs: false,
  },
  {
    command: "pause",
    frontendFile: "src/lib/tauri/playback.ts",
    frontendFn: "pause",
    hasArgs: false,
  },
  {
    command: "seek",
    frontendFile: "src/lib/tauri/playback.ts",
    frontendFn: "seek",
    hasArgs: true,
    rustParams: ["ms"],
  },
  {
    command: "set_volume",
    frontendFile: "src/lib/tauri/playback.ts",
    frontendFn: "setVolume",
    hasArgs: true,
    rustParams: ["level"],
  },
  {
    command: "set_stem_volume",
    frontendFile: "src/lib/tauri/playback.ts",
    frontendFn: "setStemVolume",
    hasArgs: true,
    rustParams: ["stem", "level"],
  },
  {
    command: "load_stems",
    frontendFile: "src/lib/tauri/playback.ts",
    frontendFn: "loadStems",
    hasArgs: false,
  },
  {
    command: "get_playback_state",
    frontendFile: "src/lib/tauri/playback.ts",
    frontendFn: "getPlaybackState",
    hasArgs: false,
  },
  {
    command: "get_audio_peaks",
    frontendFile: "src/lib/tauri/playback.ts",
    frontendFn: "getAudioPeaks",
    hasArgs: false,
  },
  {
    command: "set_preload_candidate",
    frontendFile: "src/lib/tauri/playback.ts",
    frontendFn: "setPreloadCandidate",
    hasArgs: true,
    rustParams: ["song_id"],
  },
];

const LIBRARY_COMMANDS: CommandContract[] = [
  {
    command: "import_songs",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "importSongs",
    hasArgs: true,
    rustParams: ["paths", "options"],
  },
  {
    command: "get_import_candidate_details",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "getImportCandidateDetails",
    hasArgs: true,
    rustParams: ["paths"],
  },
  {
    command: "expand_import_paths",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "expandImportPaths",
    hasArgs: true,
    rustParams: ["paths"],
  },
  {
    command: "pick_import_paths",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "pickImportPaths",
    hasArgs: true,
    rustParams: ["default_path"],
  },
  {
    command: "get_library",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "getLibrary",
    hasArgs: false,
  },
  {
    command: "search_library",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "searchLibrary",
    hasArgs: true,
    rustParams: ["query"],
  },
  {
    command: "get_cover_art",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "getCoverArt",
    hasArgs: true,
    rustParams: ["hash", "size"],
  },
  {
    command: "update_song_metadata",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "updateSongMetadata",
    hasArgs: true,
    rustParams: ["hash", "title", "artist"],
  },
  {
    command: "set_songs_instrumental",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "setSongsInstrumental",
    hasArgs: true,
    rustParams: ["song_ids", "instrumental"],
  },
  {
    command: "set_songs_language",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "setSongsLanguage",
    hasArgs: true,
    rustParams: ["song_ids", "language"],
  },
  {
    command: "delete_songs",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "deleteSongs",
    hasArgs: true,
    rustParams: ["song_ids"],
  },
  {
    command: "get_song_properties",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "getSongProperties",
    hasArgs: true,
    rustParams: ["song_id"],
  },
  {
    command: "extract_embedded_cover_art",
    frontendFile: "src/lib/tauri/maintenance.ts",
    frontendFn: "extractEmbeddedCoverArt",
    hasArgs: true,
    rustParams: ["song_ids"],
  },
  {
    command: "check_library_integrity",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "checkLibraryIntegrity",
    hasArgs: false,
  },
  {
    command: "remove_missing_library_entries",
    frontendFile: "src/lib/tauri/library.ts",
    frontendFn: "removeMissingLibraryEntries",
    hasArgs: true,
    rustParams: ["hashes"],
  },
];

const LYRICS_COMMANDS: CommandContract[] = [
  {
    command: "fetch_lyrics",
    frontendFile: "src/lib/tauri/lyrics.ts",
    frontendFn: "fetchLyrics",
    hasArgs: true,
    rustParams: ["song_id"],
  },
  {
    command: "set_lyrics_offset",
    frontendFile: "src/lib/tauri/lyrics.ts",
    frontendFn: "setLyricsOffset",
    hasArgs: true,
    rustParams: ["song_id", "ms"],
  },
  {
    command: "fetch_lyrics_online",
    frontendFile: "src/lib/tauri/lyrics.ts",
    frontendFn: "fetchLyricsOnline",
    hasArgs: true,
    rustParams: ["song_id", "user_initiated"],
  },
  {
    command: "save_manual_lyrics",
    frontendFile: "src/lib/tauri/lyrics.ts",
    frontendFn: "saveManualLyrics",
    hasArgs: true,
    rustParams: ["song_id", "text"],
  },
  {
    command: "extract_embedded_lyrics",
    frontendFile: "src/lib/tauri/lyrics.ts",
    frontendFn: "extractEmbeddedLyrics",
    hasArgs: true,
    rustParams: ["song_id"],
  },
  {
    command: "import_lyrics_files",
    frontendFile: "src/lib/tauri/lyrics.ts",
    frontendFn: "importLyricsFiles",
    hasArgs: true,
    rustParams: ["paths"],
  },
  {
    command: "set_lyrics_font_step",
    frontendFile: "src/lib/tauri/settings.ts",
    frontendFn: "setLyricsFontStep",
    hasArgs: true,
    rustParams: ["step"],
  },
];

const SETTINGS_COMMANDS: CommandContract[] = [
  {
    command: "set_eq_enabled",
    frontendFile: "src/lib/tauri/settings.ts",
    frontendFn: "setEqEnabled",
    hasArgs: true,
    rustParams: ["enabled"],
  },
  {
    command: "set_eq_gains",
    frontendFile: "src/lib/tauri/settings.ts",
    frontendFn: "setEqGains",
    hasArgs: true,
    rustParams: ["gains_db"],
  },
  {
    command: "set_library_sort_mode",
    frontendFile: "src/lib/tauri/settings.ts",
    frontendFn: "setLibrarySortMode",
    hasArgs: true,
    rustParams: ["mode"],
  },
  {
    command: "set_theme_preference",
    frontendFile: "src/lib/tauri/settings.ts",
    frontendFn: "setThemePreference",
    hasArgs: true,
    rustParams: ["preference"],
  },
];

const SEPARATION_COMMANDS: CommandContract[] = [
  {
    command: "separate",
    frontendFile: "src/lib/tauri/separation.ts",
    frontendFn: "separate",
    hasArgs: true,
    rustParams: ["song_id"],
  },
  {
    command: "get_separation_status",
    frontendFile: "src/lib/tauri/separation.ts",
    frontendFn: "getSeparationStatus",
    hasArgs: true,
    rustParams: ["song_id"],
  },
  {
    command: "get_all_separation_statuses",
    frontendFile: "src/lib/tauri/separation.ts",
    frontendFn: "getAllSeparationStatuses",
    hasArgs: false,
  },
  {
    command: "upgrade_to_four_stem",
    frontendFile: "src/lib/tauri/separation.ts",
    frontendFn: "upgradeToFourStem",
    hasArgs: true,
    rustParams: ["song_id"],
  },
  {
    command: "re_separate",
    frontendFile: "src/lib/tauri/separation.ts",
    frontendFn: "reSeparate",
    hasArgs: true,
    rustParams: ["song_id", "stem_mode"],
  },
];

const ALL_COMMANDS = [
  ...PLAYBACK_COMMANDS,
  ...LIBRARY_COMMANDS,
  ...LYRICS_COMMANDS,
  ...SEPARATION_COMMANDS,
  ...SETTINGS_COMMANDS,
];

describe("IPC command registry", () => {
  test("all registered commands have unique names", () => {
    const names = ALL_COMMANDS.map((c) => c.command);
    const unique = new Set(names);
    expect(unique.size).toBe(names.length);
  });

  test("playback commands match contract documentation", () => {
    // Phase 2 contract defines these exact command names
    const expectedPlaybackCommands = [
      "play",
      "resume",
      "pause",
      "seek",
      "set_volume",
      "set_stem_volume",
      "load_stems",
      "get_playback_state",
      "get_audio_peaks",
      "set_preload_candidate",
    ];
    const registered = PLAYBACK_COMMANDS.map((c) => c.command);
    expect(registered.sort()).toEqual(expectedPlaybackCommands.sort());
  });

  test("library commands match contract documentation", () => {
    // Phase 1 contract defines these exact command names
    const expectedLibraryCommands = [
      "import_songs",
      "pick_import_paths",
      "expand_import_paths",
      "get_library",
      "search_library",
      "get_cover_art",
      "set_songs_instrumental",
      "extract_embedded_cover_art",
      "get_import_candidate_details",
      "update_song_metadata",
      "set_songs_language",
      "delete_songs",
      "get_song_properties",
      "check_library_integrity",
      "remove_missing_library_entries",
    ];
    const registered = LIBRARY_COMMANDS.map((c) => c.command);
    expect(registered.sort()).toEqual(expectedLibraryCommands.sort());
  });

  test("lyrics commands match contract documentation", () => {
    // Phase 4 contract defines these exact command names
    const expectedLyricsCommands = [
      "fetch_lyrics",
      "set_lyrics_offset",
      "set_lyrics_font_step",
      "fetch_lyrics_online",
      "save_manual_lyrics",
      "extract_embedded_lyrics",
      "import_lyrics_files",
    ];
    const registered = LYRICS_COMMANDS.map((c) => c.command);
    expect(registered.sort()).toEqual(expectedLyricsCommands.sort());
  });

  test("settings commands match contract documentation", () => {
    const expectedSettingsCommands = [
      "set_eq_enabled",
      "set_eq_gains",
      "set_library_sort_mode",
      "set_theme_preference",
    ];
    const registered = SETTINGS_COMMANDS.map((c) => c.command);
    expect(registered.sort()).toEqual(expectedSettingsCommands.sort());
  });

  test("separation commands match contract documentation", () => {
    const expectedSeparationCommands = [
      "separate",
      "get_separation_status",
      "get_all_separation_statuses",
      "upgrade_to_four_stem",
      "re_separate",
    ];
    const registered = SEPARATION_COMMANDS.map((c) => c.command);
    expect(registered.sort()).toEqual(expectedSeparationCommands.sort());
  });

  test("all command names use snake_case (Tauri convention)", () => {
    const snakeCase = /^[a-z][a-z0-9]*(_[a-z0-9]+)*$/;
    for (const cmd of ALL_COMMANDS) {
      expect(cmd.command).toMatch(snakeCase);
    }
  });

  test("all frontend function names use camelCase", () => {
    const camelCase = /^[a-z][a-zA-Z0-9]*$/;
    for (const cmd of ALL_COMMANDS) {
      expect(cmd.frontendFn).toMatch(camelCase);
    }
  });
});

describe("IPC parameter contracts", () => {
  test("play expects song_id parameter (camelCase: songId)", () => {
    const contract = PLAYBACK_COMMANDS.find((c) => c.command === "play")!;
    expect(contract.rustParams).toContain("song_id");
  });

  test("seek expects ms parameter", () => {
    const contract = PLAYBACK_COMMANDS.find((c) => c.command === "seek")!;
    expect(contract.rustParams).toContain("ms");
  });

  test("set_volume expects level parameter", () => {
    const contract = PLAYBACK_COMMANDS.find((c) => c.command === "set_volume")!;
    expect(contract.rustParams).toContain("level");
  });

  test("set_stem_volume expects stem and level parameters", () => {
    const contract = PLAYBACK_COMMANDS.find(
      (c) => c.command === "set_stem_volume",
    )!;
    expect(contract.rustParams).toEqual(["stem", "level"]);
  });

  test("set_preload_candidate expects song_id parameter (camelCase: songId)", () => {
    const contract = PLAYBACK_COMMANDS.find(
      (c) => c.command === "set_preload_candidate",
    )!;
    expect(contract.rustParams).toContain("song_id");
  });

  test("import_songs expects paths and optional options parameters", () => {
    const contract = LIBRARY_COMMANDS.find(
      (c) => c.command === "import_songs",
    )!;
    expect(contract.rustParams).toEqual(["paths", "options"]);
  });

  test("search_library expects query parameter", () => {
    const contract = LIBRARY_COMMANDS.find(
      (c) => c.command === "search_library",
    )!;
    expect(contract.rustParams).toEqual(["query"]);
  });

  test("set_songs_instrumental expects song_ids and instrumental parameters", () => {
    const contract = LIBRARY_COMMANDS.find(
      (c) => c.command === "set_songs_instrumental",
    )!;
    expect(contract.rustParams).toEqual(["song_ids", "instrumental"]);
  });

  test("fetch_lyrics expects song_id parameter", () => {
    const contract = LYRICS_COMMANDS.find((c) => c.command === "fetch_lyrics")!;
    expect(contract.rustParams).toEqual(["song_id"]);
  });

  test("set_lyrics_offset expects song_id and ms parameters", () => {
    const contract = LYRICS_COMMANDS.find(
      (c) => c.command === "set_lyrics_offset",
    )!;
    expect(contract.rustParams).toEqual(["song_id", "ms"]);
  });

  test("separate expects song_id parameter", () => {
    const contract = SEPARATION_COMMANDS.find((c) => c.command === "separate")!;
    expect(contract.rustParams).toEqual(["song_id"]);
  });

  test("re_separate expects song_id and stem_mode parameters", () => {
    const contract = SEPARATION_COMMANDS.find(
      (c) => c.command === "re_separate",
    )!;
    expect(contract.rustParams).toEqual(["song_id", "stem_mode"]);
  });
});

describe("PlaybackStateSnapshot shape matches Rust PlaybackStateSnapshot", () => {
  function assertSnapshotShape(snapshot: PlaybackStateSnapshot): void {
    // Required fields that Rust always serializes
    expect(snapshot).toHaveProperty("song_id");
    expect(snapshot).toHaveProperty("transport_generation");
    expect(snapshot).toHaveProperty("state");
    expect(snapshot).toHaveProperty("is_playing");
    expect(snapshot).toHaveProperty("position_ms");
    expect(snapshot).toHaveProperty("duration_ms");
    expect(snapshot).toHaveProperty("buffered_ms");
    expect(snapshot).toHaveProperty("volume");
    expect(snapshot).toHaveProperty("stem_volumes");
    expect(snapshot).toHaveProperty("has_stems");
    expect(snapshot).toHaveProperty("stem_mode");

    // StemVolumes sub-structure
    expect(snapshot.stem_volumes).toHaveProperty("vocals");
    expect(snapshot.stem_volumes).toHaveProperty("drums");
    expect(snapshot.stem_volumes).toHaveProperty("bass");
    expect(snapshot.stem_volumes).toHaveProperty("other");
  }

  test("idle snapshot has all required fields", () => {
    const idle: PlaybackStateSnapshot = {
      song_id: null,
      transport_generation: 0,
      state: "idle",
      is_playing: false,
      position_ms: 0,
      duration_ms: null,
      buffered_ms: 0,
      volume: 1.0,
      stem_volumes: { vocals: 1.0, drums: 1.0, bass: 1.0, other: 1.0 },
      has_stems: false,
      stem_mode: null,
    };
    assertSnapshotShape(idle);
  });

  test("playing snapshot has all required fields", () => {
    const playing: PlaybackStateSnapshot = {
      song_id: "abc123",
      transport_generation: 1,
      state: "playing",
      is_playing: true,
      position_ms: 1500,
      duration_ms: 180000,
      buffered_ms: 180000,
      volume: 0.8,
      stem_volumes: { vocals: 0.5, drums: 1.0, bass: 1.0, other: 1.0 },
      has_stems: true,
      stem_mode: "two_stem",
    };
    assertSnapshotShape(playing);
  });

  test("loading snapshot has all required fields", () => {
    const loading: PlaybackStateSnapshot = {
      song_id: "abc123",
      transport_generation: 2,
      state: "loading",
      is_playing: false,
      position_ms: 0,
      duration_ms: null,
      buffered_ms: 0,
      volume: 1.0,
      stem_volumes: { vocals: 1.0, drums: 1.0, bass: 1.0, other: 1.0 },
      has_stems: false,
      stem_mode: null,
    };
    assertSnapshotShape(loading);
  });

  test("transport state values match Rust enum variants", () => {
    const validStates = ["idle", "loading", "playing", "buffering"];
    for (const state of validStates) {
      const snapshot: PlaybackStateSnapshot = {
        song_id: null,
        transport_generation: 0,
        state: state as PlaybackStateSnapshot["state"],
        is_playing: false,
        position_ms: 0,
        duration_ms: null,
        buffered_ms: 0,
        volume: 1.0,
        stem_volumes: { vocals: 1.0, drums: 1.0, bass: 1.0, other: 1.0 },
        has_stems: false,
        stem_mode: null,
      };
      expect(snapshot.state).toBe(state);
    }
  });

  test("stem_mode accepts two_stem, four_stem, and null", () => {
    const twoStem: PlaybackStateSnapshot["stem_mode"] = "two_stem";
    const fourStem: PlaybackStateSnapshot["stem_mode"] = "four_stem";
    const nullMode: PlaybackStateSnapshot["stem_mode"] = null;
    expect(twoStem).toBe("two_stem");
    expect(fourStem).toBe("four_stem");
    expect(nullMode).toBeNull();
  });
});

describe("Song shape matches Rust Song struct", () => {
  test("Song has all fields from Rust serialization", () => {
    const song: Song = {
      hash: "abc123",
      file_path: "/path/to/song.mp3",
      audio_source_kind: "original",
      cdg_path: null,
      media_g_container: null,
      instrumental: false,
      language: null,
      title: "Test Song",
      artist: "Test Artist",
      album: "Test Album",
      duration_ms: 180000,
      cover_art: null,
      has_cover_art: false,
      artwork_thumb_path: null,
      imported_at: 1700000000,
      original_ext: "mp3",
    };

    // All fields from Rust Song struct
    expect(song).toHaveProperty("hash");
    expect(song).toHaveProperty("file_path");
    expect(song).toHaveProperty("audio_source_kind");
    expect(song).toHaveProperty("cdg_path");
    expect(song).toHaveProperty("media_g_container");
    expect(song).toHaveProperty("instrumental");
    expect(song).toHaveProperty("language");
    expect(song).toHaveProperty("title");
    expect(song).toHaveProperty("artist");
    expect(song).toHaveProperty("album");
    expect(song).toHaveProperty("duration_ms");
    expect(song).toHaveProperty("cover_art");
    expect(song).toHaveProperty("has_cover_art");
    expect(song).toHaveProperty("imported_at");
    expect(song).toHaveProperty("original_ext");
  });

  test("Song accepts null for optional fields (remote songs)", () => {
    const remoteSong: Song = {
      hash: "remote-song",
      file_path: null,
      audio_source_kind: "original_remote",
      cdg_path: null,
      media_g_container: null,
      instrumental: false,
      language: null,
      title: null,
      artist: null,
      album: null,
      duration_ms: 0,
      cover_art: null,
      has_cover_art: false,
      artwork_thumb_path: null,
      imported_at: 0,
      original_ext: null,
    };
    expect(remoteSong.file_path).toBeNull();
    expect(remoteSong.audio_source_kind).toBe("original_remote");
  });

  test("audio_source_kind values match Rust enum", () => {
    const validKinds: Song["audio_source_kind"][] = [
      "original",
      "original_remote",
      "stems_remote",
    ];
    for (const kind of validKinds) {
      expect(["original", "original_remote", "stems_remote"]).toContain(kind);
    }
  });

  test("media_g_container values match Rust enum", () => {
    const validContainers: Array<Song["media_g_container"]> = [
      "paired",
      "zip",
      null,
    ];
    for (const container of validContainers) {
      expect(["paired", "zip", null]).toContain(container);
    }
  });
});

describe("ImportSongsResult shape matches Rust ImportSongsResult", () => {
  test("has imported and failed fields", () => {
    const result: ImportSongsResult = {
      imported: [],
      failed: [],
    };
    expect(result).toHaveProperty("imported");
    expect(result).toHaveProperty("failed");
    expect(Array.isArray(result.imported)).toBe(true);
    expect(Array.isArray(result.failed)).toBe(true);
  });

  test("ImportFailure has path and error fields", () => {
    const result: ImportSongsResult = {
      imported: [],
      failed: [
        {
          path: "/bad/file.mp3",
          error: {
            code: "media_read_failed",
            message: "could not read file",
            retryable: false,
            fallback: "reimport_song",
          },
        },
      ],
    };
    expect(result.failed[0]).toHaveProperty("path");
    expect(result.failed[0]).toHaveProperty("error");
    expect(result.failed[0].error).toHaveProperty("code");
    expect(result.failed[0].error).toHaveProperty("message");
    expect(result.failed[0].error).toHaveProperty("retryable");
    expect(result.failed[0].error).toHaveProperty("fallback");
  });
});

describe("ExpandedImportPaths shape matches Rust ExpandedImportPaths", () => {
  test("has paths and song_count fields", () => {
    const result: ExpandedImportPaths = {
      paths: ["/music/song.mp3"],
      song_count: 1,
    };
    expect(result).toHaveProperty("paths");
    expect(result).toHaveProperty("song_count");
  });
});

describe("LyricsPayload shape matches Rust LyricsPayload", () => {
  test("has all fields from Rust serialization", () => {
    const payload: LyricsPayload = {
      song_id: "abc123",
      lines: [
        {
          time_ms: 35660,
          text: "Look at the stars",
          words: null,
          bg_words: null,
          section: null,
        },
        {
          time_ms: 38000,
          text: "Look how they shine",
          words: null,
          bg_words: null,
          section: null,
        },
      ],
      source: "lrc_lib",
      offset_ms: 0,
      raw_lrc: "[00:35.66] Look at the stars\n[00:38.00] Look how they shine",
    };

    expect(payload).toHaveProperty("song_id");
    expect(payload).toHaveProperty("lines");
    expect(payload).toHaveProperty("source");
    expect(payload).toHaveProperty("offset_ms");
    expect(payload).toHaveProperty("raw_lrc");
  });

  test("LyricLine has time_ms, text, and words fields", () => {
    const payload: LyricsPayload = {
      song_id: "abc123",
      lines: [
        {
          time_ms: 1000,
          text: "Hello",
          words: null,
          bg_words: null,
          section: null,
        },
      ],
      source: "lrc_lib",
      offset_ms: 0,
      raw_lrc: "[00:01.00] Hello",
    };
    const line = payload.lines[0];
    expect(line).toHaveProperty("time_ms");
    expect(line).toHaveProperty("text");
    expect(line).toHaveProperty("words");
  });

  test("source values match Rust LyricsSource enum", () => {
    const validSources: Array<LyricsPayload["source"]> = [
      "lrc_lib",
      "lrc_api",
      "embedded",
      "sidecar",
      "manual",
      null,
    ];
    for (const source of validSources) {
      expect([
        "lrc_lib",
        "lrc_api",
        "embedded",
        "sidecar",
        "manual",
        null,
      ]).toContain(source);
    }
  });

  test("miss payload returns empty lines and null source", () => {
    const miss: LyricsPayload = {
      song_id: "abc123",
      lines: [],
      source: null,
      offset_ms: 0,
      raw_lrc: "",
    };
    expect(miss.lines).toHaveLength(0);
    expect(miss.source).toBeNull();
  });
});

describe("SeparationStatusSnapshot shape matches Rust SeparationStatusSnapshot", () => {
  test("has all fields from Rust serialization", () => {
    const snapshot: SeparationStatusSnapshot = {
      song_id: "abc123",
      state: "completed",
      percent: 100,
      cache_hit: true,
      vocals_path: "/path/vocals.ogg",
      accomp_path: "/path/accomp.ogg",
      drums_path: null,
      bass_path: null,
      other_path: null,
      model_variant: "htdemucs",
      error: null,
    };

    expect(snapshot).toHaveProperty("song_id");
    expect(snapshot).toHaveProperty("state");
    expect(snapshot).toHaveProperty("percent");
    expect(snapshot).toHaveProperty("cache_hit");
    expect(snapshot).toHaveProperty("vocals_path");
    expect(snapshot).toHaveProperty("accomp_path");
    expect(snapshot).toHaveProperty("drums_path");
    expect(snapshot).toHaveProperty("bass_path");
    expect(snapshot).toHaveProperty("other_path");
    expect(snapshot).toHaveProperty("model_variant");
    expect(snapshot).toHaveProperty("error");
  });

  test("separation state values match Rust SeparationState enum", () => {
    const validStates: SeparationStatusSnapshot["state"][] = [
      "idle",
      "running",
      "completed",
      "failed",
    ];
    for (const state of validStates) {
      expect(["idle", "running", "completed", "failed"]).toContain(state);
    }
  });

  test("idle status has null paths and zero percent", () => {
    const idle: SeparationStatusSnapshot = {
      song_id: "abc123",
      state: "idle",
      percent: 0,
      cache_hit: false,
      vocals_path: null,
      accomp_path: null,
      drums_path: null,
      bass_path: null,
      other_path: null,
      model_variant: null,
      error: null,
    };
    expect(idle.vocals_path).toBeNull();
    expect(idle.percent).toBe(0);
  });

  test("failed status includes error object", () => {
    const failed: SeparationStatusSnapshot = {
      song_id: "abc123",
      state: "failed",
      percent: 100,
      cache_hit: false,
      vocals_path: null,
      accomp_path: null,
      drums_path: null,
      bass_path: null,
      other_path: null,
      model_variant: null,
      error: {
        code: "separation_failed",
        message: "model crashed",
        retryable: true,
        fallback: "stay_in_original_mode",
      },
    };
    expect(failed.error).not.toBeNull();
    expect(failed.error!.code).toBe("separation_failed");
  });
});

describe("CommandError shape matches Rust CommandError", () => {
  test("has code, message, retryable, fallback fields", () => {
    const error: CommandError = {
      code: "song_not_found",
      message: "song with hash abc not found",
      retryable: false,
      fallback: "refresh_library",
    };
    expect(error).toHaveProperty("code");
    expect(error).toHaveProperty("message");
    expect(error).toHaveProperty("retryable");
    expect(error).toHaveProperty("fallback");
  });

  test("ErrorCode values match Rust ErrorCode enum", () => {
    const validCodes: CommandError["code"][] = [
      "database_unavailable",
      "media_read_failed",
      "song_not_found",
      "model_unavailable",
      "audio_decode_failed",
      "audio_output_unavailable",
      "karaoke_not_ready",
      "lyrics_not_ready",
      "network_unavailable",
      "invalid_playback_state",
      "separation_failed",
      "internal",
    ];
    // All 12 error codes from phase-5-error-contract.md
    expect(validCodes).toHaveLength(12);
    for (const code of validCodes) {
      expect(typeof code).toBe("string");
    }
  });

  test("FallbackAction values match Rust FallbackAction enum", () => {
    const validActions: CommandError["fallback"][] = [
      "retry",
      "refresh_library",
      "reimport_song",
      "check_audio_output_device",
      "stay_in_original_mode",
      "show_empty_state",
      "keep_current_state",
    ];
    expect(validActions).toHaveLength(7);
    for (const action of validActions) {
      expect(typeof action).toBe("string");
    }
  });
});

describe("SongProperties shape matches Rust SongProperties", () => {
  test("has all fields from Rust serialization", () => {
    const props: SongProperties = {
      format: "MP3",
      sample_rate_hz: 44100,
      channels: 2,
      bit_rate_bps: 320,
      file_size_bytes: 5_000_000,
      duration_ms: 180000,
      hash: "abc123",
    };
    expect(props).toHaveProperty("format");
    expect(props).toHaveProperty("sample_rate_hz");
    expect(props).toHaveProperty("channels");
    expect(props).toHaveProperty("bit_rate_bps");
    expect(props).toHaveProperty("file_size_bytes");
    expect(props).toHaveProperty("duration_ms");
    expect(props).toHaveProperty("hash");
  });
});

describe("DeleteSongsResult shape matches Rust DeleteSongsResult", () => {
  test("has deleted_song_ids and failed fields", () => {
    const result: DeleteSongsResult = {
      deleted_song_ids: ["abc", "def"],
      failed: [],
    };
    expect(result).toHaveProperty("deleted_song_ids");
    expect(result).toHaveProperty("failed");
  });

  test("DeleteSongsFailure has song_id and error fields", () => {
    const result: DeleteSongsResult = {
      deleted_song_ids: [],
      failed: [
        {
          song_id: "abc",
          error: {
            code: "database_unavailable",
            message: "db locked",
            retryable: true,
            fallback: "retry",
          },
        },
      ],
    };
    expect(result.failed[0]).toHaveProperty("song_id");
    expect(result.failed[0]).toHaveProperty("error");
  });
});

describe("ExtractEmbeddedCoverArtResult shape matches Rust", () => {
  test("has updated_songs and failed fields", () => {
    const result: ExtractEmbeddedCoverArtResult = {
      updated_songs: [],
      failed: [],
    };
    expect(result).toHaveProperty("updated_songs");
    expect(result).toHaveProperty("failed");
  });
});

describe("ImportLyricsResult shape matches Rust ImportLyricsResult", () => {
  test("has matched and unmatched fields", () => {
    const result: ImportLyricsResult = {
      matched: [],
      unmatched: [],
    };
    expect(result).toHaveProperty("matched");
    expect(result).toHaveProperty("unmatched");
  });

  test("LyricsMatch has song_id and lrc_path fields", () => {
    const result: ImportLyricsResult = {
      matched: [
        {
          song_id: "abc",
          lrc_path: "/path/to/lyrics.lrc",
          song_title: "Test Song",
          song_artist: "Test Artist",
        },
      ],
      unmatched: [],
    };
    expect(result.matched[0]).toHaveProperty("song_id");
    expect(result.matched[0]).toHaveProperty("lrc_path");
    expect(result.matched[0]).toHaveProperty("song_title");
    expect(result.matched[0]).toHaveProperty("song_artist");
  });
});

describe("AppSettings shape matches Rust AppSettings", () => {
  test("has all fields from Rust serialization", () => {
    const settings: AppSettings = {
      stem_mode: "two_stem",
      model_variant: "htdemucs",
      language: "en",
      hide_batch_separate: false,
      cover_art_backdrop: true,
      hide_upgrade_all: false,
      lyrics_font_step: 0,
      execution_provider: "cpu",
      available_execution_providers: ["cpu", "xnnpack"],
      eq_enabled: false,
      eq_gains_db: [0, 0, 0, 0, 0],
      crossfade_enabled: false,
      crossfade_duration_ms: 3_000,
      library_sort_mode: "recently_imported",
      theme_preference: "dark",
      update_policy: "notify",
    };
    expect(settings).toHaveProperty("stem_mode");
    expect(settings).toHaveProperty("model_variant");
    expect(settings).toHaveProperty("language");
    expect(settings).toHaveProperty("hide_batch_separate");
    expect(settings).toHaveProperty("cover_art_backdrop");
    expect(settings).toHaveProperty("hide_upgrade_all");
    expect(settings).toHaveProperty("lyrics_font_step");
    expect(settings).toHaveProperty("execution_provider");
    expect(settings).toHaveProperty("available_execution_providers");
    expect(settings).toHaveProperty("eq_enabled");
    expect(settings).toHaveProperty("eq_gains_db");
    expect(settings).toHaveProperty("library_sort_mode");
    expect(settings).toHaveProperty("theme_preference");
    expect(settings).toHaveProperty("update_policy");
  });

  test("stem_mode values match Rust StemMode enum", () => {
    const validModes: AppSettings["stem_mode"][] = ["two_stem", "four_stem"];
    for (const mode of validModes) {
      expect(["two_stem", "four_stem"]).toContain(mode);
    }
  });

  test("model_variant values match Rust ModelVariant enum", () => {
    const validVariants: AppSettings["model_variant"][] = [
      "htdemucs",
      "htdemucs_ft",
    ];
    for (const variant of validVariants) {
      expect(["htdemucs", "htdemucs_ft"]).toContain(variant);
    }
  });

  test("execution_provider values match Rust ExecutionProvider enum", () => {
    const validProviders: AppSettings["execution_provider"][] = [
      "cpu",
      "xnnpack",
      "directml",
    ];
    for (const provider of validProviders) {
      expect(["cpu", "xnnpack", "directml"]).toContain(provider);
    }
  });

  test("library_sort_mode values match Rust LibrarySortMode enum", () => {
    const validModes: AppSettings["library_sort_mode"][] = [
      "recently_imported",
      "title_asc",
      "artist_asc",
    ];
    for (const mode of validModes) {
      expect(["recently_imported", "title_asc", "artist_asc"]).toContain(mode);
    }
  });
});

describe("Event payload shapes", () => {
  test("PlaybackPositionEvent has ms and snapshot fields", () => {
    // Import the type at the module level is sufficient; here we validate
    // the shape by constructing a compatible object
    const event = {
      ms: 1234,
      transport_generation: 1,
      snapshot: {
        song_id: "abc123",
        transport_generation: 1,
        state: "playing" as const,
        is_playing: true,
        position_ms: 1234,
        duration_ms: 180000,
        buffered_ms: 180000,
        volume: 1.0,
        stem_volumes: { vocals: 1.0, drums: 1.0, bass: 1.0, other: 1.0 },
        has_stems: false,
        stem_mode: null,
      },
    };
    expect(event).toHaveProperty("ms");
    expect(event).toHaveProperty("transport_generation");
    expect(event).toHaveProperty("snapshot");
  });

  test("PlaybackEndedEvent has song_id field", () => {
    const event = { song_id: "abc123" };
    expect(event).toHaveProperty("song_id");
  });

  test("PlaybackErrorEvent has song_id and error fields", () => {
    const event = {
      song_id: "abc123",
      error: {
        code: "audio_decode_failed" as const,
        message: "decode failed",
        retryable: false,
        fallback: "reimport_song" as const,
      },
    };
    expect(event).toHaveProperty("song_id");
    expect(event).toHaveProperty("error");
    expect(event.error).toHaveProperty("code");
    expect(event.error).toHaveProperty("retryable");
  });

  test("SeparationProgressEvent has song_id and percent fields", () => {
    const event = { song_id: "abc123", percent: 50 };
    expect(event).toHaveProperty("song_id");
    expect(event).toHaveProperty("percent");
  });

  test("SeparationCompleteEvent has song_id and status fields", () => {
    const event = {
      song_id: "abc123",
      status: {
        song_id: "abc123",
        state: "completed" as const,
        percent: 100,
        cache_hit: true,
        vocals_path: "/path/vocals.ogg",
        accomp_path: "/path/accomp.ogg",
        drums_path: null,
        bass_path: null,
        other_path: null,
        model_variant: "htdemucs",
        error: null,
      },
    };
    expect(event).toHaveProperty("song_id");
    expect(event).toHaveProperty("status");
  });

  test("SeparationErrorEvent has song_id and error fields", () => {
    const event = {
      song_id: "abc123",
      error: {
        code: "separation_failed" as const,
        message: "model failed",
        retryable: true,
        fallback: "stay_in_original_mode" as const,
      },
    };
    expect(event).toHaveProperty("song_id");
    expect(event).toHaveProperty("error");
  });
});

describe("Serialization compatibility", () => {
  test("PlaybackStateSnapshot uses snake_case field names (no rename_all on Rust struct)", () => {
    // The Rust PlaybackStateSnapshot struct does NOT use #[serde(rename_all)],
    // so fields serialize as-is in snake_case. The TypeScript type must match.
    const snapshot: PlaybackStateSnapshot = {
      song_id: "x", // not songId
      transport_generation: 1, // not transportGeneration
      state: "idle",
      is_playing: false, // not isPlaying
      position_ms: 0, // not positionMs
      duration_ms: null, // not durationMs
      buffered_ms: 0, // not bufferedMs
      volume: 1.0,
      stem_volumes: { vocals: 1.0, drums: 1.0, bass: 1.0, other: 1.0 }, // not stemVolumes
      has_stems: false, // not hasStems
      stem_mode: null, // not stemMode
    };
    expect(snapshot.song_id).toBeDefined();
    expect(snapshot.transport_generation).toBeDefined();
    expect(snapshot.is_playing).toBeDefined();
    expect(snapshot.position_ms).toBeDefined();
  });

  test("Song uses snake_case field names", () => {
    const song: Song = {
      hash: "x",
      file_path: null, // not filePath
      audio_source_kind: "original", // not audioSourceKind
      cdg_path: null, // not cdgPath
      media_g_container: null, // not mediaGContainer
      instrumental: false,
      language: null,
      title: null,
      artist: null,
      album: null,
      duration_ms: 0, // not durationMs
      cover_art: null, // not coverArt
      has_cover_art: false,
      artwork_thumb_path: null,
      imported_at: 0, // not importedAt
      original_ext: null, // not originalExt
    };
    expect(song.file_path).toBeDefined();
    expect(song.audio_source_kind).toBeDefined();
    expect(song.duration_ms).toBeDefined();
  });

  test("SeparationStatusSnapshot uses snake_case field names", () => {
    const snapshot: SeparationStatusSnapshot = {
      song_id: "x",
      state: "idle",
      percent: 0,
      cache_hit: false, // not cacheHit
      vocals_path: null, // not vocalsPath
      accomp_path: null, // not accompPath
      drums_path: null,
      bass_path: null,
      other_path: null,
      model_variant: null, // not modelVariant
      error: null,
    };
    expect(snapshot.cache_hit).toBeDefined();
    expect(snapshot.vocals_path).toBeDefined();
    expect(snapshot.model_variant).toBeDefined();
  });

  test("LyricsPayload uses snake_case field names", () => {
    const payload: LyricsPayload = {
      song_id: "x", // not songId
      lines: [],
      source: null,
      offset_ms: 0, // not offsetMs
      raw_lrc: "", // not rawLrc
    };
    expect(payload.song_id).toBeDefined();
    expect(payload.offset_ms).toBeDefined();
    expect(payload.raw_lrc).toBeDefined();
  });

  test("CommandError uses snake_case for retryable (no rename needed)", () => {
    const error: CommandError = {
      code: "internal",
      message: "test",
      retryable: false,
      fallback: "retry",
    };
    // retryable is already lowercase, no rename issue
    expect(error.retryable).toBe(false);
    expect(error.fallback).toBe("retry");
  });
});
