import { beforeEach, describe, expect, test, vi } from "vitest";

const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

import { createTauriBackend } from "@/lib/backend/tauri-backend";
import { tauriInvoke } from "./invoke";

const {
  cdg,
  library,
  librarySetup,
  lyrics,
  maintenance,
  playback,
  playlist,
  remoteRepository,
  separation,
  settings,
} = createTauriBackend(tauriInvoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("library", () => {
  test("importSongs invokes import_songs with paths and options", async () => {
    const result = { imported: [], failed: [] };
    mockInvoke.mockResolvedValueOnce(result);
    const options = {
      explicit_cdg_by_audio_path: {},
      skip_cdg_for_audio_paths: [],
    };
    const returned = await library.importSongs(["/a.mp3"], options);
    expect(mockInvoke).toHaveBeenCalledWith("import_songs", {
      paths: ["/a.mp3"],
      options,
    });
    expect(returned).toBe(result);
  });

  test("importSongs passes undefined options when omitted", async () => {
    mockInvoke.mockResolvedValueOnce({ imported: [], failed: [] });
    await library.importSongs(["/b.mp3"]);
    expect(mockInvoke).toHaveBeenCalledWith("import_songs", {
      paths: ["/b.mp3"],
      options: undefined,
    });
  });

  test("getImportCandidateDetails invokes get_import_candidate_details", async () => {
    const result = [
      {
        path: "/a.mp3",
        format: "mp3",
        bit_rate: 320,
        file_size: 1024,
        duration_ms: 180000,
      },
    ];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.getImportCandidateDetails(["/a.mp3"]);
    expect(mockInvoke).toHaveBeenCalledWith("get_import_candidate_details", {
      paths: ["/a.mp3"],
    });
    expect(returned).toBe(result);
  });

  test("expandImportPaths invokes expand_import_paths", async () => {
    const result = { paths: ["/a.mp3", "/b.mp3"], song_count: 2 };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.expandImportPaths(["/dir"]);
    expect(mockInvoke).toHaveBeenCalledWith("expand_import_paths", {
      paths: ["/dir"],
    });
    expect(returned).toBe(result);
  });

  test("pickImportPaths invokes pick_import_paths with defaultPath", async () => {
    mockInvoke.mockResolvedValueOnce(["/chosen"]);
    const returned = await library.pickImportPaths("/default");
    expect(mockInvoke).toHaveBeenCalledWith("pick_import_paths", {
      defaultPath: "/default",
    });
    expect(returned).toEqual(["/chosen"]);
  });

  test("pickImportPaths passes null when defaultPath is omitted", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    await library.pickImportPaths();
    expect(mockInvoke).toHaveBeenCalledWith("pick_import_paths", {
      defaultPath: null,
    });
  });

  test("getLibrary invokes get_library", async () => {
    const result = [{ hash: "abc", title: "Song" }];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.getLibrary();
    expect(mockInvoke).toHaveBeenCalledWith("get_library");
    expect(returned).toBe(result);
  });

  test("searchLibrary invokes search_library with query", async () => {
    const result = [{ hash: "abc", title: "Match" }];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.searchLibrary("Match");
    expect(mockInvoke).toHaveBeenCalledWith("search_library", {
      query: "Match",
    });
    expect(returned).toBe(result);
  });

  test("getCoverArt invokes get_cover_art", async () => {
    const result = [0xff, 0xd8, 0xff, 0xe0];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.getCoverArt("abc");
    expect(mockInvoke).toHaveBeenCalledWith("get_cover_art", { hash: "abc" });
    expect(returned).toBe(result);
  });

  test("getCoverArtThumbnail invokes get_cover_art with thumb size", async () => {
    const result = [0x52, 0x49, 0x46, 0x46];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.getCoverArtThumbnail("abc");
    expect(mockInvoke).toHaveBeenCalledWith("get_cover_art", {
      hash: "abc",
      size: "thumb",
    });
    expect(returned).toBe(result);
  });

  test("checkLibraryIntegrity invokes check_library_integrity", async () => {
    const result = {
      checked_local_songs: 5,
      skipped_remote_songs: 1,
      missing_primary_media: [],
      empty_primary_media: [],
      missing_optional_assets: [],
      empty_optional_assets: [],
      orphaned_managed_files: [],
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.checkLibraryIntegrity();
    expect(mockInvoke).toHaveBeenCalledWith("check_library_integrity");
    expect(returned).toBe(result);
  });

  test("removeMissingLibraryEntries invokes remove_missing_library_entries", async () => {
    const result = {
      deleted_song_hashes: ["hash1"],
      skipped_song_hashes: ["hash2"],
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.removeMissingLibraryEntries([
      "hash1",
      "hash2",
    ]);
    expect(mockInvoke).toHaveBeenCalledWith("remove_missing_library_entries", {
      hashes: ["hash1", "hash2"],
    });
    expect(returned).toBe(result);
  });

  test("updateSongMetadata invokes update_song_metadata", async () => {
    const result = { hash: "abc", title: "New", artist: "Art" };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.updateSongMetadata("abc", "New", "Art");
    expect(mockInvoke).toHaveBeenCalledWith("update_song_metadata", {
      hash: "abc",
      title: "New",
      artist: "Art",
    });
    expect(returned).toBe(result);
  });

  test("setSongsInstrumental invokes set_songs_instrumental", async () => {
    const result = [{ hash: "abc", instrumental: true }];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.setSongsInstrumental(["abc"], true);
    expect(mockInvoke).toHaveBeenCalledWith("set_songs_instrumental", {
      songIds: ["abc"],
      instrumental: true,
    });
    expect(returned).toBe(result);
  });

  test("setSongsLanguage invokes set_songs_language", async () => {
    const result = [{ hash: "abc", language: "en" }];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.setSongsLanguage(["abc"], "en");
    expect(mockInvoke).toHaveBeenCalledWith("set_songs_language", {
      songIds: ["abc"],
      language: "en",
    });
    expect(returned).toBe(result);
  });

  test("setSongsLanguage passes null language", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    await library.setSongsLanguage(["abc"], null);
    expect(mockInvoke).toHaveBeenCalledWith("set_songs_language", {
      songIds: ["abc"],
      language: null,
    });
  });

  test("deleteSongs invokes delete_songs", async () => {
    const result = { deleted_song_ids: ["abc"], failed: [] };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.deleteSongs(["abc"]);
    expect(mockInvoke).toHaveBeenCalledWith("delete_songs", {
      songIds: ["abc"],
    });
    expect(returned).toBe(result);
  });

  test("getSongProperties invokes get_song_properties", async () => {
    const result = {
      format: "mp3",
      sample_rate: 44100,
      channels: 2,
      bit_rate: 320,
      file_size: 1024,
      duration_ms: 180000,
      hash: "abc",
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await library.getSongProperties("abc");
    expect(mockInvoke).toHaveBeenCalledWith("get_song_properties", {
      songId: "abc",
    });
    expect(returned).toBe(result);
  });
});

describe("playback", () => {
  const snapshot = {
    song_id: "song-1",
    state: "playing" as const,
    is_playing: true,
    position_ms: 0,
    duration_ms: 180000,
    buffered_ms: 180000,
    volume: 0.8,
    stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
    has_stems: false,
    stem_mode: null,
  };

  test("play invokes play with songId", async () => {
    mockInvoke.mockResolvedValueOnce(snapshot);
    const returned = await playback.play("song-1");
    expect(mockInvoke).toHaveBeenCalledWith("play", { songId: "song-1" });
    expect(returned).toBe(snapshot);
  });

  test("resume invokes resume", async () => {
    mockInvoke.mockResolvedValueOnce(snapshot);
    const returned = await playback.resume();
    expect(mockInvoke).toHaveBeenCalledWith("resume");
    expect(returned).toBe(snapshot);
  });

  test("pause invokes pause", async () => {
    mockInvoke.mockResolvedValueOnce(snapshot);
    const returned = await playback.pause();
    expect(mockInvoke).toHaveBeenCalledWith("pause");
    expect(returned).toBe(snapshot);
  });

  test("seek invokes seek with rounded ms", async () => {
    mockInvoke.mockResolvedValueOnce(snapshot);
    const returned = await playback.seek(1234.7);
    expect(mockInvoke).toHaveBeenCalledWith("seek", { ms: 1235 });
    expect(returned).toBe(snapshot);
  });

  test("setVolume invokes set_volume with level", async () => {
    mockInvoke.mockResolvedValueOnce(snapshot);
    const returned = await playback.setVolume(0.5);
    expect(mockInvoke).toHaveBeenCalledWith("set_volume", { level: 0.5 });
    expect(returned).toBe(snapshot);
  });

  test("setStemVolume invokes set_stem_volume", async () => {
    mockInvoke.mockResolvedValueOnce(snapshot);
    const returned = await playback.setStemVolume("vocals", 0.3);
    expect(mockInvoke).toHaveBeenCalledWith("set_stem_volume", {
      stem: "vocals",
      level: 0.3,
    });
    expect(returned).toBe(snapshot);
  });

  test("loadStems invokes load_stems", async () => {
    mockInvoke.mockResolvedValueOnce(snapshot);
    const returned = await playback.loadStems();
    expect(mockInvoke).toHaveBeenCalledWith("load_stems");
    expect(returned).toBe(snapshot);
  });

  test("getPlaybackState invokes get_playback_state", async () => {
    mockInvoke.mockResolvedValueOnce(snapshot);
    const returned = await playback.getPlaybackState();
    expect(mockInvoke).toHaveBeenCalledWith("get_playback_state");
    expect(returned).toBe(snapshot);
  });

  test("getWaveform invokes get_waveform with hash and buckets", async () => {
    const peaks = new Array(200).fill(0.5);
    mockInvoke.mockResolvedValueOnce(peaks);
    const returned = await playback.getWaveform("song-1", 200);
    expect(mockInvoke).toHaveBeenCalledWith("get_waveform", {
      hash: "song-1",
      buckets: 200,
    });
    expect(returned).toEqual({ peaks, buckets: 200 });
  });

  test("getWaveform passes undefined buckets when omitted", async () => {
    const peaks = new Array(200).fill(0.5);
    mockInvoke.mockResolvedValueOnce(peaks);
    const returned = await playback.getWaveform("song-1");
    expect(mockInvoke).toHaveBeenCalledWith("get_waveform", {
      hash: "song-1",
      buckets: undefined,
    });
    expect(returned).toEqual({ peaks, buckets: 200 });
  });

  test("getWaveform returns empty peaks for remote sources", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    const returned = await playback.getWaveform("remote-1", 200);
    expect(returned).toEqual({ peaks: [], buckets: 0 });
  });

  test("setPreloadCandidate invokes set_preload_candidate with songId", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await playback.setPreloadCandidate("song-abc");
    expect(mockInvoke).toHaveBeenCalledWith("set_preload_candidate", {
      songId: "song-abc",
    });
  });

  test("setPreloadCandidate invokes set_preload_candidate with null", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await playback.setPreloadCandidate(null);
    expect(mockInvoke).toHaveBeenCalledWith("set_preload_candidate", {
      songId: null,
    });
  });

  test("syncAirPlayRoutePicker invokes sync_airplay_route_picker", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const bounds = { left_px: 10, top_px: 20, width_px: 100, height_px: 50 };
    const returned = await playback.syncAirPlayRoutePicker(bounds);
    expect(mockInvoke).toHaveBeenCalledWith("sync_airplay_route_picker", {
      bounds,
    });
    expect(returned).toBeUndefined();
  });

  test("syncAirPlayRoutePicker passes null bounds", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await playback.syncAirPlayRoutePicker(null);
    expect(mockInvoke).toHaveBeenCalledWith("sync_airplay_route_picker", {
      bounds: null,
    });
  });

  test("syncAirPlayAudienceState invokes sync_airplay_audience_state", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const payload = {
      mode: "idle" as const,
      songId: null,
      lines: [],
      offsetMs: 0,
      isLoading: false,
      lyricsFontStep: 0,
      messages: {
        selectSong: "",
        loadingLyrics: "",
        noLyrics: "",
        addLyrics: "",
      },
      viewport: { width_px: 1280, height_px: 720, bottom_inset_px: 0 },
      presentationSpec: {
        contentWidthRatio: 0.92,
        contentMaxWidthPx: 1600,
        horizontalPaddingPx: 64,
        verticalPaddingPx: 56,
        lineGapPx: 40,
        fontSizePx: 72,
        lineHeightMultiple: 1.08,
        activeScale: 1.05,
        statusFontSizePx: 18,
        activeGlowBlurPx: 12,
        activeTextColor: { red: 1, green: 1, blue: 1, alpha: 1 },
        pastTextColor: { red: 0, green: 0, blue: 0, alpha: 1 },
        futureTextColor: { red: 0, green: 0, blue: 0, alpha: 1 },
        plainTextColor: { red: 1, green: 1, blue: 1, alpha: 1 },
        statusTextColor: { red: 0.5, green: 0.5, blue: 0.5, alpha: 1 },
        activeGlowColor: { red: 1, green: 1, blue: 1, alpha: 0.8 },
      },
    };
    const returned = await playback.syncAirPlayAudienceState(payload);
    expect(mockInvoke).toHaveBeenCalledWith("sync_airplay_audience_state", {
      payload,
    });
    expect(returned).toBeUndefined();
  });

  test("stepAirPlayPlainTextPage invokes step_airplay_plain_text_page", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await playback.stepAirPlayPlainTextPage("next");
    expect(mockInvoke).toHaveBeenCalledWith("step_airplay_plain_text_page", {
      direction: "next",
    });
    expect(returned).toBeUndefined();
  });
});

