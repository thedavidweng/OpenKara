import { beforeEach, describe, expect, test, vi } from "vitest";

const {
  mockListPlaylists,
  mockCreatePlaylist,
  mockRenamePlaylist,
  mockDeletePlaylist,
  mockAddSongsToPlaylist,
  mockRemoveSongsFromPlaylist,
  mockGetPlaylistSongs,
} = vi.hoisted(() => ({
  mockListPlaylists: vi.fn(),
  mockCreatePlaylist: vi.fn(),
  mockRenamePlaylist: vi.fn(),
  mockDeletePlaylist: vi.fn(),
  mockAddSongsToPlaylist: vi.fn(),
  mockRemoveSongsFromPlaylist: vi.fn(),
  mockGetPlaylistSongs: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  listPlaylists: mockListPlaylists,
  createPlaylist: mockCreatePlaylist,
  renamePlaylist: mockRenamePlaylist,
  deletePlaylist: mockDeletePlaylist,
  addSongsToPlaylist: mockAddSongsToPlaylist,
  removeSongsFromPlaylist: mockRemoveSongsFromPlaylist,
  getPlaylistSongs: mockGetPlaylistSongs,
}));

import { usePlaylistStore } from "./playlist-store";

const mockPlaylists = [
  {
    id: "pl-1",
    name: "Favourites",
    song_count: 2,
    created_at: 1000,
    updated_at: 2000,
  },
  {
    id: "pl-2",
    name: "Party",
    song_count: 0,
    created_at: 3000,
    updated_at: 3000,
  },
];

const mockSongs = [
  { song_hash: "s1", added_at: 1000, sort_order: 0, singer: null },
  { song_hash: "s2", added_at: 1000, sort_order: 1, singer: "Alice" },
];

