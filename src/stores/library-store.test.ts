import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { createLibraryStore } from "./library-store";

const {
  mockInvalidateCoverArtUrl,
  mockNotifyError,
  mockNotifySuccess,
  mockRunImportWorkflow,
  mockCreateWebviewSyncChannel,
} = vi.hoisted(() => ({
  mockInvalidateCoverArtUrl: vi.fn(),
  mockNotifyError: vi.fn(),
  mockNotifySuccess: vi.fn(),
  mockRunImportWorkflow: vi.fn(),
  mockCreateWebviewSyncChannel: vi.fn().mockReturnValue({
    publish: vi.fn(),
    subscribe: vi.fn().mockReturnValue(vi.fn()),
    close: vi.fn(),
  }),
}));

const mockUpdateSongMetadata = vi.fn();
const mockExtractEmbeddedCoverArt = vi.fn();
const mockImportSongs = vi.fn();
const mockGetLibrary = vi.fn();
const mockSetSongsInstrumental = vi.fn();
const mockGetActiveLibrary = vi.fn();
const mockRefreshRemoteRepository = vi.fn();
const mockGetAllSeparationStatuses = vi.fn();
const mockGetAllUploadStatuses = vi.fn();
const mockSearchLibrary = vi.fn();
const mockSetSongsLanguage = vi.fn();

const backend = createMockBackend({
  overrides: {
    library: {
      importSongs: mockImportSongs,
      getLibrary: mockGetLibrary,
      updateSongMetadata: mockUpdateSongMetadata,
      setSongsInstrumental: mockSetSongsInstrumental,
      searchLibrary: mockSearchLibrary,
      setSongsLanguage: mockSetSongsLanguage,
    },
    librarySetup: { getActiveLibrary: mockGetActiveLibrary },
    maintenance: { extractEmbeddedCoverArt: mockExtractEmbeddedCoverArt },
    remoteRepository: {
      refreshRemoteRepository: mockRefreshRemoteRepository,
      getAllUploadStatuses: mockGetAllUploadStatuses,
    },
    separation: { getAllSeparationStatuses: mockGetAllSeparationStatuses },
  },
});

const useLibraryStore = createLibraryStore(backend);

vi.mock("@/lib/cover-art", () => ({
  invalidateCoverArtUrl: mockInvalidateCoverArtUrl,
}));

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
  notifySuccess: mockNotifySuccess,
}));

vi.mock("@/lib/i18n", () => ({
  default: { t: (key: string) => key },
}));

vi.mock("@/runtime/import-workflow", async () => {
  const actual = await vi.importActual<
    typeof import("@/runtime/import-workflow")
  >("@/runtime/import-workflow");
  mockRunImportWorkflow.mockImplementation(actual.runImportWorkflow);
  return { runImportWorkflow: mockRunImportWorkflow };
});

vi.mock("@/runtime/webview-sync", () => ({
  createWebviewSyncChannel: mockCreateWebviewSyncChannel,
}));

