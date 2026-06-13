import { beforeEach, describe, expect, test, vi } from "vitest";
import type { Song } from "@/types/ipc";

// --- hoisted mocks ----------------------------------------------------------
const {
  mockBatchSeparate,
  mockExtractEmbeddedLyrics,
  mockFetchLyricsOnline,
  mockNotifyError,
  mockNotifySuccess,
  mockSongCanBeSeparated,
  mockSongSupportsInstrumentalFlag,
  mockBuildSongListContextMenuItems,
} = vi.hoisted(() => ({
  mockBatchSeparate: vi.fn(),
  mockExtractEmbeddedLyrics: vi.fn(),
  mockFetchLyricsOnline: vi.fn(),
  mockNotifyError: vi.fn(),
  mockNotifySuccess: vi.fn(),
  mockSongCanBeSeparated: vi.fn().mockReturnValue(false),
  mockSongSupportsInstrumentalFlag: vi.fn().mockReturnValue(false),
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  mockBuildSongListContextMenuItems: vi.fn((args: any) => args),
}));

vi.mock("@/lib/tauri", () => ({
  batchSeparate: mockBatchSeparate,
  extractEmbeddedLyrics: mockExtractEmbeddedLyrics,
  fetchLyricsOnline: mockFetchLyricsOnline,
}));

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
  notifySuccess: mockNotifySuccess,
}));

vi.mock("@/lib/song-media", () => ({
  songCanBeSeparated: mockSongCanBeSeparated,
  songSupportsInstrumentalFlag: mockSongSupportsInstrumentalFlag,
}));

vi.mock("./song-list-item-menu", () => ({
  buildSongListContextMenuItems: mockBuildSongListContextMenuItems,
  SONG_LANGUAGES: ["mandarin", "cantonese", "japanese"],
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: { getState: vi.fn() },
}));
vi.mock("@/stores/playlist-store", () => ({
  usePlaylistStore: { getState: vi.fn() },
}));
vi.mock("@/stores/rotation-store", () => ({
  useRotationStore: { getState: vi.fn() },
}));
vi.mock("@/stores/queue-store", () => ({
  useQueueStore: { getState: vi.fn() },
}));
vi.mock("@/stores/player-store", () => ({
  usePlayerStore: { getState: vi.fn() },
}));
vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: { getState: vi.fn() },
}));

// --- imports after mocks -----------------------------------------------------
import {
  buildSongListContextMenuForSong,
  getSongListContextSongIds,
} from "./song-list-item-context-menu-build";
import { useLibraryStore } from "@/stores/library-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import { useRotationStore } from "@/stores/rotation-store";
import { useQueueStore } from "@/stores/queue-store";
import { usePlayerStore } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";

// --- helpers -----------------------------------------------------------------
function makeSong(overrides: Partial<Song> = {}): Song {
  return {
    hash: "song-abc",
    title: "Test Song",
    artist: "Test Artist",
    album: null,
    file_path: "/music/test.mp3",
    audio_source_kind: "original",
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language: null,
    duration_ms: 180_000,
    cover_art: null,
    has_cover_art: false,
    imported_at: 0,
    original_ext: null,
    ...overrides,
  };
}

const t: (key: string, opts?: Record<string, string | number>) => string = (
  key,
  opts,
) => {
  if (opts?.count !== undefined) return `${key}:${opts.count}`;
  return key;
};

const defaultActions = {
  setEditDialogOpen: vi.fn(),
  setPropertiesDialogOpen: vi.fn(),
  setDeleteSongIds: vi.fn(),
  setPlaylistDialogOpen: vi.fn(),
};

