// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";
import * as api from "@/lib/tauri";
import { SongListItem } from "./SongListItem";
import { buildSongListContextMenuItems } from "./song-list-item-menu";

const {
  mockLibraryState,
  mockPlayerState,
  mockLyricsState,
  mockSettingsState,
  mockBootstrapState,
} = vi.hoisted(() => ({
  mockBootstrapState: {
    status: null as { state: string } | null,
  },
  mockLibraryState: {
    selectedSongIds: new Set<string>(),
    selectSong: vi.fn(),
    separationStatuses: {},
    uploadStatuses: {},
    batchSeparation: null as null | {
      total: number;
      completed: number;
      skipped: number;
      failed: number;
      current_song_id: string | null;
      current_percent: number;
    },
    songs: [],
    loadLibrary: vi.fn(),
    lastClickedSongId: null,
    extractEmbeddedCoverArt: vi.fn(),
    setSongsInstrumental: vi.fn(),
  },
  mockPlayerState: {
    snapshot: null,
    playSong: vi.fn(),
    loadState: vi.fn(),
  },
  mockLyricsState: {
    songId: null,
    clear: vi.fn(),
  },
  mockSettingsState: {
    close: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, string | number>) =>
      vars?.title ? `${key}:${vars.title}` : key,
  }),
}));

vi.mock("@/stores/bootstrap-store", () => ({
  useBootstrapStore: Object.assign(
    (selector: (state: typeof mockBootstrapState) => unknown) =>
      selector(mockBootstrapState),
    {
      getState: () => ({ loadStatus: vi.fn() }),
    },
  ),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: Object.assign(
    (selector: (state: typeof mockLibraryState) => unknown) =>
      selector(mockLibraryState),
    {
      setState: vi.fn(),
    },
  ),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: Object.assign(
    (selector: (state: typeof mockPlayerState) => unknown) =>
      selector(mockPlayerState),
    {
      getState: () => mockPlayerState,
    },
  ),
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: {
    getState: () => mockLyricsState,
  },
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (selector: (state: typeof mockSettingsState) => unknown) =>
    selector(mockSettingsState),
}));

vi.mock("@/stores/queue-store", () => ({
  useQueueStore: {
    getState: () => ({
      addToQueue: vi.fn(),
      removeSongIds: vi.fn(),
    }),
  },
}));

vi.mock("@/lib/tauri", () => ({
  separate: vi.fn(),
  cancelSeparation: vi.fn(() => Promise.resolve()),
  deleteSongs: vi.fn(),
  batchSeparate: vi.fn(),
  extractEmbeddedLyrics: vi.fn(),
  fetchLyricsOnline: vi.fn(() => Promise.resolve({ lines: [] })),
}));