describe("library-store updateSongMetadata", () => {
  beforeEach(() => {
    mockUpdateSongMetadata.mockReset();
    mockExtractEmbeddedCoverArt.mockReset();
    mockImportSongs.mockReset();
    mockGetLibrary.mockReset();
    mockSetSongsInstrumental.mockReset();
    mockInvalidateCoverArtUrl.mockReset();
    mockNotifyError.mockReset();
    mockGetActiveLibrary.mockReset();
    mockRefreshRemoteRepository.mockReset();
    mockGetAllSeparationStatuses.mockReset();
    mockGetAllUploadStatuses.mockReset();
    mockSearchLibrary.mockReset();
    mockSetSongsLanguage.mockReset();
    mockCreateWebviewSyncChannel.mockReturnValue({
      publish: vi.fn(),
      subscribe: vi.fn().mockReturnValue(vi.fn()),
      close: vi.fn(),
    });
    useLibraryStore.setState({
      songs: [
        {
          hash: "song-1",
          title: "Original Title",
          artist: "Original Artist",
          album: null,
          file_path: "/music/original.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          duration_ms: 123000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: null,
        },
        {
          hash: "song-2",
          title: "Second Song",
          artist: "Second Artist",
          album: null,
          file_path: "/music/second.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          duration_ms: 456000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: null,
        },
      ],
      searchQuery: "",
      isImporting: false,
      importErrors: [],
      selectedSongIds: new Set<string>(),
      lastClickedSongId: null,
      separationStatuses: {},
      uploadStatuses: {},
      filter: "all",
      batchSeparation: null,
      pendingImportCdgChoice: null,
    });
  });

  test("returns true and updates the song on save success", async () => {
    mockUpdateSongMetadata.mockResolvedValue({
      title: "Updated Title",
      artist: "Updated Artist",
    });

    const result = await useLibraryStore
      .getState()
      .updateSongMetadata("song-1", "Updated Title", "Updated Artist");

    expect(result).toBe(true);
    expect(useLibraryStore.getState().songs[0]).toMatchObject({
      title: "Updated Title",
      artist: "Updated Artist",
    });
    expect(mockNotifyError).not.toHaveBeenCalled();
  });

  test("returns false and keeps the current song when save fails", async () => {
    const error = new Error("Save failed");
    mockUpdateSongMetadata.mockRejectedValue(error);

    const result = await useLibraryStore
      .getState()
      .updateSongMetadata("song-1", "Updated Title", "Updated Artist");

    expect(result).toBe(false);
    expect(useLibraryStore.getState().songs[0]).toMatchObject({
      title: "Original Title",
      artist: "Original Artist",
    });
    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });

  test("returns true and updates selected songs when instrumental state changes", async () => {
    mockSetSongsInstrumental.mockResolvedValue([
      {
        hash: "song-1",
        title: "Original Title",
        artist: "Original Artist",
        album: null,
        file_path: "/music/original.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: true,
        language: null,
        duration_ms: 123000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: null,
      },
    ]);

    const result = await useLibraryStore
      .getState()
      .setSongsInstrumental(["song-1"], true);

    expect(result).toBe(true);
    expect(mockSetSongsInstrumental).toHaveBeenCalledWith(["song-1"], true);
    expect(useLibraryStore.getState().songs[0].instrumental).toBe(true);
    expect(mockNotifyError).not.toHaveBeenCalled();
  });

  test("prompts for ambiguous CDG selections before importing songs", async () => {
    const promptForCdgChoice = vi.fn().mockResolvedValue("/tmp/track.flac");

    mockImportSongs.mockResolvedValue({ imported: [], failed: [] });
    mockGetLibrary.mockResolvedValue([]);

    useLibraryStore.setState({ promptForCdgChoice });

    await useLibraryStore
      .getState()
      .importFiles(["/tmp/track.mp3", "/tmp/track.flac", "/tmp/track.cdg"]);

    expect(promptForCdgChoice).toHaveBeenCalledWith({
      cdgPath: "/tmp/track.cdg",
      audioCandidates: ["/tmp/track.flac", "/tmp/track.mp3"],
      stem: "track",
    });
    expect(mockImportSongs).toHaveBeenCalledWith(
      ["/tmp/track.flac", "/tmp/track.cdg"],
      {
        explicit_cdg_by_audio_path: {
          "/tmp/track.flac": "/tmp/track.cdg",
        },
      },
    );
  });

  test("applies only successful cover-art refreshes and reports individual failures", async () => {
    const failure = new Error("missing artwork");
    mockExtractEmbeddedCoverArt.mockResolvedValue({
      updated_songs: [
        {
          hash: "song-1",
          title: "Original Title",
          artist: "Original Artist",
          album: null,
          file_path: "/music/original.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
          instrumental: false,
          language: null,
          duration_ms: 123000,
          cover_art: [0xff, 0xd8, 0x00],
          has_cover_art: true,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: null,
        },
      ],
      failed: [
        {
          song_id: "song-2",
          error: failure,
        },
      ],
    });

    const result = await useLibraryStore
      .getState()
      .extractEmbeddedCoverArt(["song-1", "song-2"]);

    expect(result).toBe(true);
    expect(mockExtractEmbeddedCoverArt).toHaveBeenCalledWith([
      "song-1",
      "song-2",
    ]);
    expect(mockInvalidateCoverArtUrl).toHaveBeenCalledWith("song-1");
    expect(useLibraryStore.getState().songs[0].cover_art).toEqual([
      0xff, 0xd8, 0x00,
    ]);
    expect(useLibraryStore.getState().songs[1].cover_art).toBeNull();
    expect(mockNotifyError).toHaveBeenCalledWith(failure);
  });

  test("returns false when every cover-art extraction fails", async () => {
    const error = new Error("all failed");
    mockExtractEmbeddedCoverArt.mockResolvedValue({
      updated_songs: [],
      failed: [
        {
          song_id: "song-1",
          error,
        },
      ],
    });

    const result = await useLibraryStore
      .getState()
      .extractEmbeddedCoverArt(["song-1"]);

    expect(result).toBe(false);
    expect(useLibraryStore.getState().songs[0].cover_art).toBeNull();
    expect(mockInvalidateCoverArtUrl).not.toHaveBeenCalled();
    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });

  test("tracks upload progress and clears individual upload statuses", () => {
    useLibraryStore.getState().updateUploadStatus({
      song_id: "song-1",
      state: "running",
      percent: 35,
      remote_library_id: null,
      detail: null,
      error: null,
    });

    expect(useLibraryStore.getState().uploadStatuses["song-1"]).toMatchObject({
      state: "running",
      percent: 35,
    });

    useLibraryStore.getState().clearUploadStatus("song-1");

    expect(useLibraryStore.getState().uploadStatuses["song-1"]).toBeUndefined();
  });
});