/** Minimal library store state stub */
function libraryState(overrides: Record<string, unknown> = {}) {
  return {
    selectedSongIds: new Set<string>(),
    songs: [] as Song[],
    setSongsLanguage: vi.fn().mockResolvedValue(undefined),
    setSongsInstrumental: vi.fn().mockResolvedValue(undefined),
    extractEmbeddedCoverArt: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

/** Minimal playlist store state stub */
function playlistState(overrides: Record<string, unknown> = {}) {
  return {
    playlists: [] as Array<{ id: string; name: string }>,
    playlistSongSets: new Map<string, Set<string>>(),
    activePlaylistId: null as string | null,
    addSongsToPlaylist: vi.fn().mockResolvedValue(undefined),
    removeSongsFromPlaylist: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

/** Minimal rotation store state stub */
function rotationState(overrides: Record<string, unknown> = {}) {
  return {
    singerNames: [] as string[],
    getNextSinger: vi.fn(),
    assignSingerToQueueEntry: vi.fn(),
    advanceRotation: vi.fn(),
    ...overrides,
  };
}

function queueState(overrides: Record<string, unknown> = {}) {
  return {
    addToQueue: vi.fn(),
    playNext: vi.fn(),
    ...overrides,
  };
}

function playerState(overrides: Record<string, unknown> = {}) {
  return {
    playNow: vi.fn(),
    snapshot: null as { song_id: string } | null,
    ...overrides,
  };
}

function lyricsState(overrides: Record<string, unknown> = {}) {
  return {
    clear: vi.fn(),
    fetchLyrics: vi.fn(),
    ...overrides,
  };
}

// --- tests -------------------------------------------------------------------

beforeEach(() => {
  vi.clearAllMocks();
  // Default store stubs
  (
    useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
  ).mockReturnValue(libraryState());
  (
    usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
  ).mockReturnValue(playlistState());
  (
    useRotationStore.getState as unknown as ReturnType<typeof vi.fn>
  ).mockReturnValue(rotationState());
  (
    useQueueStore.getState as unknown as ReturnType<typeof vi.fn>
  ).mockReturnValue(queueState());
  (
    usePlayerStore.getState as unknown as ReturnType<typeof vi.fn>
  ).mockReturnValue(playerState());
  (
    useLyricsStore.getState as unknown as ReturnType<typeof vi.fn>
  ).mockReturnValue(lyricsState());
});

// =============================================================================
// getSongListContextSongIds
// =============================================================================
describe("getSongListContextSongIds", () => {
  test("returns [song.hash] when the song is not in selectedSongIds", () => {
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(libraryState({ selectedSongIds: new Set(["other"]) }));
    const song = makeSong();

    const result = getSongListContextSongIds(song);

    expect(result).toEqual(["song-abc"]);
  });

  test("returns all selectedSongIds when the song is among them", () => {
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({ selectedSongIds: new Set(["song-abc", "song-xyz"]) }),
    );
    const song = makeSong();

    const result = getSongListContextSongIds(song);

    expect(result).toEqual(expect.arrayContaining(["song-abc", "song-xyz"]));
    expect(result).toHaveLength(2);
  });
});

// =============================================================================
// buildSongListContextMenuForSong
// =============================================================================
describe("buildSongListContextMenuForSong", () => {
  test("calls buildSongListContextMenuItems with isMultiSelected=false for single song", () => {
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.isMultiSelected).toBe(false);
    expect(args.selectedCount).toBe(0);
    expect(args.selectedSongIds).toEqual(["song-abc"]);
  });

  test("passes isMultiSelected=true when multiple songs are selected and clicked song is among them", () => {
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set(["song-abc", "song-def"]),
        songs: [makeSong(), makeSong({ hash: "song-def" })],
      }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.isMultiSelected).toBe(true);
    expect(args.selectedCount).toBe(2);
    expect(args.selectedSongIds).toEqual(
      expect.arrayContaining(["song-abc", "song-def"]),
    );
  });

  test("treats multi-selected songs as single when clicked song is not in selection", () => {
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set(["song-def", "song-ghi"]),
        songs: [makeSong({ hash: "song-def" }), makeSong({ hash: "song-ghi" })],
      }),
    );
    const song = makeSong(); // hash "song-abc" is not selected

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.isMultiSelected).toBe(false);
    expect(args.selectedSongIds).toEqual(["song-abc"]);
  });

  test("supportsEmbeddedLyrics is false when media_g_container is 'zip'", () => {
    const song = makeSong({ media_g_container: "zip" });

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.supportsEmbeddedLyrics).toBe(false);
  });

  test("supportsEmbeddedLyrics is true when media_g_container is null", () => {
    const song = makeSong({ media_g_container: null });

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.supportsEmbeddedLyrics).toBe(true);
  });

  test("selectedInstrumentalState is 'unchecked' when no selected songs support instrumental flag", () => {
    mockSongSupportsInstrumentalFlag.mockReturnValue(false);
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.selectedInstrumentalState).toBe("unchecked");
    expect(args.selectedCanToggleInstrumentalSongs).toBe(false);
  });

  test("selectedInstrumentalState is 'checked' when all instrumental-capable songs are instrumental", () => {
    mockSongSupportsInstrumentalFlag.mockReturnValue(true);
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set<string>(),
        songs: [makeSong({ instrumental: true })],
      }),
    );
    const song = makeSong({ instrumental: true });

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.selectedInstrumentalState).toBe("checked");
    expect(args.selectedCanToggleInstrumentalSongs).toBe(true);
  });

  test("selectedInstrumentalState is 'mixed' when some songs are instrumental and some are not", () => {
    mockSongSupportsInstrumentalFlag.mockReturnValue(true);
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set(["song-abc", "song-def"]),
        songs: [
          makeSong({ instrumental: true }),
          makeSong({ hash: "song-def", instrumental: false }),
        ],
      }),
    );
    const song = makeSong({ instrumental: true });

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.selectedInstrumentalState).toBe("mixed");
  });

  test("selectedLanguage is null when languages differ across selection", () => {
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set(["song-abc", "song-def"]),
        songs: [
          makeSong({ language: "mandarin" }),
          makeSong({ hash: "song-def", language: "japanese" }),
        ],
      }),
    );
    const song = makeSong({ language: "mandarin" });

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.selectedLanguage).toBeNull();
  });

  test("selectedLanguage is set when all songs share the same recognized language", () => {
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set(["song-abc", "song-def"]),
        songs: [
          makeSong({ language: "mandarin" }),
          makeSong({ hash: "song-def", language: "mandarin" }),
        ],
      }),
    );
    const song = makeSong({ language: "mandarin" });

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.selectedLanguage).toBe("mandarin");
  });

  test("selectedLanguage falls back to single song language when no multi-selection", () => {
    const song = makeSong({ language: "japanese" });

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.selectedLanguage).toBe("japanese");
  });

  test("passes playlists and computed membership from the store", () => {
    const playlists = [{ id: "pl-1", name: "Favorites" }];
    const playlistSongSets = new Map([["pl-1", new Set(["song-abc"])]]);

    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playlistState({ playlists, playlistSongSets }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.playlists).toEqual(playlists);
    expect(args.songPlaylistMembership.get("pl-1")).toBe("checked");
  });

  test("activePlaylistId is forwarded from the store", () => {
    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playlistState({ activePlaylistId: "pl-active" }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.activePlaylistId).toBe("pl-active");
  });
});