vi.mock("@/lib/errors", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

vi.mock("./ContextMenu", () => ({
  ContextMenu: ({
    items,
  }: {
    items: Array<{
      label: string;
    }>;
  }) => <div>{items.map((item) => item.label).join(" | ")}</div>,
}));

vi.mock("../Settings/ConfirmationDialog", () => ({
  ConfirmationDialog: () => <div>confirm dialog</div>,
}));

vi.mock("./SongEditDialog", () => ({
  SongEditDialog: () => <div>edit dialog</div>,
}));

vi.mock("./SongPropertiesDialog", () => ({
  SongPropertiesDialog: () => <div>properties dialog</div>,
}));

describe("SongListItem", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    mockLibraryState.batchSeparation = null;
    mockLibraryState.separationStatuses = {};
    mockLibraryState.uploadStatuses = {};
  });

  test("shows a cancel affordance while separating and calls the API on click", () => {
    mockLibraryState.selectedSongIds = new Set();
    mockLibraryState.separationStatuses = {
      "song-cancel": {
        song_id: "song-cancel",
        state: "running",
        percent: 30,
        cache_hit: false,
        vocals_path: null,
        accomp_path: null,
        drums_path: null,
        bass_path: null,
        other_path: null,
        model_variant: null,
        error: null,
      },
    };

    const { getByLabelText } = render(
      <SongListItem
        song={{
          hash: "song-cancel",
          file_path: "Artist/Song.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          title: "Song",
          artist: "Artist",
          album: null,
          duration_ms: 180000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-cancel"]}
      />,
    );

    fireEvent.click(getByLabelText("library.cancelSeparation"));

    expect(api.cancelSeparation).toHaveBeenCalledWith("song-cancel");

    mockLibraryState.separationStatuses = {};
  });

  test("renders media-g badges and duration in the trailing metadata slot", () => {
    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-cdg",
          file_path: "Taylor Swift/22.mp3",
          audio_source_kind: "original",
          cdg_path: "Taylor Swift/22.cdg",
          media_g_container: "paired",
          instrumental: false,
          language: null,
          title: "22 [Z Karaoke]",
          artist: "Taylor Swift",
          album: null,
          duration_ms: 246000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-cdg"]}
      />,
    );

    expect(markup).toContain(">CDG<");
    expect(markup).toContain("4:06");
    expect(markup).not.toContain("2</span>4:06");
  });

  test("renders a compact cover art thumbnail when cover art is available", () => {
    vi.stubGlobal("URL", {
      createObjectURL: vi.fn(() => "blob:cover"),
      revokeObjectURL: vi.fn(),
    });

    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-1",
          file_path: "Brent Faiyaz/Loose Change.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          title: "LOOSE CHANGE",
          artist: "Brent Faiyaz",
          album: null,
          duration_ms: 226000,
          cover_art: [0xff, 0xd8, 0x00],
          has_cover_art: true,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-1"]}
      />,
    );

    expect(markup).toContain("<img");
    expect(markup).toContain('src="blob:cover"');
    expect(markup).toContain("LOOSE CHANGE");
    expect(markup).not.toContain('loading="lazy"');
    expect(markup).not.toContain('decoding="async"');

    vi.unstubAllGlobals();
  });

  test("renders unified row hooks for every song row", () => {
    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-native",
          file_path: "Fuji Kaze/Hachiko.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          title: "Hachiko",
          artist: "Fuji Kaze",
          album: null,
          duration_ms: 270000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-native"]}
      />,
    );

    expect(markup).toContain('data-song-list-item-variant="unified"');
    expect(markup).toContain('data-native-overlay-surface="song-row"');
    expect(markup).toContain("hover:bg-[var(--sidebar-row-overlay-bg)]");
  });

  test("renders selected rows and actions as overlay surfaces", () => {
    mockLibraryState.selectedSongIds = new Set(["song-native-selected"]);

    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-native-selected",
          file_path: "Rina Sawayama/Hold The Girl.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          title: "Hold The Girl",
          artist: "Rina Sawayama",
          album: null,
          duration_ms: 240000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-native-selected"]}
      />,
    );

    expect(markup).toContain("bg-[var(--sidebar-row-selected-bg)]");
    expect(markup).toContain("border-[var(--sidebar-row-selected-border)]");
    expect(markup).toContain('data-selected="true"');
    expect(markup).toContain('data-native-overlay-surface="song-action"');
    expect(markup).toContain("bg-[var(--sidebar-control-bg)]");
    expect(markup).toContain("border-[var(--sidebar-control-border)]");

    mockLibraryState.selectedSongIds = new Set();
  });

  test("renders badges as overlay surfaces instead of opaque fills", () => {
    mockLibraryState.selectedSongIds = new Set();
    mockLibraryState.separationStatuses = {
      "song-native-badges": {
        state: "completed",
        drums_path: "drums.ogg",
      },
    };

    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-native-badges",
          file_path: "Rina Sawayama/Hold The Girl.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: "paired",
          instrumental: false,
          language: null,
          title: "Hold The Girl",
          artist: "Rina Sawayama",
          album: null,
          duration_ms: 240000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-native-badges"]}
      />,
    );

    expect(markup).toContain("bg-[var(--sidebar-row-overlay-bg)]");
    expect(markup).not.toContain("bg-[var(--color-hover)]");

    mockLibraryState.separationStatuses = {};
  });

  test("single-song separation uses a compact status chip without a full progress bar", () => {
    mockLibraryState.selectedSongIds = new Set();
    mockLibraryState.batchSeparation = null;
    mockLibraryState.separationStatuses = {
      "song-single-progress": {
        song_id: "song-single-progress",
        state: "running",
        percent: 55,
        cache_hit: false,
        vocals_path: null,
        accomp_path: null,
        drums_path: null,
        bass_path: null,
        other_path: null,
        model_variant: null,
        error: null,
      },
    };

    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-single-progress",
          file_path: "Rina Sawayama/Hold The Girl.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          title: "Hold The Girl",
          artist: "Rina Sawayama",
          album: null,
          duration_ms: 240000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-single-progress"]}
      />,
    );

    expect(markup).toContain("55%");
    expect(markup).toContain("library.cancelSeparation");
    expect(markup).not.toContain('role="progressbar"');
    expect(markup).not.toContain("h-1 w-full overflow-hidden rounded-full");

    mockLibraryState.separationStatuses = {};
  });

  test("batch separation shows a compact per-song bar under the active row", () => {
    mockLibraryState.selectedSongIds = new Set();
    mockLibraryState.batchSeparation = {
      total: 3,
      completed: 1,
      skipped: 0,
      failed: 0,
      current_song_id: "song-batch-progress",
      current_percent: 40,
    };
    mockLibraryState.separationStatuses = {
      "song-batch-progress": {
        song_id: "song-batch-progress",
        state: "running",
        percent: 40,
        cache_hit: false,
        vocals_path: null,
        accomp_path: null,
        drums_path: null,
        bass_path: null,
        other_path: null,
        model_variant: null,
        error: null,
      },
    };

    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-batch-progress",
          file_path: "Rina Sawayama/Hold The Girl.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          title: "Hold The Girl",
          artist: "Rina Sawayama",
          album: null,
          duration_ms: 240000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-batch-progress"]}
      />,
    );

    expect(markup).toContain("40%");
    expect(markup).toContain('role="progressbar"');
    expect(markup).toContain('aria-label="progress.separating:Hold The Girl"');
    expect(markup).toContain("h-1 w-full overflow-hidden rounded-full");
    expect(markup).not.toContain("library.cancelSeparation");

    mockLibraryState.separationStatuses = {};
    mockLibraryState.batchSeparation = null;
  });

  test("upload progress is a compact chip on the row (full bar is global-only)", () => {
    mockLibraryState.selectedSongIds = new Set();
    mockLibraryState.batchSeparation = null;
    mockLibraryState.separationStatuses = {};
    mockLibraryState.uploadStatuses = {
      "song-upload-progress": {
        song_id: "song-upload-progress",
        state: "running",
        percent: 88,
        remote_library_id: null,
        detail: null,
        error: null,
      },
    };

    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-upload-progress",
          file_path: "Rina Sawayama/Hold The Girl.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          title: "Hold The Girl",
          artist: "Rina Sawayama",
          album: null,
          duration_ms: 240000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-upload-progress"]}
      />,
    );

    expect(markup).toContain("88%");
    expect(markup).not.toContain("progress.uploadingToRemote:Hold The Girl");
    expect(markup).not.toContain('role="progressbar"');

    mockLibraryState.uploadStatuses = {};
  });

  test("renders a compact cover art thumbnail when cover art arrives as Uint8Array", () => {
    vi.stubGlobal("URL", {
      createObjectURL: vi.fn(() => "blob:cover"),
      revokeObjectURL: vi.fn(),
    });

    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-typed-array",
          file_path: "Madvillain/Bistro.m4a",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          title: "Bistro",
          artist: "Madvillain",
          album: null,
          duration_ms: 67000,
          cover_art: new Uint8Array([0xff, 0xd8, 0x00]),
          has_cover_art: true,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "m4a",
        }}
        orderedHashes={["song-typed-array"]}
      />,
    );

    expect(markup).toContain("<img");
    expect(markup).toContain('src="blob:cover"');
    expect(markup).not.toContain('loading="lazy"');
    expect(markup).not.toContain('decoding="async"');

    vi.unstubAllGlobals();
  });

  test("shows extract embedded cover art in the single-song context menu", () => {
    const labels = buildSongListContextMenuItems({
      t: (key: string) => key,
      isMultiSelected: false,
      selectedCount: 1,
      selectedSongIds: ["song-1"],
      selectedHasSeparableSongs: true,
      selectedCanToggleInstrumentalSongs: true,
      selectedInstrumentalState: "unchecked",
      selectedLanguage: null,
      setSelectedLanguage: vi.fn(),
      supportsEmbeddedLyrics: true,
      queueAllSelected: vi.fn(),
      separateAllSelected: vi.fn(),
      toggleSelectedInstrumental: vi.fn(),
      extractSelectedEmbeddedCoverArt: vi.fn(),
      deleteSelected: vi.fn(),
      playNow: vi.fn(),
      playNext: vi.fn(),
      addToQueue: vi.fn(),
      extractEmbeddedCoverArt: vi.fn(),
      extractEmbeddedLyrics: vi.fn(),
      fetchLyricsOnline: vi.fn(),
      editInfo: vi.fn(),
      openProperties: vi.fn(),
      deleteSong: vi.fn(),
      playlists: [],
      songPlaylistMembership: new Map(),
      onAddToPlaylist: vi.fn(),
      onRemoveFromPlaylist: vi.fn(),
      onCreatePlaylistAndAdd: vi.fn(),
      activePlaylistId: null,
      onRemoveFromActivePlaylist: vi.fn(),
    }).map((item) => item.label);

    expect(labels).toContain("library.extractEmbeddedCoverArt");
    expect(labels).toContain("library.extractEmbeddedLyrics");
  });

  test("shows multi-select embedded cover art extraction in the selected context menu", () => {
    const items = buildSongListContextMenuItems({
      t: (key: string, vars?: { count?: number }) =>
        vars?.count ? `${key}:${vars.count}` : key,
      isMultiSelected: true,
      selectedCount: 2,
      selectedSongIds: ["song-1", "song-2"],
      selectedHasSeparableSongs: true,
      selectedCanToggleInstrumentalSongs: true,
      selectedInstrumentalState: "mixed",
      selectedLanguage: null,
      setSelectedLanguage: vi.fn(),
      supportsEmbeddedLyrics: false,
      queueAllSelected: vi.fn(),
      separateAllSelected: vi.fn(),
      toggleSelectedInstrumental: vi.fn(),
      extractSelectedEmbeddedCoverArt: vi.fn(),
      deleteSelected: vi.fn(),
      playNow: vi.fn(),
      playNext: vi.fn(),
      addToQueue: vi.fn(),
      extractEmbeddedCoverArt: vi.fn(),
      extractEmbeddedLyrics: vi.fn(),
      fetchLyricsOnline: vi.fn(),
      editInfo: vi.fn(),
      openProperties: vi.fn(),
      deleteSong: vi.fn(),
      playlists: [],
      songPlaylistMembership: new Map(),
      onAddToPlaylist: vi.fn(),
      onRemoveFromPlaylist: vi.fn(),
      onCreatePlaylistAndAdd: vi.fn(),
      activePlaylistId: null,
      onRemoveFromActivePlaylist: vi.fn(),
    });

    const labels = items.map((item) => item.label);

    expect(labels).toContain("library.markInstrumentalSelected:2");
    expect(labels).toContain("library.extractEmbeddedCoverArtSelected:2");
    expect(labels).not.toContain("library.extractEmbeddedLyrics");
    expect(
      items.find((item) => item.label === "library.markInstrumentalSelected:2")
        ?.indicator,
    ).toBe("mixed");
  });

  test("shows playlist submenu with membership indicators", () => {
    const addToPlaylist = vi.fn();
    const removeFromPlaylist = vi.fn();
    const createPlaylistAndAdd = vi.fn();
    const items = buildSongListContextMenuItems({
      t: (key: string) => key,
      isMultiSelected: false,
      selectedCount: 1,
      selectedSongIds: ["song-1"],
      selectedHasSeparableSongs: false,
      selectedCanToggleInstrumentalSongs: false,
      selectedInstrumentalState: "unchecked",
      selectedLanguage: null,
      setSelectedLanguage: vi.fn(),
      supportsEmbeddedLyrics: false,
      queueAllSelected: vi.fn(),
      separateAllSelected: vi.fn(),
      toggleSelectedInstrumental: vi.fn(),
      extractSelectedEmbeddedCoverArt: vi.fn(),
      deleteSelected: vi.fn(),
      playNow: vi.fn(),
      playNext: vi.fn(),
      addToQueue: vi.fn(),
      extractEmbeddedCoverArt: vi.fn(),
      extractEmbeddedLyrics: vi.fn(),
      fetchLyricsOnline: vi.fn(),
      editInfo: vi.fn(),
      openProperties: vi.fn(),
      deleteSong: vi.fn(),
      playlists: [
        { id: "playlist-1", name: "Favorites" },
        { id: "playlist-2", name: "Duets" },
      ],
      songPlaylistMembership: new Map([
        ["playlist-1", "checked"],
        ["playlist-2", "mixed"],
      ]),
      onAddToPlaylist: addToPlaylist,
      onRemoveFromPlaylist: removeFromPlaylist,
      onCreatePlaylistAndAdd: createPlaylistAndAdd,
      activePlaylistId: null,
      onRemoveFromActivePlaylist: vi.fn(),
    });

    const playlistItem = items.find((item) => item.label === "playlist.addTo");

    expect(playlistItem?.children?.[0]).toMatchObject({
      label: "Favorites",
      indicator: "checked",
    });
    expect(playlistItem?.children?.[1]).toMatchObject({
      label: "Duets",
      indicator: "mixed",
    });

    playlistItem?.children?.[0]?.onClick?.();
    playlistItem?.children?.[1]?.onClick?.();
    playlistItem?.children?.[2]?.onClick?.();

    expect(removeFromPlaylist).toHaveBeenCalledWith("playlist-1");
    expect(addToPlaylist).toHaveBeenCalledWith("playlist-2");
    expect(createPlaylistAndAdd).toHaveBeenCalledOnce();
  });

  test("shows remove action inside an active playlist", () => {
    const removeFromActivePlaylist = vi.fn();
    const items = buildSongListContextMenuItems({
      t: (key: string) => key,
      isMultiSelected: false,
      selectedCount: 1,
      selectedSongIds: ["song-1"],
      selectedHasSeparableSongs: false,
      selectedCanToggleInstrumentalSongs: false,
      selectedInstrumentalState: "unchecked",
      selectedLanguage: null,
      setSelectedLanguage: vi.fn(),
      supportsEmbeddedLyrics: false,
      queueAllSelected: vi.fn(),
      separateAllSelected: vi.fn(),
      toggleSelectedInstrumental: vi.fn(),
      extractSelectedEmbeddedCoverArt: vi.fn(),
      deleteSelected: vi.fn(),
      playNow: vi.fn(),
      playNext: vi.fn(),
      addToQueue: vi.fn(),
      extractEmbeddedCoverArt: vi.fn(),
      extractEmbeddedLyrics: vi.fn(),
      fetchLyricsOnline: vi.fn(),
      editInfo: vi.fn(),
      openProperties: vi.fn(),
      deleteSong: vi.fn(),
      playlists: [],
      songPlaylistMembership: new Map(),
      onAddToPlaylist: vi.fn(),
      onRemoveFromPlaylist: vi.fn(),
      onCreatePlaylistAndAdd: vi.fn(),
      activePlaylistId: "playlist-1",
      onRemoveFromActivePlaylist: removeFromActivePlaylist,
    });

    items
      .find((item) => item.label === "playlist.removeFromPlaylist")
      ?.onClick?.();

    expect(removeFromActivePlaylist).toHaveBeenCalledOnce();
  });

  test("shows a checked instrumental toggle when every selected song is instrumental", () => {
    const items = buildSongListContextMenuItems({
      t: (key: string, vars?: { count?: number }) =>
        vars?.count ? `${key}:${vars.count}` : key,
      isMultiSelected: true,
      selectedCount: 2,
      selectedSongIds: ["song-1", "song-2"],
      selectedHasSeparableSongs: false,
      selectedCanToggleInstrumentalSongs: true,
      selectedInstrumentalState: "checked",
      selectedLanguage: null,
      setSelectedLanguage: vi.fn(),
      supportsEmbeddedLyrics: false,
      queueAllSelected: vi.fn(),
      separateAllSelected: vi.fn(),
      toggleSelectedInstrumental: vi.fn(),
      extractSelectedEmbeddedCoverArt: vi.fn(),
      deleteSelected: vi.fn(),
      playNow: vi.fn(),
      playNext: vi.fn(),
      addToQueue: vi.fn(),
      extractEmbeddedCoverArt: vi.fn(),
      extractEmbeddedLyrics: vi.fn(),
      fetchLyricsOnline: vi.fn(),
      editInfo: vi.fn(),
      openProperties: vi.fn(),
      deleteSong: vi.fn(),
      playlists: [],
      songPlaylistMembership: new Map(),
      onAddToPlaylist: vi.fn(),
      onRemoveFromPlaylist: vi.fn(),
      onCreatePlaylistAndAdd: vi.fn(),
      activePlaylistId: null,
      onRemoveFromActivePlaylist: vi.fn(),
    });

    expect(
      items.find((item) => item.label === "library.markInstrumentalSelected:2")
        ?.indicator,
    ).toBe("checked");
  });

  test("does not render a separate button for instrumental songs", () => {
    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-instrumental",
          file_path: "Artist/Official Instrumental.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: true,
          language: null,
          title: "Official Instrumental",
          artist: "Artist",
          album: null,
          duration_ms: 180000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-instrumental"]}
      />,
    );

    expect(markup).not.toContain("library.separate");
  });

  test("disables the separate button while the model is still downloading", () => {
    mockBootstrapState.status = { state: "downloading" };

    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-preparing",
          file_path: "Artist/Song.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          title: "Song",
          artist: "Artist",
          album: null,
          duration_ms: 180000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-preparing"]}
      />,
    );

    expect(markup).toContain("library.separate");
    expect(markup).toContain("disabled");
    expect(markup).toContain("library.modelPreparing");

    mockBootstrapState.status = null;
  });

  test("keeps the separate button enabled when the model is ready", () => {
    mockBootstrapState.status = { state: "ready" };

    const markup = renderToStaticMarkup(
      <SongListItem
        song={{
          hash: "song-ready",
          file_path: "Artist/Song.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          title: "Song",
          artist: "Artist",
          album: null,
          duration_ms: 180000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        }}
        orderedHashes={["song-ready"]}
      />,
    );

    expect(markup).toContain("library.separate");
    expect(markup).not.toContain("library.modelPreparing");

    mockBootstrapState.status = null;
  });
});