describe("library-store loadLibrary", () => {
  const songFixture = {
    hash: "song-1",
    title: "Loaded Song",
    artist: "Loaded Artist",
    album: null,
    file_path: "/music/loaded.mp3",
    audio_source_kind: "original" as const,
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language: null,
    duration_ms: 200000,
    cover_art: null,
    has_cover_art: false,
    artwork_thumb_path: null,
    imported_at: 0,
    original_ext: null,
  };

  beforeEach(() => {
    mockGetLibrary.mockReset();
    mockGetActiveLibrary.mockReset();
    mockRefreshRemoteRepository.mockReset();
    mockGetAllSeparationStatuses.mockReset();
    mockGetAllUploadStatuses.mockReset();
    mockNotifyError.mockReset();
    mockCreateWebviewSyncChannel.mockReturnValue({
      publish: vi.fn(),
      subscribe: vi.fn().mockReturnValue(vi.fn()),
      close: vi.fn(),
    });
    useLibraryStore.setState({
      songs: [],
      searchQuery: "",
      isImporting: false,
      importErrors: [],
      selectedSongIds: new Set<string>(),
      lastClickedSongId: null,
      separationStatuses: {},
      uploadStatuses: {},
      filter: "all",
      batchSeparation: null,
      pendingImportCdgChoice: null,
    });
  });

  test("loads songs from getLibrary on success", async () => {
    mockGetActiveLibrary.mockResolvedValue(null);
    mockGetLibrary.mockResolvedValue([songFixture]);
    mockGetAllSeparationStatuses.mockResolvedValue([]);
    mockGetAllUploadStatuses.mockResolvedValue([]);

    await useLibraryStore.getState().loadLibrary();

    expect(useLibraryStore.getState().songs).toEqual([songFixture]);
    expect(mockGetLibrary).toHaveBeenCalled();
    expect(mockNotifyError).not.toHaveBeenCalled();
  });

  test("calls refreshRemoteRepository when active library is remote", async () => {
    mockGetActiveLibrary.mockResolvedValue({ kind: "remote", id: "remote-1" });
    mockRefreshRemoteRepository.mockResolvedValue(undefined);
    mockGetLibrary.mockResolvedValue([]);
    mockGetAllSeparationStatuses.mockResolvedValue([]);
    mockGetAllUploadStatuses.mockResolvedValue([]);

    await useLibraryStore.getState().loadLibrary();

    expect(mockRefreshRemoteRepository).toHaveBeenCalled();
  });

  test("continues loading songs when getActiveLibrary throws", async () => {
    mockGetActiveLibrary.mockRejectedValue(new Error("no active library"));
    mockGetLibrary.mockResolvedValue([songFixture]);
    mockGetAllSeparationStatuses.mockResolvedValue([]);
    mockGetAllUploadStatuses.mockResolvedValue([]);

    await useLibraryStore.getState().loadLibrary();

    expect(useLibraryStore.getState().songs).toEqual([songFixture]);
  });

  test("notifies error when getLibrary throws", async () => {
    const error = new Error("DB unavailable");
    mockGetActiveLibrary.mockResolvedValue(null);
    mockGetLibrary.mockRejectedValue(error);

    await useLibraryStore.getState().loadLibrary();

    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });

  test("hydrates separation statuses from database", async () => {
    const status = {
      song_id: "song-1",
      state: "completed" as const,
      percent: 100,
      cache_hit: false,
      vocals_path: "/stems/vocals.wav",
      accomp_path: null,
      drums_path: null,
      bass_path: null,
      other_path: null,
      model_variant: null,
      error: null,
    };
    mockGetActiveLibrary.mockResolvedValue(null);
    mockGetLibrary.mockResolvedValue([]);
    mockGetAllSeparationStatuses.mockResolvedValue([status]);
    mockGetAllUploadStatuses.mockResolvedValue([]);

    await useLibraryStore.getState().loadLibrary();

    expect(useLibraryStore.getState().separationStatuses["song-1"]).toEqual(
      status,
    );
  });

  test("continues when getAllSeparationStatuses throws", async () => {
    mockGetActiveLibrary.mockResolvedValue(null);
    mockGetLibrary.mockResolvedValue([songFixture]);
    mockGetAllSeparationStatuses.mockRejectedValue(
      new Error("separation table missing"),
    );
    mockGetAllUploadStatuses.mockResolvedValue([]);

    await useLibraryStore.getState().loadLibrary();

    expect(useLibraryStore.getState().songs).toEqual([songFixture]);
    expect(useLibraryStore.getState().separationStatuses).toEqual({});
  });

  test("hydrates upload statuses from database", async () => {
    const upload = {
      song_id: "song-1",
      state: "running" as const,
      percent: 42,
      remote_library_id: null,
      detail: null,
      error: null,
    };
    mockGetActiveLibrary.mockResolvedValue(null);
    mockGetLibrary.mockResolvedValue([]);
    mockGetAllSeparationStatuses.mockResolvedValue([]);
    mockGetAllUploadStatuses.mockResolvedValue([upload]);

    await useLibraryStore.getState().loadLibrary();

    expect(useLibraryStore.getState().uploadStatuses["song-1"]).toEqual(upload);
  });

  test("continues when getAllUploadStatuses throws", async () => {
    mockGetActiveLibrary.mockResolvedValue(null);
    mockGetLibrary.mockResolvedValue([songFixture]);
    mockGetAllSeparationStatuses.mockResolvedValue([]);
    mockGetAllUploadStatuses.mockRejectedValue(
      new Error("upload table missing"),
    );

    await useLibraryStore.getState().loadLibrary();

    expect(useLibraryStore.getState().songs).toEqual([songFixture]);
    expect(useLibraryStore.getState().uploadStatuses).toEqual({});
  });
});