describe("settings", () => {
  const appSettings = {
    stem_mode: "two_stem" as const,
    model_variant: "htdemucs" as const,
    language: null,
    hide_batch_separate: false,
    cover_art_backdrop: true,
    hide_upgrade_all: false,
    lyrics_font_step: 0,
    execution_provider: "cpu" as const,
    available_execution_providers: ["cpu" as const],
    compatible_execution_providers: ["cpu" as const],
    eq_enabled: false,
    eq_gains_db: [0, 0, 0, 0, 0] as [number, number, number, number, number],
    crossfade_enabled: false,
    crossfade_duration_ms: 3_000,
    library_sort_mode: "recently_imported" as const,
    theme_preference: "dark" as const,
  };

  test("getModelBootstrapStatus invokes get_model_bootstrap_status", async () => {
    const result = {
      state: "ready" as const,
      model_path: "/model",
      downloaded_bytes: null,
      total_bytes: null,
      error: null,
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await settings.getModelBootstrapStatus();
    expect(mockInvoke).toHaveBeenCalledWith("get_model_bootstrap_status");
    expect(returned).toBe(result);
  });

  test("getSettings invokes get_settings", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.getSettings();
    expect(mockInvoke).toHaveBeenCalledWith("get_settings");
    expect(returned).toBe(appSettings);
  });

  test("getDebugInfo invokes get_debug_info", async () => {
    const debugInfo = { app_version: "0.9.1" };
    mockInvoke.mockResolvedValueOnce(debugInfo);
    const returned = await settings.getDebugInfo();
    expect(mockInvoke).toHaveBeenCalledWith("get_debug_info");
    expect(returned).toBe(debugInfo);
  });

  test("getWindowShellState invokes get_window_shell_state", async () => {
    const result = {
      chrome_variant: "desktop" as const,
      tier: "desktop" as const,
      toolbar_height: 48,
      traffic_light_inset_leading: 0,
      sidebar_header_height: 56,
      sidebar_width: 260,
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await settings.getWindowShellState();
    expect(mockInvoke).toHaveBeenCalledWith("get_window_shell_state");
    expect(returned).toBe(result);
  });

  test("setNativeSidebarVisibility invokes set_native_sidebar_visibility", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await settings.setNativeSidebarVisibility(true);
    expect(mockInvoke).toHaveBeenCalledWith("set_native_sidebar_visibility", {
      visible: true,
    });
    expect(returned).toBeUndefined();
  });

  test("windowReady invokes window_ready", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await settings.windowReady();
    expect(mockInvoke).toHaveBeenCalledWith("window_ready");
    expect(returned).toBeUndefined();
  });

  test("setNativeAppMenuLabels invokes set_native_app_menu_labels", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const labels = {
      file: "File",
      edit: "Edit",
      view: "View",
      window: "Window",
      help: "Help",
      import: "Import",
      settings: "Settings",
      switchLibrary: "Switch Library",
      toggleSidebar: "Toggle Sidebar",
      copyDebugInfo: "Copy debug info",
    };
    const returned = await settings.setNativeAppMenuLabels(labels);
    expect(mockInvoke).toHaveBeenCalledWith("set_native_app_menu_labels", {
      labels,
    });
    expect(returned).toBeUndefined();
  });

  test("setStemMode invokes set_stem_mode", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.setStemMode("four_stem");
    expect(mockInvoke).toHaveBeenCalledWith("set_stem_mode", {
      mode: "four_stem",
    });
    expect(returned).toBe(appSettings);
  });

  test("setModelVariant invokes set_model_variant", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.setModelVariant("htdemucs_ft");
    expect(mockInvoke).toHaveBeenCalledWith("set_model_variant", {
      variant: "htdemucs_ft",
    });
    expect(returned).toBe(appSettings);
  });

  test("downloadModel invokes download_model", async () => {
    const result = {
      state: "downloading" as const,
      model_path: "/model",
      downloaded_bytes: 0,
      total_bytes: 1000,
      error: null,
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await settings.downloadModel("htdemucs_ft");
    expect(mockInvoke).toHaveBeenCalledWith("download_model", {
      variant: "htdemucs_ft",
    });
    expect(returned).toBe(result);
  });

  test("deleteModel invokes delete_model", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await settings.deleteModel("htdemucs_ft");
    expect(mockInvoke).toHaveBeenCalledWith("delete_model", {
      variant: "htdemucs_ft",
    });
    expect(returned).toBeUndefined();
  });

  test("getModelStatus invokes get_model_status", async () => {
    const result = {
      variant: "htdemucs",
      downloaded: true,
      legacy_install_present: false,
      file_size: 1000,
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await settings.getModelStatus("htdemucs");
    expect(mockInvoke).toHaveBeenCalledWith("get_model_status", {
      variant: "htdemucs",
    });
    expect(returned).toBe(result);
  });

  test("setLanguage invokes set_language", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.setLanguage("ja");
    expect(mockInvoke).toHaveBeenCalledWith("set_language", { language: "ja" });
    expect(returned).toBe(appSettings);
  });

  test("setHideBatchSeparate invokes set_hide_batch_separate", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.setHideBatchSeparate(true);
    expect(mockInvoke).toHaveBeenCalledWith("set_hide_batch_separate", {
      value: true,
    });
    expect(returned).toBe(appSettings);
  });

  test("setCoverArtBackdrop invokes set_cover_art_backdrop", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.setCoverArtBackdrop(false);
    expect(mockInvoke).toHaveBeenCalledWith("set_cover_art_backdrop", {
      value: false,
    });
    expect(returned).toBe(appSettings);
  });

  test("setHideUpgradeAll invokes set_hide_upgrade_all", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.setHideUpgradeAll(true);
    expect(mockInvoke).toHaveBeenCalledWith("set_hide_upgrade_all", {
      value: true,
    });
    expect(returned).toBe(appSettings);
  });

  test("setExecutionProvider invokes set_execution_provider", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.setExecutionProvider("xnnpack");
    expect(mockInvoke).toHaveBeenCalledWith("set_execution_provider", {
      provider: "xnnpack",
    });
    expect(returned).toBe(appSettings);
  });

  test("setLyricsFontStep invokes set_lyrics_font_step", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.setLyricsFontStep(2);
    expect(mockInvoke).toHaveBeenCalledWith("set_lyrics_font_step", {
      step: 2,
    });
    expect(returned).toBe(appSettings);
  });

  test("setLibrarySortMode invokes set_library_sort_mode", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.setLibrarySortMode("title_asc");
    expect(mockInvoke).toHaveBeenCalledWith("set_library_sort_mode", {
      mode: "title_asc",
    });
    expect(returned).toBe(appSettings);
  });

  test("setThemePreference invokes set_theme_preference", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.setThemePreference("light");
    expect(mockInvoke).toHaveBeenCalledWith("set_theme_preference", {
      preference: "light",
    });
    expect(returned).toBe(appSettings);
  });

  test("setUpdatePolicy invokes set_update_policy", async () => {
    mockInvoke.mockResolvedValueOnce(appSettings);
    const returned = await settings.setUpdatePolicy("auto_download");
    expect(mockInvoke).toHaveBeenCalledWith("set_update_policy", {
      policy: "auto_download",
    });
    expect(returned).toBe(appSettings);
  });

  test("checkRuntimeUpdates invokes check_runtime_updates", async () => {
    const report = {
      generation: 3,
      release_id: "2026-08-01-001",
      target_triple: "aarch64-apple-darwin",
      state: "up_to_date" as const,
      installed_version: "v1.27.1",
      available_version: "v1.27.1",
      available_bytes: 0,
      restart_required: true,
    };
    mockInvoke.mockResolvedValueOnce(report);
    const returned = await settings.checkRuntimeUpdates();
    expect(mockInvoke).toHaveBeenCalledWith("check_runtime_updates");
    expect(returned).toBe(report);
  });

  test("restartApp invokes restart_app", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await settings.restartApp();
    expect(mockInvoke).toHaveBeenCalledWith("restart_app");
    expect(returned).toBeUndefined();
  });
});