describe("playlist-store", () => {
  beforeEach(() => {
    mockListPlaylists.mockReset();
    mockCreatePlaylist.mockReset();
    mockRenamePlaylist.mockReset();
    mockDeletePlaylist.mockReset();
    mockAddSongsToPlaylist.mockReset();
    mockRemoveSongsFromPlaylist.mockReset();
    mockGetPlaylistSongs.mockReset();

    usePlaylistStore.setState({
      playlists: [],
      activePlaylistId: null,
      isLoading: false,
      playlistSongSets: new Map(),
    });
  });

  describe("loadPlaylists", () => {
    test("fetches playlists and sets state", async () => {
      mockListPlaylists.mockResolvedValue(mockPlaylists);
      mockGetPlaylistSongs.mockResolvedValue([]);

      await usePlaylistStore.getState().loadPlaylists();

      const state = usePlaylistStore.getState();
      expect(state.playlists).toEqual(mockPlaylists);
      expect(state.isLoading).toBe(false);
    });

    test("sets isLoading during fetch", async () => {
      let resolveList: (v: unknown) => void;
      mockListPlaylists.mockReturnValue(new Promise((r) => (resolveList = r)));
      mockGetPlaylistSongs.mockResolvedValue([]);

      const promise = usePlaylistStore.getState().loadPlaylists();
      expect(usePlaylistStore.getState().isLoading).toBe(true);

      resolveList!(mockPlaylists);
      await promise;
      expect(usePlaylistStore.getState().isLoading).toBe(false);
    });

    test("sets isLoading false on error", async () => {
      mockListPlaylists.mockRejectedValue(new Error("fail"));

      await usePlaylistStore.getState().loadPlaylists();

      expect(usePlaylistStore.getState().isLoading).toBe(false);
      expect(usePlaylistStore.getState().playlists).toEqual([]);
    });

    test("populates playlistSongSets after loading", async () => {
      mockListPlaylists.mockResolvedValue(mockPlaylists);
      mockGetPlaylistSongs
        .mockResolvedValueOnce([mockSongs[0]])
        .mockResolvedValueOnce([]);

      await usePlaylistStore.getState().loadPlaylists();

      const sets = usePlaylistStore.getState().playlistSongSets;
      expect(sets.get("pl-1")).toEqual(new Set(["s1"]));
      expect(sets.get("pl-2")).toEqual(new Set());
    });
  });

  describe("createPlaylist", () => {
    test("calls API and reloads playlists", async () => {
      const newPlaylist = {
        id: "pl-new",
        name: "New",
        song_count: 0,
        created_at: 1,
        updated_at: 1,
      };
      mockCreatePlaylist.mockResolvedValue(newPlaylist);
      mockListPlaylists.mockResolvedValue([...mockPlaylists, newPlaylist]);
      mockGetPlaylistSongs.mockResolvedValue([]);

      const result = await usePlaylistStore.getState().createPlaylist("New");

      expect(mockCreatePlaylist).toHaveBeenCalledWith("New");
      expect(result).toEqual(newPlaylist);
      expect(usePlaylistStore.getState().playlists).toHaveLength(3);
    });
  });

  describe("renamePlaylist", () => {
    test("calls API and updates local state optimistically", async () => {
      mockRenamePlaylist.mockResolvedValue(undefined);
      usePlaylistStore.setState({ playlists: [...mockPlaylists] });

      await usePlaylistStore.getState().renamePlaylist("pl-1", "Renamed");

      expect(mockRenamePlaylist).toHaveBeenCalledWith("pl-1", "Renamed");
      const updated = usePlaylistStore
        .getState()
        .playlists.find((p) => p.id === "pl-1");
      expect(updated?.name).toBe("Renamed");
      const other = usePlaylistStore
        .getState()
        .playlists.find((p) => p.id === "pl-2");
      expect(other?.name).toBe("Party");
    });
  });

  describe("deletePlaylist", () => {
    test("calls API, clears activePlaylistId if it was the deleted one, and reloads", async () => {
      mockDeletePlaylist.mockResolvedValue(undefined);
      mockListPlaylists.mockResolvedValue([mockPlaylists[1]]);
      mockGetPlaylistSongs.mockResolvedValue([]);

      usePlaylistStore.setState({
        playlists: [...mockPlaylists],
        activePlaylistId: "pl-1",
      });

      await usePlaylistStore.getState().deletePlaylist("pl-1");

      expect(mockDeletePlaylist).toHaveBeenCalledWith("pl-1");
      expect(usePlaylistStore.getState().activePlaylistId).toBeNull();
    });

    test("keeps activePlaylistId if it was a different playlist", async () => {
      mockDeletePlaylist.mockResolvedValue(undefined);
      mockListPlaylists.mockResolvedValue([mockPlaylists[0]]);
      mockGetPlaylistSongs.mockResolvedValue([]);

      usePlaylistStore.setState({
        playlists: [...mockPlaylists],
        activePlaylistId: "pl-2",
      });

      await usePlaylistStore.getState().deletePlaylist("pl-1");

      expect(usePlaylistStore.getState().activePlaylistId).toBe("pl-2");
    });
  });

  describe("addSongsToPlaylist", () => {
    test("calls API and reloads playlists", async () => {
      mockAddSongsToPlaylist.mockResolvedValue(undefined);
      mockListPlaylists.mockResolvedValue(mockPlaylists);
      mockGetPlaylistSongs.mockResolvedValue([]);

      await usePlaylistStore
        .getState()
        .addSongsToPlaylist("pl-1", ["s3", "s4"]);

      expect(mockAddSongsToPlaylist).toHaveBeenCalledWith("pl-1", ["s3", "s4"]);
      expect(mockListPlaylists).toHaveBeenCalled();
    });
  });

  describe("removeSongsFromPlaylist", () => {
    test("calls API and reloads playlists", async () => {
      mockRemoveSongsFromPlaylist.mockResolvedValue(undefined);
      mockListPlaylists.mockResolvedValue(mockPlaylists);
      mockGetPlaylistSongs.mockResolvedValue([]);

      await usePlaylistStore.getState().removeSongsFromPlaylist("pl-1", ["s1"]);

      expect(mockRemoveSongsFromPlaylist).toHaveBeenCalledWith("pl-1", ["s1"]);
      expect(mockListPlaylists).toHaveBeenCalled();
    });
  });

  describe("getPlaylistSongs", () => {
    test("returns songs from API", async () => {
      mockGetPlaylistSongs.mockResolvedValue(mockSongs);

      const result = await usePlaylistStore.getState().getPlaylistSongs("pl-1");

      expect(mockGetPlaylistSongs).toHaveBeenCalledWith("pl-1");
      expect(result).toEqual(mockSongs);
    });
  });

  describe("setActivePlaylist", () => {
    test("sets activePlaylistId", () => {
      usePlaylistStore.getState().setActivePlaylist("pl-1");
      expect(usePlaylistStore.getState().activePlaylistId).toBe("pl-1");
    });

    test("clears activePlaylistId with null", () => {
      usePlaylistStore.setState({ activePlaylistId: "pl-1" });
      usePlaylistStore.getState().setActivePlaylist(null);
      expect(usePlaylistStore.getState().activePlaylistId).toBeNull();
    });
  });

  describe("loadPlaylistSongSets", () => {
    test("builds set of song hashes per playlist", async () => {
      usePlaylistStore.setState({ playlists: mockPlaylists });
      mockGetPlaylistSongs
        .mockResolvedValueOnce([mockSongs[0], mockSongs[1]])
        .mockResolvedValueOnce([]);

      await usePlaylistStore.getState().loadPlaylistSongSets();

      const sets = usePlaylistStore.getState().playlistSongSets;
      expect(sets.get("pl-1")).toEqual(new Set(["s1", "s2"]));
      expect(sets.get("pl-2")).toEqual(new Set());
    });

    test("handles empty playlists list", async () => {
      usePlaylistStore.setState({ playlists: [] });

      await usePlaylistStore.getState().loadPlaylistSongSets();

      expect(usePlaylistStore.getState().playlistSongSets.size).toBe(0);
      expect(mockGetPlaylistSongs).not.toHaveBeenCalled();
    });
  });
});