describe("library-store selectSong", () => {
  const orderedHashes = ["a", "b", "c", "d", "e"];

  beforeEach(() => {
    useLibraryStore.setState({
      songs: [],
      searchQuery: "",
      isImporting: false,
      importErrors: [],
      selectedSongIds: new Set<string>(),
      lastClickedSongId: null,
      separationStatuses: {},
      uploadStatuses: {},
      filter: "all",
      batchSeparation: null,
      pendingImportCdgChoice: null,
    });
  });

  test("normal click selects only the clicked song", () => {
    useLibraryStore.setState({
      selectedSongIds: new Set(["x"]),
      lastClickedSongId: "x",
    });

    useLibraryStore
      .getState()
      .selectSong(
        "c",
        { shiftKey: false, metaKey: false, ctrlKey: false },
        orderedHashes,
      );

    expect(useLibraryStore.getState().selectedSongIds).toEqual(new Set(["c"]));
    expect(useLibraryStore.getState().lastClickedSongId).toBe("c");
  });

  test("metaKey toggles a song into the selection", () => {
    useLibraryStore.setState({
      selectedSongIds: new Set(["a"]),
      lastClickedSongId: "a",
    });

    useLibraryStore
      .getState()
      .selectSong(
        "c",
        { shiftKey: false, metaKey: true, ctrlKey: false },
        orderedHashes,
      );

    expect(useLibraryStore.getState().selectedSongIds).toEqual(
      new Set(["a", "c"]),
    );
    expect(useLibraryStore.getState().lastClickedSongId).toBe("c");
  });

  test("ctrlKey toggles a song out of the selection", () => {
    useLibraryStore.setState({
      selectedSongIds: new Set(["a", "c"]),
      lastClickedSongId: "a",
    });

    useLibraryStore
      .getState()
      .selectSong(
        "c",
        { shiftKey: false, metaKey: false, ctrlKey: true },
        orderedHashes,
      );

    expect(useLibraryStore.getState().selectedSongIds).toEqual(new Set(["a"]));
    expect(useLibraryStore.getState().lastClickedSongId).toBe("c");
  });

  test("shiftKey selects a forward range from lastClicked to current", () => {
    useLibraryStore.setState({
      selectedSongIds: new Set(["a"]),
      lastClickedSongId: "a",
    });

    useLibraryStore
      .getState()
      .selectSong(
        "d",
        { shiftKey: true, metaKey: false, ctrlKey: false },
        orderedHashes,
      );

    expect(useLibraryStore.getState().selectedSongIds).toEqual(
      new Set(["a", "b", "c", "d"]),
    );
  });

  test("shiftKey selects a reverse range from lastClicked to current", () => {
    useLibraryStore.setState({
      selectedSongIds: new Set(["d"]),
      lastClickedSongId: "d",
    });

    useLibraryStore
      .getState()
      .selectSong(
        "a",
        { shiftKey: true, metaKey: false, ctrlKey: false },
        orderedHashes,
      );

    expect(useLibraryStore.getState().selectedSongIds).toEqual(
      new Set(["a", "b", "c", "d"]),
    );
  });

  test("shiftKey without lastClickedSongId falls back to normal click", () => {
    useLibraryStore.setState({
      selectedSongIds: new Set(["x"]),
      lastClickedSongId: null,
    });

    useLibraryStore
      .getState()
      .selectSong(
        "b",
        { shiftKey: true, metaKey: false, ctrlKey: false },
        orderedHashes,
      );

    expect(useLibraryStore.getState().selectedSongIds).toEqual(new Set(["b"]));
    expect(useLibraryStore.getState().lastClickedSongId).toBe("b");
  });
});