describe("lyrics", () => {
  const lyricsPayload = {
    song_id: "song-1",
    lines: [{ time_ms: 0, text: "Hello", words: null }],
    source: "manual" as const,
    offset_ms: 0,
    raw_lrc: "[00:00.00]Hello",
  };

  test("importLyricsFiles invokes import_lyrics_files", async () => {
    const result = {
      matched: [
        {
          song_id: "song-1",
          lrc_path: "/a.lrc",
          song_title: "Test Song",
          song_artist: "Test Artist",
        },
      ],
      unmatched: [],
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await lyrics.importLyricsFiles(["/a.lrc"]);
    expect(mockInvoke).toHaveBeenCalledWith("import_lyrics_files", {
      paths: ["/a.lrc"],
    });
    expect(returned).toBe(result);
  });

  test("fetchLyrics invokes fetch_lyrics", async () => {
    mockInvoke.mockResolvedValueOnce(lyricsPayload);
    const returned = await lyrics.fetchLyrics("song-1");
    expect(mockInvoke).toHaveBeenCalledWith("fetch_lyrics", {
      songId: "song-1",
    });
    expect(returned).toBe(lyricsPayload);
  });

  test("setLyricsOffset invokes set_lyrics_offset", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await lyrics.setLyricsOffset("song-1", 500);
    expect(mockInvoke).toHaveBeenCalledWith("set_lyrics_offset", {
      songId: "song-1",
      ms: 500,
    });
    expect(returned).toBeUndefined();
  });

  test("saveManualLyrics invokes save_manual_lyrics", async () => {
    mockInvoke.mockResolvedValueOnce(lyricsPayload);
    const returned = await lyrics.saveManualLyrics("song-1", "[00:00.00]Hello");
    expect(mockInvoke).toHaveBeenCalledWith("save_manual_lyrics", {
      songId: "song-1",
      text: "[00:00.00]Hello",
    });
    expect(returned).toBe(lyricsPayload);
  });

  test("extractEmbeddedLyrics invokes extract_embedded_lyrics", async () => {
    mockInvoke.mockResolvedValueOnce(lyricsPayload);
    const returned = await lyrics.extractEmbeddedLyrics("song-1");
    expect(mockInvoke).toHaveBeenCalledWith("extract_embedded_lyrics", {
      songId: "song-1",
    });
    expect(returned).toBe(lyricsPayload);
  });

  test("fetchLyricsOnline invokes fetch_lyrics_online with intent", async () => {
    mockInvoke.mockResolvedValueOnce(lyricsPayload);
    const returned = await lyrics.fetchLyricsOnline("song-1", "user_replace");
    expect(mockInvoke).toHaveBeenCalledWith("fetch_lyrics_online", {
      songId: "song-1",
      intent: "user_replace",
    });
    expect(returned).toBe(lyricsPayload);
  });
});