// =============================================================================
// Action callbacks passed through the context menu args
// =============================================================================
describe("buildSongListContextMenuForSong – action callbacks", () => {
  test("queueAllSelected adds each context song id to the queue", () => {
    const mockAddToQueue = vi.fn();
    (
      useQueueStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(queueState({ addToQueue: mockAddToQueue }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.queueAllSelected();

    expect(mockAddToQueue).toHaveBeenCalledWith("song-abc");
  });

  test("playNow calls usePlayerStore.playNow with the song hash", () => {
    const mockPlayNow = vi.fn();
    (
      usePlayerStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playerState({ playNow: mockPlayNow }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.playNow();

    expect(mockPlayNow).toHaveBeenCalledWith("song-abc");
  });

  test("playNext adds to queue and does NOT assign singer when rotation is inactive", () => {
    const mockPlayNext = vi.fn();
    const mockAssignSinger = vi.fn();
    const mockAdvance = vi.fn();
    (
      useQueueStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(queueState({ playNext: mockPlayNext }));
    (
      useRotationStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      rotationState({
        singerNames: [],
        assignSingerToQueueEntry: mockAssignSinger,
        advanceRotation: mockAdvance,
      }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.playNext();

    expect(mockPlayNext).toHaveBeenCalledWith("song-abc");
    expect(mockAssignSinger).not.toHaveBeenCalled();
    expect(mockAdvance).not.toHaveBeenCalled();
  });

  test("playNext assigns singer and advances rotation when rotation is active", () => {
    const mockPlayNext = vi.fn();
    const mockGetNextSinger = vi.fn().mockReturnValue("Alice");
    const mockAssignSinger = vi.fn();
    const mockAdvance = vi.fn().mockResolvedValue(undefined);
    (
      useQueueStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(queueState({ playNext: mockPlayNext }));
    (
      useRotationStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      rotationState({
        singerNames: ["Alice", "Bob"],
        getNextSinger: mockGetNextSinger,
        assignSingerToQueueEntry: mockAssignSinger,
        advanceRotation: mockAdvance,
      }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.playNext();

    expect(mockPlayNext).toHaveBeenCalledWith("song-abc");
    expect(mockGetNextSinger).toHaveBeenCalled();
    expect(mockAssignSinger).toHaveBeenCalledWith("song-abc", "Alice");
    expect(mockAdvance).toHaveBeenCalled();
  });

  test("addToQueue adds to queue without singer assignment when rotation is inactive", () => {
    const mockAddToQueue = vi.fn();
    (
      useQueueStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(queueState({ addToQueue: mockAddToQueue }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.addToQueue();

    expect(mockAddToQueue).toHaveBeenCalledWith("song-abc");
  });

  test("addToQueue assigns singer and advances rotation when rotation is active", () => {
    const mockAddToQueue = vi.fn();
    const mockGetNextSinger = vi.fn().mockReturnValue("Charlie");
    const mockAssignSinger = vi.fn();
    const mockAdvance = vi.fn().mockResolvedValue(undefined);
    (
      useQueueStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(queueState({ addToQueue: mockAddToQueue }));
    (
      useRotationStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      rotationState({
        singerNames: ["Charlie"],
        getNextSinger: mockGetNextSinger,
        assignSingerToQueueEntry: mockAssignSinger,
        advanceRotation: mockAdvance,
      }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.addToQueue();

    expect(mockAddToQueue).toHaveBeenCalledWith("song-abc");
    expect(mockAssignSinger).toHaveBeenCalledWith("song-abc", "Charlie");
    expect(mockAdvance).toHaveBeenCalled();
  });

  test("editInfo delegates to actions.setEditDialogOpen(true)", () => {
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.editInfo();

    expect(defaultActions.setEditDialogOpen).toHaveBeenCalledWith(true);
  });

  test("openProperties delegates to actions.setPropertiesDialogOpen(true)", () => {
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.openProperties();

    expect(defaultActions.setPropertiesDialogOpen).toHaveBeenCalledWith(true);
  });

  test("deleteSong calls actions.setDeleteSongIds with [song.hash]", () => {
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.deleteSong();

    expect(defaultActions.setDeleteSongIds).toHaveBeenCalledWith(["song-abc"]);
  });

  test("deleteSelected calls actions.setDeleteSongIds with all context ids", () => {
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set(["song-abc", "song-def"]),
        songs: [makeSong(), makeSong({ hash: "song-def" })],
      }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.deleteSelected();

    expect(defaultActions.setDeleteSongIds).toHaveBeenCalledWith(
      expect.arrayContaining(["song-abc", "song-def"]),
    );
  });

  test("extractEmbeddedCoverArt calls library.extractEmbeddedCoverArt with [song.hash]", () => {
    const mockExtract = vi.fn().mockResolvedValue(undefined);
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(libraryState({ extractEmbeddedCoverArt: mockExtract }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.extractEmbeddedCoverArt();

    expect(mockExtract).toHaveBeenCalledWith(["song-abc"]);
  });

  test("extractSelectedEmbeddedCoverArt calls library.extractEmbeddedCoverArt with all selected ids", () => {
    const mockExtract = vi.fn().mockResolvedValue(undefined);
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set(["song-abc", "song-def"]),
        songs: [makeSong(), makeSong({ hash: "song-def" })],
        extractEmbeddedCoverArt: mockExtract,
      }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.extractSelectedEmbeddedCoverArt();

    expect(mockExtract).toHaveBeenCalledWith(
      expect.arrayContaining(["song-abc", "song-def"]),
    );
  });

  test("extractEmbeddedLyrics calls api.extractEmbeddedLyrics with the song hash", () => {
    mockExtractEmbeddedLyrics.mockResolvedValue(undefined);
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.extractEmbeddedLyrics();

    expect(mockExtractEmbeddedLyrics).toHaveBeenCalledWith("song-abc");
  });

  test("separateAllSelected calls api.batchSeparate with selected ids and notifies on error", async () => {
    const error = new Error("separate failed");
    mockBatchSeparate.mockRejectedValue(error);
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set(["song-abc", "song-def"]),
        songs: [makeSong(), makeSong({ hash: "song-def" })],
      }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.separateAllSelected();

    // batchSeparate returns a promise, .catch(notifyError) is attached
    // Wait for the microtask to flush
    await vi.waitFor(() => {
      expect(mockBatchSeparate).toHaveBeenCalledWith(
        expect.arrayContaining(["song-abc", "song-def"]),
      );
    });
  });

  test("toggleSelectedInstrumental calls library.setSongsInstrumental with the instrumental-capable song ids", () => {
    const mockSetInstrumental = vi.fn().mockResolvedValue(undefined);
    mockSongSupportsInstrumentalFlag.mockReturnValue(true);
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set<string>(),
        songs: [makeSong()],
        setSongsInstrumental: mockSetInstrumental,
      }),
    );
    const song = makeSong({ instrumental: false });

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.toggleSelectedInstrumental();

    // instrumental is "unchecked" so next should be true
    expect(mockSetInstrumental).toHaveBeenCalledWith(["song-abc"], true);
  });

  test("setSelectedLanguage calls library.setSongsLanguage with context song ids", () => {
    const mockSetLanguage = vi.fn().mockResolvedValue(undefined);
    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set<string>(),
        songs: [makeSong()],
        setSongsLanguage: mockSetLanguage,
      }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.setSelectedLanguage("mandarin");

    expect(mockSetLanguage).toHaveBeenCalledWith(["song-abc"], "mandarin");
  });

  test("onAddToPlaylist calls playlist store and shows success toast", async () => {
    const mockAdd = vi.fn().mockResolvedValue(undefined);
    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playlistState({ addSongsToPlaylist: mockAdd }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    await args.onAddToPlaylist("pl-1");

    expect(mockAdd).toHaveBeenCalledWith("pl-1", ["song-abc"]);
    expect(mockNotifySuccess).toHaveBeenCalled();
  });

  test("onAddToPlaylist notifies error on failure", async () => {
    const error = new Error("add failed");
    const mockAdd = vi.fn().mockRejectedValue(error);
    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playlistState({ addSongsToPlaylist: mockAdd }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    await args.onAddToPlaylist("pl-1");

    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });

  test("onRemoveFromPlaylist calls playlist store and shows success toast", async () => {
    const mockRemove = vi.fn().mockResolvedValue(undefined);
    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playlistState({ removeSongsFromPlaylist: mockRemove }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    await args.onRemoveFromPlaylist("pl-1");

    expect(mockRemove).toHaveBeenCalledWith("pl-1", ["song-abc"]);
    expect(mockNotifySuccess).toHaveBeenCalled();
  });

  test("onRemoveFromPlaylist notifies error on failure", async () => {
    const error = new Error("remove failed");
    const mockRemove = vi.fn().mockRejectedValue(error);
    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playlistState({ removeSongsFromPlaylist: mockRemove }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    await args.onRemoveFromPlaylist("pl-1");

    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });

  test("onCreatePlaylistAndAdd delegates to actions.setPlaylistDialogOpen(true)", () => {
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.onCreatePlaylistAndAdd();

    expect(defaultActions.setPlaylistDialogOpen).toHaveBeenCalledWith(true);
  });

  test("onRemoveFromActivePlaylist removes song from active playlist and shows toast", async () => {
    const mockRemove = vi.fn().mockResolvedValue(undefined);
    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      playlistState({
        activePlaylistId: "pl-active",
        removeSongsFromPlaylist: mockRemove,
      }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    await args.onRemoveFromActivePlaylist();

    expect(mockRemove).toHaveBeenCalledWith("pl-active", ["song-abc"]);
    expect(mockNotifySuccess).toHaveBeenCalled();
  });

  test("onRemoveFromActivePlaylist does nothing when there is no active playlist", async () => {
    const mockRemove = vi.fn();
    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      playlistState({
        activePlaylistId: null,
        removeSongsFromPlaylist: mockRemove,
      }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    await args.onRemoveFromActivePlaylist();

    expect(mockRemove).not.toHaveBeenCalled();
  });

  test("fetchLyricsOnline refreshes lyrics when the song is currently playing and lyrics are returned", async () => {
    mockFetchLyricsOnline.mockResolvedValue({ lines: [{ text: "hello" }] });
    const mockClear = vi.fn();
    const mockFetchLyrics = vi.fn();
    (
      usePlayerStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playerState({ snapshot: { song_id: "song-abc" } }));
    (
      useLyricsStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      lyricsState({ clear: mockClear, fetchLyrics: mockFetchLyrics }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    await args.fetchLyricsOnline();

    expect(mockFetchLyricsOnline).toHaveBeenCalledWith("song-abc");
    expect(mockClear).toHaveBeenCalled();
    expect(mockFetchLyrics).toHaveBeenCalledWith("song-abc");
  });

  test("fetchLyricsOnline does not refresh lyrics when a different song is playing", async () => {
    mockFetchLyricsOnline.mockResolvedValue({ lines: [{ text: "hello" }] });
    const mockClear = vi.fn();
    const mockFetchLyrics = vi.fn();
    (
      usePlayerStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playerState({ snapshot: { song_id: "other-song" } }));
    (
      useLyricsStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      lyricsState({ clear: mockClear, fetchLyrics: mockFetchLyrics }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    await args.fetchLyricsOnline();

    expect(mockClear).not.toHaveBeenCalled();
    expect(mockFetchLyrics).not.toHaveBeenCalled();
  });

  test("fetchLyricsOnline does not refresh lyrics when returned payload has no lines", async () => {
    mockFetchLyricsOnline.mockResolvedValue({ lines: [] });
    const mockClear = vi.fn();
    const mockFetchLyrics = vi.fn();
    (
      usePlayerStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playerState({ snapshot: { song_id: "song-abc" } }));
    (
      useLyricsStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      lyricsState({ clear: mockClear, fetchLyrics: mockFetchLyrics }),
    );
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    await args.fetchLyricsOnline();

    expect(mockClear).not.toHaveBeenCalled();
  });

  test("fetchLyricsOnline notifies error on failure", async () => {
    const error = new Error("fetch failed");
    mockFetchLyricsOnline.mockRejectedValue(error);
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    args.fetchLyricsOnline();

    await vi.waitFor(() => {
      expect(mockNotifyError).toHaveBeenCalledWith(error);
    });
  });
});

// =============================================================================
// computePlaylistMembership (tested indirectly through buildSongListContextMenuForSong)
// =============================================================================
describe("computePlaylistMembership (indirect)", () => {
  test("returns null for playlists with no song set", () => {
    const playlists = [{ id: "pl-1", name: "Empty" }];
    const playlistSongSets = new Map<string, Set<string>>();

    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playlistState({ playlists, playlistSongSets }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.songPlaylistMembership.get("pl-1")).toBeNull();
  });

  test("returns null when there is no intersection between context songs and playlist", () => {
    const playlists = [{ id: "pl-1", name: "Favorites" }];
    const playlistSongSets = new Map([["pl-1", new Set(["other-song"])]]);

    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playlistState({ playlists, playlistSongSets }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.songPlaylistMembership.get("pl-1")).toBeNull();
  });

  test("returns 'checked' when all context songs are in the playlist", () => {
    const playlists = [{ id: "pl-1", name: "Favorites" }];
    const playlistSongSets = new Map([["pl-1", new Set(["song-abc"])]]);

    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playlistState({ playlists, playlistSongSets }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.songPlaylistMembership.get("pl-1")).toBe("checked");
  });

  test("returns 'mixed' when only some context songs are in the playlist", () => {
    const playlists = [{ id: "pl-1", name: "Favorites" }];
    const playlistSongSets = new Map([["pl-1", new Set(["song-abc"])]]);

    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set(["song-abc", "song-def"]),
        songs: [makeSong(), makeSong({ hash: "song-def" })],
      }),
    );
    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playlistState({ playlists, playlistSongSets }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.songPlaylistMembership.get("pl-1")).toBe("mixed");
  });

  test("returns 'checked' when all multi-selected songs are in the playlist", () => {
    const playlists = [{ id: "pl-1", name: "Favorites" }];
    const playlistSongSets = new Map([
      ["pl-1", new Set(["song-abc", "song-def"])],
    ]);

    (
      useLibraryStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(
      libraryState({
        selectedSongIds: new Set(["song-abc", "song-def"]),
        songs: [makeSong(), makeSong({ hash: "song-def" })],
      }),
    );
    (
      usePlaylistStore.getState as unknown as ReturnType<typeof vi.fn>
    ).mockReturnValue(playlistState({ playlists, playlistSongSets }));
    const song = makeSong();

    buildSongListContextMenuForSong(song, t, defaultActions);

    const args = mockBuildSongListContextMenuItems.mock.calls[0][0];
    expect(args.songPlaylistMembership.get("pl-1")).toBe("checked");
  });
});