describe("library-store clearSelection", () => {
  test("clears selectedSongIds and lastClickedSongId", () => {
    useLibraryStore.setState({
      selectedSongIds: new Set(["a", "b"]),
      lastClickedSongId: "a",
    });

    useLibraryStore.getState().clearSelection();

    expect(useLibraryStore.getState().selectedSongIds).toEqual(new Set());
    expect(useLibraryStore.getState().lastClickedSongId).toBeNull();
  });
});

describe("library-store clearRangeSelectionAnchor", () => {
  test("clears only lastClickedSongId, preserving selectedSongIds", () => {
    useLibraryStore.setState({
      selectedSongIds: new Set(["a", "b", "c"]),
      lastClickedSongId: "b",
    });

    useLibraryStore.getState().clearRangeSelectionAnchor();

    expect(useLibraryStore.getState().lastClickedSongId).toBeNull();
    expect(useLibraryStore.getState().selectedSongIds).toEqual(
      new Set(["a", "b", "c"]),
    );
  });

  test("is a no-op when lastClickedSongId is already null", () => {
    useLibraryStore.setState({
      selectedSongIds: new Set(["a"]),
      lastClickedSongId: null,
    });

    const before = useLibraryStore.getState();
    useLibraryStore.getState().clearRangeSelectionAnchor();

    expect(useLibraryStore.getState().lastClickedSongId).toBeNull();
    expect(useLibraryStore.getState().selectedSongIds).toEqual(
      before.selectedSongIds,
    );
  });
});

describe("library-store setFilter", () => {
  test("sets filter to separated", () => {
    useLibraryStore.setState({ filter: "all" });

    useLibraryStore.getState().setFilter("separated");

    expect(useLibraryStore.getState().filter).toBe("separated");
  });

  test("sets filter back to all", () => {
    useLibraryStore.setState({ filter: "separated" });

    useLibraryStore.getState().setFilter("all");

    expect(useLibraryStore.getState().filter).toBe("all");
  });
});