describe("separation", () => {
  const sepStatus = {
    song_id: "song-1",
    state: "running" as const,
    percent: 50,
    cache_hit: false,
    vocals_path: null,
    accomp_path: null,
    drums_path: null,
    bass_path: null,
    other_path: null,
    model_variant: "htdemucs",
    error: null,
  };

  test("separate invokes separate with songId", async () => {
    mockInvoke.mockResolvedValueOnce(sepStatus);
    const returned = await separation.separate("song-1");
    expect(mockInvoke).toHaveBeenCalledWith("separate", { songId: "song-1" });
    expect(returned).toBe(sepStatus);
  });

  test("cancelSeparation invokes cancel_separation with songId", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await separation.cancelSeparation("song-1");
    expect(mockInvoke).toHaveBeenCalledWith("cancel_separation", {
      songId: "song-1",
    });
  });

  test("getSeparationStatus invokes get_separation_status", async () => {
    mockInvoke.mockResolvedValueOnce(sepStatus);
    const returned = await separation.getSeparationStatus("song-1");
    expect(mockInvoke).toHaveBeenCalledWith("get_separation_status", {
      songId: "song-1",
    });
    expect(returned).toBe(sepStatus);
  });

  test("getAllSeparationStatuses invokes get_all_separation_statuses", async () => {
    const result = [sepStatus];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await separation.getAllSeparationStatuses();
    expect(mockInvoke).toHaveBeenCalledWith("get_all_separation_statuses");
    expect(returned).toBe(result);
  });

  test("upgradeToFourStem invokes upgrade_to_four_stem", async () => {
    mockInvoke.mockResolvedValueOnce(sepStatus);
    const returned = await separation.upgradeToFourStem("song-1");
    expect(mockInvoke).toHaveBeenCalledWith("upgrade_to_four_stem", {
      songId: "song-1",
    });
    expect(returned).toBe(sepStatus);
  });

  test("reSeparate invokes re_separate with songId and stemMode", async () => {
    mockInvoke.mockResolvedValueOnce(sepStatus);
    const returned = await separation.reSeparate("song-1", "four_stem");
    expect(mockInvoke).toHaveBeenCalledWith("re_separate", {
      songId: "song-1",
      stemMode: "four_stem",
    });
    expect(returned).toBe(sepStatus);
  });
});

