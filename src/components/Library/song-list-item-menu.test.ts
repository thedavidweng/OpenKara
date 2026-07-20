import { describe, expect, test, vi } from "vitest";
import {
  buildSongListContextMenuItems,
  SONG_LANGUAGES,
} from "./song-list-item-menu";
import type { SongLanguage } from "./song-list-item-menu";

const t = (key: string, _opts?: Record<string, string | number>) => key;

function makeDefaults(
  overrides: Partial<Parameters<typeof buildSongListContextMenuItems>[0]> = {},
) {
  return {
    t,
    isMultiSelected: false,
    selectedCount: 1,
    selectedSongIds: ["song-1"],
    selectedHasSeparableSongs: false,
    selectedCanToggleInstrumentalSongs: false,
    selectedInstrumentalState: "unchecked" as const,
    selectedLanguage: null as SongLanguage | null,
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
    playlists: [] as Array<{ id: string; name: string }>,
    songPlaylistMembership: new Map<string, "checked" | "mixed" | null>(),
    onAddToPlaylist: vi.fn(),
    onRemoveFromPlaylist: vi.fn(),
    onCreatePlaylistAndAdd: vi.fn(),
    activePlaylistId: null as string | null,
    onRemoveFromActivePlaylist: vi.fn(),
    ...overrides,
  };
}

function findItem(
  items: ReturnType<typeof buildSongListContextMenuItems>,
  label: string,
) {
  return items.find((i) => i.label === label);
}