describe("library-store setSongsLanguage", () => {
  const songFixture = {
    hash: "song-1",
    title: "Test Song",
    artist: "Test Artist",
    album: null,
    file_path: "/music/test.mp3",
    audio_source_kind: "original" as const,
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language: null,
    duration_ms: 180000,
    cover_art: null,
    has_cover_art: false,
    artwork_thumb_path: null,
    imported_at: 0,
    original_ext: null,
  };

  beforeEach(() => {
    mockSetSongsLanguage.mockReset();
    mockNotifyError.mockReset();
    mockCreateWebviewSyncChannel.mockReturnValue({
      publish: vi.fn(),
      subscribe: vi.fn().mockReturnValue(vi.fn()),
      close: vi.fn(),
    });
    useLibraryStore.setState({
      songs: [songFixture],
      searchQuery: "",
      isImporting: false,
      importErrors: [],
      selectedSongIds: new Set<string>(),
      lastClickedSongId: null,
      separationStatuses: {},
      uploadStatuses: {},
      filter: "all",
      batchSeparation: null,
      pendingImportCdgChoice: null,
    });
  });

  test("returns true and updates song language on success", async () => {
    const updated = { ...songFixture, language: "en" };
    mockSetSongsLanguage.mockResolvedValue([updated]);

    const result = await useLibraryStore
      .getState()
      .setSongsLanguage(["song-1"], "en");

    expect(result).toBe(true);
    expect(mockSetSongsLanguage).toHaveBeenCalledWith(["song-1"], "en");
    expect(useLibraryStore.getState().songs[0].language).toBe("en");
    expect(mockNotifyError).not.toHaveBeenCalled();
  });

  test("returns false and notifies error on failure", async () => {
    const error = new Error("set language failed");
    mockSetSongsLanguage.mockRejectedValue(error);

    const result = await useLibraryStore
      .getState()
      .setSongsLanguage(["song-1"], "en");

    expect(result).toBe(false);
    expect(useLibraryStore.getState().songs[0].language).toBeNull();
    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });
});

describe("library-store searchSongs", () => {
  beforeEach(() => {
    mockSearchLibrary.mockReset();
    mockNotifyError.mockReset();
    mockCreateWebviewSyncChannel.mockReturnValue({
      publish: vi.fn(),
      subscribe: vi.fn().mockReturnValue(vi.fn()),
      close: vi.fn(),
    });
    useLibraryStore.setState({
      songs: [],
      searchQuery: "",
      isImporting: false,
      importErrors: [],
      selectedSongIds: new Set<string>(),
      lastClickedSongId: null,
      separationStatuses: {},
      uploadStatuses: {},
      filter: "all",
      batchSeparation: null,
      pendingImportCdgChoice: null,
    });
  });

  test("sets songs from search results on success", async () => {
    const results = [
      {
        hash: "found-1",
        title: "Found Song",
        artist: "Found Artist",
        album: null,
        file_path: "/music/found.mp3",
        audio_source_kind: "original" as const,
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        duration_ms: 100000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: null,
      },
    ];
    mockSearchLibrary.mockResolvedValue(results);

    await useLibraryStore.getState().searchSongs("found");

    expect(mockSearchLibrary).toHaveBeenCalledWith("found");
    expect(useLibraryStore.getState().songs).toEqual(results);
    expect(mockNotifyError).not.toHaveBeenCalled();
  });

  test("notifies error on search failure", async () => {
    const error = new Error("search failed");
    mockSearchLibrary.mockRejectedValue(error);

    await useLibraryStore.getState().searchSongs("bad query");

    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });
});