describe("maintenance", () => {
  test("deleteAllStems invokes delete_all_stems", async () => {
    const result = { deleted_count: 5, freed_bytes: 1024 };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await maintenance.deleteAllStems();
    expect(mockInvoke).toHaveBeenCalledWith("delete_all_stems");
    expect(returned).toBe(result);
  });

  test("estimateStemsSize invokes estimate_stems_size", async () => {
    mockInvoke.mockResolvedValueOnce(2048);
    const returned = await maintenance.estimateStemsSize();
    expect(mockInvoke).toHaveBeenCalledWith("estimate_stems_size");
    expect(returned).toBe(2048);
  });

  test("deleteAllCachedLyrics invokes delete_all_cached_lyrics", async () => {
    mockInvoke.mockResolvedValueOnce(10);
    const returned = await maintenance.deleteAllCachedLyrics();
    expect(mockInvoke).toHaveBeenCalledWith("delete_all_cached_lyrics");
    expect(returned).toBe(10);
  });

  test("extractEmbeddedCoverArt invokes extract_embedded_cover_art", async () => {
    const result = { updated_songs: [], failed: [] };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await maintenance.extractEmbeddedCoverArt([
      "song-1",
      "song-2",
    ]);
    expect(mockInvoke).toHaveBeenCalledWith("extract_embedded_cover_art", {
      songIds: ["song-1", "song-2"],
    });
    expect(returned).toBe(result);
  });

  test("batchSeparate invokes batch_separate", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await maintenance.batchSeparate(["song-1", "song-2"]);
    expect(mockInvoke).toHaveBeenCalledWith("batch_separate", {
      songIds: ["song-1", "song-2"],
    });
    expect(returned).toBeUndefined();
  });

  test("cancelBatchSeparation invokes cancel_batch_separation", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await maintenance.cancelBatchSeparation();
    expect(mockInvoke).toHaveBeenCalledWith("cancel_batch_separation");
    expect(returned).toBeUndefined();
  });

  test("downgradeToTwoStem invokes downgrade_single_to_two_stem", async () => {
    const result = {
      song_id: "song-1",
      state: "completed" as const,
      percent: 100,
      cache_hit: false,
      vocals_path: null,
      accomp_path: null,
      drums_path: null,
      bass_path: null,
      other_path: null,
      model_variant: null,
      error: null,
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await maintenance.downgradeToTwoStem("song-1");
    expect(mockInvoke).toHaveBeenCalledWith("downgrade_single_to_two_stem", {
      songId: "song-1",
    });
    expect(returned).toBe(result);
  });

  test("downgradeAllToTwoStem invokes downgrade_all_to_two_stem", async () => {
    const result = { downgraded_count: 3, freed_bytes: 4096 };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await maintenance.downgradeAllToTwoStem();
    expect(mockInvoke).toHaveBeenCalledWith("downgrade_all_to_two_stem");
    expect(returned).toBe(result);
  });

  test("estimateDowngradeSavings invokes estimate_downgrade_savings", async () => {
    mockInvoke.mockResolvedValueOnce(8192);
    const returned = await maintenance.estimateDowngradeSavings();
    expect(mockInvoke).toHaveBeenCalledWith("estimate_downgrade_savings");
    expect(returned).toBe(8192);
  });
});