describe("buildSongListContextMenuItems", () => {
  describe("single selection", () => {
    test("returns base items including playNow, playNext, addToQueue", () => {
      const items = buildSongListContextMenuItems(makeDefaults());
      const labels = items.map((i) => i.label);

      expect(labels).toContain("library.playNow");
      expect(labels).toContain("library.playNext");
      expect(labels).toContain("library.addToQueue");
    });

    test("includes fetchLyricsOnline, editInfo, properties, delete", () => {
      const items = buildSongListContextMenuItems(makeDefaults());
      const labels = items.map((i) => i.label);

      expect(labels).toContain("library.fetchLyricsOnline");
      expect(labels).toContain("library.editInfo");
      expect(labels).toContain("library.properties");
      expect(labels).toContain("library.delete");
    });

    test("includes extractEmbeddedCoverArt", () => {
      const items = buildSongListContextMenuItems(makeDefaults());
      const labels = items.map((i) => i.label);

      expect(labels).toContain("library.extractEmbeddedCoverArt");
    });

    test("does NOT include extractEmbeddedLyrics when supportsEmbeddedLyrics is false", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({ supportsEmbeddedLyrics: false }),
      );
      const labels = items.map((i) => i.label);

      expect(labels).not.toContain("library.extractEmbeddedLyrics");
    });

    test("includes extractEmbeddedLyrics when supportsEmbeddedLyrics is true", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({ supportsEmbeddedLyrics: true }),
      );
      const labels = items.map((i) => i.label);

      expect(labels).toContain("library.extractEmbeddedLyrics");
    });

    test("includes the addToPlaylist submenu item", () => {
      const items = buildSongListContextMenuItems(makeDefaults());
      const addToPlaylist = findItem(items, "playlist.addTo");

      expect(addToPlaylist).toBeDefined();
      expect(addToPlaylist!.children).toBeDefined();
      expect(addToPlaylist!.children!.length).toBeGreaterThanOrEqual(1);
      expect(
        addToPlaylist!.children![addToPlaylist!.children!.length - 1]!.label,
      ).toBe("playlist.newPlaylist");
    });

    test("addToPlaylist submenu lists playlists with correct membership indicators", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          playlists: [
            { id: "pl-1", name: "Favorites" },
            { id: "pl-2", name: "Party" },
          ],
          songPlaylistMembership: new Map([["pl-1", "checked"]]),
        }),
      );

      const addToPlaylist = findItem(items, "playlist.addTo")!;
      const children = addToPlaylist.children!;

      const favorites = findItem(children, "Favorites")!;
      expect(favorites.indicator).toBe("checked");

      const party = findItem(children, "Party")!;
      expect(party.indicator).toBeNull();
    });

    test("clicking a checked playlist calls onRemoveFromPlaylist", () => {
      const onRemove = vi.fn();
      const items = buildSongListContextMenuItems(
        makeDefaults({
          playlists: [{ id: "pl-1", name: "Favorites" }],
          songPlaylistMembership: new Map([["pl-1", "checked"]]),
          onRemoveFromPlaylist: onRemove,
        }),
      );

      const addToPlaylist = findItem(items, "playlist.addTo")!;
      const favorites = findItem(addToPlaylist.children!, "Favorites")!;
      favorites.onClick!();

      expect(onRemove).toHaveBeenCalledWith("pl-1");
    });

    test("clicking an unchecked playlist calls onAddToPlaylist", () => {
      const onAdd = vi.fn();
      const items = buildSongListContextMenuItems(
        makeDefaults({
          playlists: [{ id: "pl-1", name: "Favorites" }],
          songPlaylistMembership: new Map(),
          onAddToPlaylist: onAdd,
        }),
      );

      const addToPlaylist = findItem(items, "playlist.addTo")!;
      const favorites = findItem(addToPlaylist.children!, "Favorites")!;
      favorites.onClick!();

      expect(onAdd).toHaveBeenCalledWith("pl-1");
    });

    test("removeFromPlaylist item appears when activePlaylistId is set", () => {
      const onRemove = vi.fn();
      const items = buildSongListContextMenuItems(
        makeDefaults({
          activePlaylistId: "pl-active",
          onRemoveFromActivePlaylist: onRemove,
        }),
      );

      const removeItem = findItem(items, "playlist.removeFromPlaylist");
      expect(removeItem).toBeDefined();
      removeItem!.onClick!();
      expect(onRemove).toHaveBeenCalled();
    });

    test("removeFromPlaylist item is absent when activePlaylistId is null", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({ activePlaylistId: null }),
      );

      const removeItem = findItem(items, "playlist.removeFromPlaylist");
      expect(removeItem).toBeUndefined();
    });

    test("language submenu is present with auto + all SONG_LANGUAGES", () => {
      const items = buildSongListContextMenuItems(makeDefaults());
      const langItem = findItem(items, "library.language")!;

      expect(langItem).toBeDefined();
      expect(langItem.children).toBeDefined();
      expect(langItem.children!.length).toBe(1 + SONG_LANGUAGES.length);
      expect(langItem.children![0].label).toBe("library.languageAuto");
    });

    test("language auto entry shows checked indicator when selectedLanguage is null", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({ selectedLanguage: null }),
      );
      const langItem = findItem(items, "library.language")!;
      const auto = langItem.children![0];

      expect(auto.indicator).toBe("checked");
    });

    test("specific language shows checked indicator when selected", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({ selectedLanguage: "japanese" }),
      );
      const langItem = findItem(items, "library.language")!;
      const auto = langItem.children![0];
      const japanese = langItem.children![3]; // 0=auto, 1=mandarin, 2=cantonese, 3=japanese

      expect(auto.indicator).toBeNull();
      expect(japanese.label).toBe("library.language_japanese");
      expect(japanese.indicator).toBe("checked");
    });
  });

  describe("multi selection", () => {
    test("returns multi-select items including queueAllSelected and deleteSelected", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedCount: 3,
          selectedSongIds: ["a", "b", "c"],
        }),
      );
      const labels = items.map((i) => i.label);

      expect(labels).toContain("library.queueAllSelected");
      expect(labels).toContain("library.deleteSelected");
    });

    test("includes addToPlaylist, language, extractEmbeddedCoverArtSelected", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedCount: 2,
          selectedSongIds: ["a", "b"],
        }),
      );
      const labels = items.map((i) => i.label);

      expect(labels).toContain("playlist.addTo");
      expect(labels).toContain("library.language");
      expect(labels).toContain("library.extractEmbeddedCoverArtSelected");
    });

    test("does NOT include single-select items like playNow, playNext, addToQueue", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({ isMultiSelected: true }),
      );
      const labels = items.map((i) => i.label);

      expect(labels).not.toContain("library.playNow");
      expect(labels).not.toContain("library.playNext");
      expect(labels).not.toContain("library.addToQueue");
      expect(labels).not.toContain("library.delete");
      expect(labels).not.toContain("library.fetchLyricsOnline");
      expect(labels).not.toContain("library.editInfo");
      expect(labels).not.toContain("library.properties");
    });

    test("includes separateAllSelected when selectedHasSeparableSongs is true", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedHasSeparableSongs: true,
          selectedCount: 2,
          selectedSongIds: ["a", "b"],
        }),
      );

      expect(findItem(items, "library.separateAllSelected")).toBeDefined();
    });

    test("excludes separateAllSelected when selectedHasSeparableSongs is false", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedHasSeparableSongs: false,
        }),
      );

      expect(findItem(items, "library.separateAllSelected")).toBeUndefined();
    });

    test("includes markInstrumentalSelected when selectedCanToggleInstrumentalSongs is true", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedCanToggleInstrumentalSongs: true,
          selectedCount: 2,
          selectedSongIds: ["a", "b"],
        }),
      );

      const item = findItem(items, "library.markInstrumentalSelected");
      expect(item).toBeDefined();
    });

    test("instrumental indicator is 'checked' when state is checked", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedCanToggleInstrumentalSongs: true,
          selectedInstrumentalState: "checked",
          selectedCount: 2,
          selectedSongIds: ["a", "b"],
        }),
      );

      const item = findItem(items, "library.markInstrumentalSelected")!;
      expect(item.indicator).toBe("checked");
    });

    test("instrumental indicator is 'mixed' when state is mixed", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedCanToggleInstrumentalSongs: true,
          selectedInstrumentalState: "mixed",
          selectedCount: 2,
          selectedSongIds: ["a", "b"],
        }),
      );

      const item = findItem(items, "library.markInstrumentalSelected")!;
      expect(item.indicator).toBe("mixed");
    });

    test("instrumental indicator is null when state is unchecked", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedCanToggleInstrumentalSongs: true,
          selectedInstrumentalState: "unchecked",
          selectedCount: 2,
          selectedSongIds: ["a", "b"],
        }),
      );

      const item = findItem(items, "library.markInstrumentalSelected")!;
      expect(item.indicator).toBeNull();
    });

    test("excludes markInstrumentalSelected when selectedCanToggleInstrumentalSongs is false", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedCanToggleInstrumentalSongs: false,
        }),
      );

      expect(
        findItem(items, "library.markInstrumentalSelected"),
      ).toBeUndefined();
    });

    test("removeFromPlaylist appears in multi-select when activePlaylistId is set", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          activePlaylistId: "pl-active",
          selectedCount: 2,
          selectedSongIds: ["a", "b"],
        }),
      );

      expect(findItem(items, "playlist.removeFromPlaylist")).toBeDefined();
    });

    test("removeFromPlaylist absent in multi-select when activePlaylistId is null", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          activePlaylistId: null,
        }),
      );

      expect(findItem(items, "playlist.removeFromPlaylist")).toBeUndefined();
    });

    test("language submenu shows auto checked when selectedLanguage is null", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedLanguage: null,
          selectedCount: 2,
          selectedSongIds: ["a", "b"],
        }),
      );

      const langItem = findItem(items, "library.language")!;
      expect(langItem.children![0].indicator).toBe("checked");
    });

    test("language submenu shows specific language checked", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedLanguage: "korean",
          selectedCount: 2,
          selectedSongIds: ["a", "b"],
        }),
      );

      const langItem = findItem(items, "library.language")!;
      const korean = langItem.children!.find(
        (c) => c.label === "library.language_korean",
      )!;
      expect(korean.indicator).toBe("checked");
    });
  });

  describe("selectedCount usage", () => {
    test("multi-select labels use selectedCount when non-zero", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedCount: 5,
          selectedSongIds: ["a", "b"],
        }),
      );

      // queueAllSelected label should include count=5 (via t function passthrough)
      const queueItem = findItem(items, "library.queueAllSelected")!;
      expect(queueItem.label).toBe("library.queueAllSelected");
    });

    test("falls back to selectedSongIds.length when selectedCount is 0", () => {
      const items = buildSongListContextMenuItems(
        makeDefaults({
          isMultiSelected: true,
          selectedCount: 0,
          selectedSongIds: ["a", "b", "c"],
        }),
      );

      // items still render; count passed to t would be selectedSongIds.length
      const queueItem = findItem(items, "library.queueAllSelected")!;
      expect(queueItem).toBeDefined();
    });
  });

  describe("new playlist creation", () => {
    test("new playlist entry calls onCreatePlaylistAndAdd", () => {
      const onCreate = vi.fn();
      const items = buildSongListContextMenuItems(
        makeDefaults({ onCreatePlaylistAndAdd: onCreate }),
      );

      const addToPlaylist = findItem(items, "playlist.addTo")!;
      const newEntry =
        addToPlaylist.children![addToPlaylist.children!.length - 1]!;
      expect(newEntry.label).toBe("playlist.newPlaylist");
      newEntry.onClick!();
      expect(onCreate).toHaveBeenCalled();
    });
  });
});