describe("library-store setSearchQuery debounce race", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockSearchLibrary.mockReset();
    mockNotifyError.mockReset();
    mockGetLibrary.mockReset();
    mockGetActiveLibrary.mockReset();
    mockRefreshRemoteRepository.mockReset();
    mockGetAllSeparationStatuses.mockReset();
    mockGetAllUploadStatuses.mockReset();
    mockCreateWebviewSyncChannel.mockReturnValue({
      publish: vi.fn(),
      subscribe: vi.fn().mockReturnValue(vi.fn()),
      close: vi.fn(),
    });
    useLibraryStore.setState({
      songs: [],
      searchQuery: "",
      isImporting: false,
      importErrors: [],
      selectedSongIds: new Set<string>(),
      lastClickedSongId: null,
      separationStatuses: {},
      uploadStatuses: {},
      filter: "all",
      batchSeparation: null,
      pendingImportCdgChoice: null,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  test("slow search does not overwrite fast search results (generation guard)", async () => {
    const songA = {
      hash: "song-a",
      title: "Song A",
      artist: "Artist A",
      album: null,
      file_path: "/music/a.mp3",
      audio_source_kind: "original" as const,
      cdg_path: null,
      media_g_container: null,
      instrumental: false,
      language: null,
      duration_ms: 100000,
      cover_art: null,
      has_cover_art: false,
      artwork_thumb_path: null,
      imported_at: 0,
      original_ext: null,
    };
    const songB = {
      hash: "song-b",
      title: "Song B",
      artist: "Artist B",
      album: null,
      file_path: "/music/b.mp3",
      audio_source_kind: "original" as const,
      cdg_path: null,
      media_g_container: null,
      instrumental: false,
      language: null,
      duration_ms: 200000,
      cover_art: null,
      has_cover_art: false,
      artwork_thumb_path: null,
      imported_at: 0,
      original_ext: null,
    };

    let resolveSlow: (v: unknown) => void;
    const slowPromise = new Promise((resolve) => {
      resolveSlow = resolve;
    });

    mockSearchLibrary.mockImplementation(async (query: string) => {
      if (query === "a") return [songA];
      // "slow-query" hangs until we manually resolve
      return slowPromise;
    });

    // Type "slow-query" -> debounce schedules for 300ms
    useLibraryStore.getState().setSearchQuery("slow-query");

    // Advance past debounce to fire the search
    await vi.advanceTimersByTimeAsync(300);

    // Now type "a" -> new debounce schedules for 300ms
    useLibraryStore.getState().setSearchQuery("a");

    // Advance past debounce to fire the "a" search
    await vi.advanceTimersByTimeAsync(300);

    // "a" results should be applied
    expect(useLibraryStore.getState().songs).toEqual([songA]);

    // Now resolve the slow-query search (which was still in-flight)
    resolveSlow!([songB]);
    await vi.advanceTimersByTimeAsync(0);

    // Slow results must NOT overwrite the fast "a" results
    expect(useLibraryStore.getState().songs).toEqual([songA]);
  });

  test("clearing search cancels a pending debounce before it can overwrite loadLibrary", async () => {
    const filtered = {
      hash: "see-you",
      title: "See You Again",
      artist: "Tyler, The Creator",
      album: null,
      file_path: "/music/see-you.m4a",
      audio_source_kind: "original" as const,
      cdg_path: null,
      media_g_container: null,
      instrumental: false,
      language: null,
      duration_ms: 100000,
      cover_art: null,
      has_cover_art: false,
      artwork_thumb_path: null,
      imported_at: 0,
      original_ext: null,
    };
    const fullLibrary = [
      {
        hash: "earfquake",
        title: "Earfquake",
        artist: "Tyler, The Creator",
        album: null,
        file_path: "/music/earfquake.m4a",
        audio_source_kind: "original" as const,
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        duration_ms: 100000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 1,
        original_ext: null,
      },
      filtered,
    ];

    mockSearchLibrary.mockResolvedValue([filtered]);
    mockGetLibrary.mockResolvedValue(fullLibrary);
    mockGetActiveLibrary.mockResolvedValue(null);
    mockGetAllSeparationStatuses.mockResolvedValue([]);
    mockGetAllUploadStatuses.mockResolvedValue([]);

    useLibraryStore.getState().setSearchQuery("See You");
    useLibraryStore.getState().setSearchQuery("");

    await vi.advanceTimersByTimeAsync(300);
    await Promise.resolve();

    expect(mockSearchLibrary).not.toHaveBeenCalled();
    expect(mockGetLibrary).toHaveBeenCalled();
    expect(useLibraryStore.getState().songs).toEqual(fullLibrary);
  });
});

describe("library-store resolveCdgChoicePrompt", () => {
  beforeEach(() => {
    useLibraryStore.setState({
      songs: [],
      searchQuery: "",
      isImporting: false,
      importErrors: [],
      selectedSongIds: new Set<string>(),
      lastClickedSongId: null,
      separationStatuses: {},
      uploadStatuses: {},
      filter: "all",
      batchSeparation: null,
      pendingImportCdgChoice: null,
    });
  });

  test("resolves pending promise with selected audio path", async () => {
    const request = {
      cdgPath: "/tmp/track.cdg",
      audioCandidates: ["/tmp/track.mp3", "/tmp/track.flac"],
      stem: "track",
    };

    const promise = useLibraryStore.getState().promptForCdgChoice(request);
    useLibraryStore.getState().resolveCdgChoicePrompt("/tmp/track.flac");

    const result = await promise;
    expect(result).toBe("/tmp/track.flac");
    expect(useLibraryStore.getState().pendingImportCdgChoice).toBeNull();
  });

  test("clears pendingImportCdgChoice when resolved with null", () => {
    useLibraryStore.setState({
      pendingImportCdgChoice: {
        cdgPath: "/tmp/song.cdg",
        audioCandidates: ["/tmp/song.wav"],
        stem: "song",
      },
    });

    useLibraryStore.getState().resolveCdgChoicePrompt(null);

    expect(useLibraryStore.getState().pendingImportCdgChoice).toBeNull();
  });
});

describe("library-store separation status management", () => {
  const baseStatus = {
    song_id: "song-1",
    state: "running" as const,
    percent: 30,
    cache_hit: false,
    vocals_path: null,
    accomp_path: null,
    drums_path: null,
    bass_path: null,
    other_path: null,
    model_variant: null,
    error: null,
  };

  beforeEach(() => {
    useLibraryStore.setState({
      songs: [],
      searchQuery: "",
      isImporting: false,
      importErrors: [],
      selectedSongIds: new Set<string>(),
      lastClickedSongId: null,
      separationStatuses: {},
      uploadStatuses: {},
      filter: "all",
      batchSeparation: null,
      pendingImportCdgChoice: null,
    });
  });

  test("updateSeparationStatus adds a new status entry", () => {
    useLibraryStore.getState().updateSeparationStatus(baseStatus);

    expect(useLibraryStore.getState().separationStatuses["song-1"]).toEqual(
      baseStatus,
    );
  });

  test("updateSeparationStatus overwrites an existing entry", () => {
    useLibraryStore.setState({
      separationStatuses: { "song-1": baseStatus },
    });

    const completed = {
      ...baseStatus,
      state: "completed" as const,
      percent: 100,
    };
    useLibraryStore.getState().updateSeparationStatus(completed);

    expect(useLibraryStore.getState().separationStatuses["song-1"]).toEqual(
      completed,
    );
  });

  test("clearAllSeparationStatuses resets all entries", () => {
    useLibraryStore.setState({
      separationStatuses: {
        "song-1": baseStatus,
        "song-2": { ...baseStatus, song_id: "song-2" },
      },
    });

    useLibraryStore.getState().clearAllSeparationStatuses();

    expect(useLibraryStore.getState().separationStatuses).toEqual({});
  });
});

describe("library-store batch progress", () => {
  const batchProgress = {
    total: 5,
    completed: 2,
    skipped: 0,
    failed: 1,
    current_song_id: "song-3",
    current_percent: 60,
  };

  beforeEach(() => {
    useLibraryStore.setState({
      songs: [],
      searchQuery: "",
      isImporting: false,
      importErrors: [],
      selectedSongIds: new Set<string>(),
      lastClickedSongId: null,
      separationStatuses: {},
      uploadStatuses: {},
      filter: "all",
      batchSeparation: null,
      pendingImportCdgChoice: null,
    });
  });

  test("updateBatchProgress sets the batch separation state", () => {
    useLibraryStore.getState().updateBatchProgress(batchProgress);

    expect(useLibraryStore.getState().batchSeparation).toEqual(batchProgress);
  });

  test("clearBatchSeparation resets batch separation to null", () => {
    useLibraryStore.setState({ batchSeparation: batchProgress });

    useLibraryStore.getState().clearBatchSeparation();

    expect(useLibraryStore.getState().batchSeparation).toBeNull();
  });
});

describe("library-store clearImportErrors", () => {
  test("resets importErrors to an empty array", () => {
    useLibraryStore.setState({
      importErrors: [
        {
          path: "/tmp/fail.mp3",
          error: {
            code: "media_read_failed",
            message: "bad file",
            retryable: false,
            fallback: "retry",
          },
        },
      ],
    });

    useLibraryStore.getState().clearImportErrors();

    expect(useLibraryStore.getState().importErrors).toEqual([]);
  });
});

describe("library-store clearUploadStatus edge cases", () => {
  test("is a no-op when songId is not in uploadStatuses", () => {
    useLibraryStore.setState({
      uploadStatuses: {
        "other-song": {
          song_id: "other-song",
          state: "completed" as const,
          percent: 100,
          remote_library_id: null,
          detail: null,
          error: null,
        },
      },
    });

    const before = useLibraryStore.getState().uploadStatuses;
    useLibraryStore.getState().clearUploadStatus("nonexistent");

    expect(useLibraryStore.getState().uploadStatuses).toBe(before);
  });
});

describe("library-store clearAllUploadStatuses", () => {
  test("resets uploadStatuses to an empty object", () => {
    useLibraryStore.setState({
      uploadStatuses: {
        "song-1": {
          song_id: "song-1",
          state: "running" as const,
          percent: 50,
          remote_library_id: null,
          detail: null,
          error: null,
        },
        "song-2": {
          song_id: "song-2",
          state: "completed" as const,
          percent: 100,
          remote_library_id: null,
          detail: null,
          error: null,
        },
      },
    });

    useLibraryStore.getState().clearAllUploadStatuses();

    expect(useLibraryStore.getState().uploadStatuses).toEqual({});
  });
});