describe("playlist", () => {
  const mockPlaylist = {
    id: "pl-1",
    name: "Favourites",
    song_count: 2,
    created_at: 1000,
    updated_at: 2000,
  };

  test("listPlaylists invokes list_playlists", async () => {
    const result = [mockPlaylist];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await playlist.listPlaylists();
    expect(mockInvoke).toHaveBeenCalledWith("list_playlists");
    expect(returned).toBe(result);
  });

  test("createPlaylist invokes create_playlist with name", async () => {
    mockInvoke.mockResolvedValueOnce(mockPlaylist);
    const returned = await playlist.createPlaylist("Favourites");
    expect(mockInvoke).toHaveBeenCalledWith("create_playlist", {
      name: "Favourites",
    });
    expect(returned).toBe(mockPlaylist);
  });

  test("renamePlaylist invokes rename_playlist", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await playlist.renamePlaylist("pl-1", "New Name");
    expect(mockInvoke).toHaveBeenCalledWith("rename_playlist", {
      playlistId: "pl-1",
      name: "New Name",
    });
    expect(returned).toBeUndefined();
  });

  test("deletePlaylist invokes delete_playlist", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await playlist.deletePlaylist("pl-1");
    expect(mockInvoke).toHaveBeenCalledWith("delete_playlist", {
      playlistId: "pl-1",
    });
    expect(returned).toBeUndefined();
  });

  test("addSongsToPlaylist invokes add_songs_to_playlist", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await playlist.addSongsToPlaylist("pl-1", [
      "hash-1",
      "hash-2",
    ]);
    expect(mockInvoke).toHaveBeenCalledWith("add_songs_to_playlist", {
      playlistId: "pl-1",
      songHashes: ["hash-1", "hash-2"],
    });
    expect(returned).toBeUndefined();
  });

  test("removeSongsFromPlaylist invokes remove_songs_from_playlist", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await playlist.removeSongsFromPlaylist("pl-1", ["hash-1"]);
    expect(mockInvoke).toHaveBeenCalledWith("remove_songs_from_playlist", {
      playlistId: "pl-1",
      songHashes: ["hash-1"],
    });
    expect(returned).toBeUndefined();
  });

  test("getPlaylistSongs invokes get_playlist_songs", async () => {
    const result = [
      { song_hash: "hash-1", added_at: 1000, sort_order: 0, singer: null },
    ];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await playlist.getPlaylistSongs("pl-1");
    expect(mockInvoke).toHaveBeenCalledWith("get_playlist_songs", {
      playlistId: "pl-1",
    });
    expect(returned).toBe(result);
  });

  test("setRotationState invokes set_rotation_state", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const rotation = {
      singer_names: ["Alice", "Bob"],
      current_index: 0,
      mode: "round_robin",
      active: true,
    };
    const returned = await playlist.setRotationState(rotation);
    expect(mockInvoke).toHaveBeenCalledWith("set_rotation_state", { rotation });
    expect(returned).toBeUndefined();
  });

  test("getRotationState invokes get_rotation_state", async () => {
    const result = {
      singer_names: ["Alice"],
      current_index: 0,
      mode: "round_robin",
      active: false,
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await playlist.getRotationState();
    expect(mockInvoke).toHaveBeenCalledWith("get_rotation_state");
    expect(returned).toBe(result);
  });

  test("advanceRotation invokes advance_rotation", async () => {
    const result = {
      singer_names: ["Alice", "Bob"],
      current_index: 1,
      mode: "round_robin",
      active: true,
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await playlist.advanceRotation();
    expect(mockInvoke).toHaveBeenCalledWith("advance_rotation");
    expect(returned).toBe(result);
  });

  test("setQueueEntrySinger invokes set_queue_entry_singer", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await playlist.setQueueEntrySinger(
      "pl-1",
      "hash-1",
      "Alice",
    );
    expect(mockInvoke).toHaveBeenCalledWith("set_queue_entry_singer", {
      playlistId: "pl-1",
      songHash: "hash-1",
      singer: "Alice",
    });
    expect(returned).toBeUndefined();
  });

  test("setQueueEntrySinger passes null singer", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await playlist.setQueueEntrySinger("pl-1", "hash-1", null);
    expect(mockInvoke).toHaveBeenCalledWith("set_queue_entry_singer", {
      playlistId: "pl-1",
      songHash: "hash-1",
      singer: null,
    });
  });
});

describe("library-setup", () => {
  const registrySnapshot = {
    active_library_id: "lib-1",
    libraries: [
      {
        id: "lib-1",
        kind: "local" as const,
        display_name: "My Music",
        root_path: "/music",
      },
    ],
  };

  test("getLibraryPath invokes get_library_path", async () => {
    mockInvoke.mockResolvedValueOnce("/music");
    const returned = await librarySetup.getLibraryPath();
    expect(mockInvoke).toHaveBeenCalledWith("get_library_path");
    expect(returned).toBe("/music");
  });

  test("getLibraryPath returns null", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    const returned = await librarySetup.getLibraryPath();
    expect(returned).toBeNull();
  });

  test("getLibraryRegistry invokes get_library_registry", async () => {
    mockInvoke.mockResolvedValueOnce(registrySnapshot);
    const returned = await librarySetup.getLibraryRegistry();
    expect(mockInvoke).toHaveBeenCalledWith("get_library_registry");
    expect(returned).toBe(registrySnapshot);
  });

  test("getActiveLibrary invokes get_active_library", async () => {
    const activeLib = registrySnapshot.libraries[0];
    mockInvoke.mockResolvedValueOnce(activeLib);
    const returned = await librarySetup.getActiveLibrary();
    expect(mockInvoke).toHaveBeenCalledWith("get_active_library");
    expect(returned).toBe(activeLib);
  });

  test("getActiveLibrary returns null when no library is active", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    const returned = await librarySetup.getActiveLibrary();
    expect(returned).toBeNull();
  });

  test("createLocalLibrary invokes create_library with path", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await librarySetup.createLocalLibrary("/music");
    expect(mockInvoke).toHaveBeenCalledWith("create_library", {
      path: "/music",
    });
    expect(returned).toBeUndefined();
  });

  test("registerLocalLibrary invokes open_library with path", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await librarySetup.registerLocalLibrary("/other");
    expect(mockInvoke).toHaveBeenCalledWith("open_library", { path: "/other" });
    expect(returned).toBeUndefined();
  });

  test("switchLibrary invokes switch_library", async () => {
    mockInvoke.mockResolvedValueOnce(registrySnapshot);
    const returned = await librarySetup.switchLibrary("lib-1");
    expect(mockInvoke).toHaveBeenCalledWith("switch_library", {
      libraryId: "lib-1",
    });
    expect(returned).toBe(registrySnapshot);
  });

  test("removeLibrary invokes remove_library", async () => {
    mockInvoke.mockResolvedValueOnce(registrySnapshot);
    const returned = await librarySetup.removeLibrary("lib-1");
    expect(mockInvoke).toHaveBeenCalledWith("remove_library", {
      libraryId: "lib-1",
    });
    expect(returned).toBe(registrySnapshot);
  });

  test("renameLibrary invokes rename_library", async () => {
    mockInvoke.mockResolvedValueOnce(registrySnapshot);
    const returned = await librarySetup.renameLibrary("lib-1", "New Name");
    expect(mockInvoke).toHaveBeenCalledWith("rename_library", {
      libraryId: "lib-1",
      displayName: "New Name",
    });
    expect(returned).toBe(registrySnapshot);
  });

  test("deleteLibrary invokes delete_library", async () => {
    mockInvoke.mockResolvedValueOnce(registrySnapshot);
    const returned = await librarySetup.deleteLibrary("lib-1");
    expect(mockInvoke).toHaveBeenCalledWith("delete_library", {
      libraryId: "lib-1",
    });
    expect(returned).toBe(registrySnapshot);
  });
});

describe("remote-repository", () => {
  const authStart = {
    session_id: "sess-1",
    provider: "google_drive" as const,
    authorization_url: "https://accounts.google.com/auth",
    expires_at_ms: 999999,
  };

  const authStatus = {
    session_id: "sess-1",
    provider: "google_drive" as const,
    state: "ready" as const,
    remote_root_locator: "drive:root",
    display_name: "My Drive",
    error: null,
  };

  test("beginRemoteAuth invokes begin_remote_auth", async () => {
    mockInvoke.mockResolvedValueOnce(authStart);
    const returned = await remoteRepository.beginRemoteAuth("google_drive");
    expect(mockInvoke).toHaveBeenCalledWith("begin_remote_auth", {
      provider: "google_drive",
      payload: null,
    });
    expect(returned).toBe(authStart);
  });

  test("beginRemoteAuth passes WebDAV payload", async () => {
    mockInvoke.mockResolvedValueOnce(authStart);
    const payload = {
      type: "webdav" as const,
      server_url: "https://dav.example.com",
      username: "user",
      password: "pass",
      root_path: null,
    };
    await remoteRepository.beginRemoteAuth("webdav", payload);
    expect(mockInvoke).toHaveBeenCalledWith("begin_remote_auth", {
      provider: "webdav",
      payload,
    });
  });

  test("pollRemoteAuth invokes poll_remote_auth", async () => {
    mockInvoke.mockResolvedValueOnce(authStatus);
    const returned = await remoteRepository.pollRemoteAuth("sess-1");
    expect(mockInvoke).toHaveBeenCalledWith("poll_remote_auth", {
      sessionId: "sess-1",
    });
    expect(returned).toBe(authStatus);
  });

  test("cancelRemoteAuth invokes cancel_remote_auth", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await remoteRepository.cancelRemoteAuth("sess-1");
    expect(mockInvoke).toHaveBeenCalledWith("cancel_remote_auth", {
      sessionId: "sess-1",
    });
    expect(returned).toBeUndefined();
  });

  test("openExternalUrl invokes open_external_url", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await remoteRepository.openExternalUrl(
      "https://example.com",
    );
    expect(mockInvoke).toHaveBeenCalledWith("open_external_url", {
      url: "https://example.com",
    });
    expect(returned).toBeUndefined();
  });

  test("listRemoteLibraryRoots invokes list_remote_library_roots", async () => {
    const result = [
      {
        provider: "google_drive" as const,
        remote_root_locator: "drive:root",
        remote_path_display: "/root",
        display_name: "Root",
        account_id: "acc-1",
      },
    ];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await remoteRepository.listRemoteLibraryRoots("sess-1");
    expect(mockInvoke).toHaveBeenCalledWith("list_remote_library_roots", {
      sessionId: "sess-1",
    });
    expect(returned).toBe(result);
  });

  test("createRemoteLibrary invokes create_remote_library", async () => {
    const result = {
      provider: "google_drive" as const,
      remote_root_locator: "drive:root",
      remote_path_display: "/root",
      display_name: "Music",
      account_id: "acc-1",
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await remoteRepository.createRemoteLibrary(
      "sess-1",
      "Music",
    );
    expect(mockInvoke).toHaveBeenCalledWith("create_remote_library", {
      sessionId: "sess-1",
      displayName: "Music",
    });
    expect(returned).toBe(result);
  });

  test("resolveRemoteLibraryCandidate invokes resolve_remote_library_candidate", async () => {
    const result = {
      provider: "dropbox" as const,
      remote_root_locator: "dbx:root",
      remote_path_display: "/",
      display_name: "Dropbox",
      account_id: "acc-2",
    };
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await remoteRepository.resolveRemoteLibraryCandidate(
      "sess-1",
      "Dropbox",
    );
    expect(mockInvoke).toHaveBeenCalledWith(
      "resolve_remote_library_candidate",
      {
        sessionId: "sess-1",
        displayName: "Dropbox",
      },
    );
    expect(returned).toBe(result);
  });

  test("registerRemoteLibrary invokes register_remote_library", async () => {
    mockInvoke.mockResolvedValueOnce({
      active_library_id: "lib-1",
      libraries: [],
    });
    const returned = await remoteRepository.registerRemoteLibrary(
      "sess-1",
      "drive:root",
      "My Drive",
    );
    expect(mockInvoke).toHaveBeenCalledWith("register_remote_library", {
      sessionId: "sess-1",
      remoteRootLocator: "drive:root",
      displayName: "My Drive",
    });
    expect(returned).toEqual({ active_library_id: "lib-1", libraries: [] });
  });

  test("registerRemoteLibrary passes null displayName when omitted", async () => {
    mockInvoke.mockResolvedValueOnce({
      active_library_id: null,
      libraries: [],
    });
    await remoteRepository.registerRemoteLibrary("sess-1", "drive:root");
    expect(mockInvoke).toHaveBeenCalledWith("register_remote_library", {
      sessionId: "sess-1",
      remoteRootLocator: "drive:root",
      displayName: null,
    });
  });

  test("reauthorizeRemoteRepository invokes reauthorize_remote_repository", async () => {
    mockInvoke.mockResolvedValueOnce({
      active_library_id: "lib-1",
      libraries: [],
    });
    const returned = await remoteRepository.reauthorizeRemoteRepository(
      "lib-1",
      "sess-2",
      "drive:root",
      "Music",
    );
    expect(mockInvoke).toHaveBeenCalledWith("reauthorize_remote_repository", {
      libraryId: "lib-1",
      sessionId: "sess-2",
      remoteRootLocator: "drive:root",
      displayName: "Music",
    });
    expect(returned).toEqual({ active_library_id: "lib-1", libraries: [] });
  });

  test("mirrorLocalLibraryToRemote invokes mirror_local_library_to_remote", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await remoteRepository.mirrorLocalLibraryToRemote(
      "local-1",
      "remote-1",
    );
    expect(mockInvoke).toHaveBeenCalledWith("mirror_local_library_to_remote", {
      localLibraryId: "local-1",
      remoteLibraryId: "remote-1",
    });
    expect(returned).toBeUndefined();
  });

  test("refreshRemoteRepository invokes refresh_remote_repository", async () => {
    mockInvoke.mockResolvedValueOnce({ synced: true });
    const returned = await remoteRepository.refreshRemoteRepository();
    expect(mockInvoke).toHaveBeenCalledWith("refresh_remote_repository");
    expect(returned).toEqual({ synced: true });
  });

  test("publishSongToRemote invokes publish_song_to_remote", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await remoteRepository.publishSongToRemote("song-1");
    expect(mockInvoke).toHaveBeenCalledWith("publish_song_to_remote", {
      songId: "song-1",
    });
    expect(returned).toBeUndefined();
  });

  test("publishSongsToRemote invokes publish_songs_to_remote", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const returned = await remoteRepository.publishSongsToRemote([
      "song-1",
      "song-2",
    ]);
    expect(mockInvoke).toHaveBeenCalledWith("publish_songs_to_remote", {
      songIds: ["song-1", "song-2"],
    });
    expect(returned).toBeUndefined();
  });

  test("getAllUploadStatuses invokes get_all_upload_statuses", async () => {
    const result = [
      {
        song_id: "song-1",
        state: "completed" as const,
        percent: 100,
        error: null,
      },
    ];
    mockInvoke.mockResolvedValueOnce(result);
    const returned = await remoteRepository.getAllUploadStatuses();
    expect(mockInvoke).toHaveBeenCalledWith("get_all_upload_statuses");
    expect(returned).toBe(result);
  });
});

describe("cdg", () => {
  test("getCdgFrame invokes get_cdg_frame with all parameters", async () => {
    const buffer = new ArrayBuffer(288 * 192 * 4);
    mockInvoke.mockResolvedValueOnce(buffer);
    const returned = await cdg.getCdgFrame("song-1", 1, 1234.6, 5);
    expect(mockInvoke).toHaveBeenCalledWith("get_cdg_frame", {
      songId: "song-1",
      transportGeneration: 1,
      positionMs: 1235,
      lastFrameVersion: 5,
    });
    expect(returned).toBe(buffer);
  });

  test("getCdgFrame rounds down for fractional ms", async () => {
    const buffer = new ArrayBuffer(0);
    mockInvoke.mockResolvedValueOnce(buffer);
    await cdg.getCdgFrame("song-1", 1, 100.2, 0);
    expect(mockInvoke).toHaveBeenCalledWith("get_cdg_frame", {
      songId: "song-1",
      transportGeneration: 1,
      positionMs: 100,
      lastFrameVersion: 0,
    });
  });

  test("getCdgStatus invokes get_cdg_status with songId and generation", async () => {
    const status = {
      availability: "ready",
      songId: "s1",
      transportGeneration: 1,
      packetCount: 100,
      errorCode: null,
    };
    mockInvoke.mockResolvedValueOnce(status);
    const returned = await cdg.getCdgStatus("s1", 1);
    expect(mockInvoke).toHaveBeenCalledWith("get_cdg_status", {
      songId: "s1",
      transportGeneration: 1,
    });
    expect(returned).toBe(status);
  });
});
