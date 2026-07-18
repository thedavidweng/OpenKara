// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { SongList } from "./SongList";
import type { Song } from "@/types/ipc";

type PlaylistEntry = {
  song_hash: string;
  added_at: number;
  sort_order: number;
  singer: string | null;
};

const {
  mockLibraryState,
  mockPlaylistState,
  mockSettingsState,
  mockClearRangeSelectionAnchor,
} = vi.hoisted(() => {
  const clearRangeSelectionAnchor = vi.fn();
  return {
    mockLibraryState: {
      songs: [] as Song[],
      filter: "all" as "all" | "separated",
      separationStatuses: {} as Record<string, { state: string }>,
      clearRangeSelectionAnchor,
    },
    mockPlaylistState: {
      activePlaylistId: null as string | null,
      getPlaylistSongs: vi.fn(() => Promise.resolve<PlaylistEntry[]>([])),
      playlistSongSets: {} as Record<string, Set<string>>,
    },
    mockSettingsState: {
      librarySortMode: "recently_imported" as string,
    },
    mockClearRangeSelectionAnchor: clearRangeSelectionAnchor,
  };
});

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (selector: (state: typeof mockLibraryState) => unknown) =>
    selector(mockLibraryState),
}));

vi.mock("@/stores/playlist-store", () => ({
  usePlaylistStore: (selector: (state: typeof mockPlaylistState) => unknown) =>
    selector(mockPlaylistState),
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (selector: (state: typeof mockSettingsState) => unknown) =>
    selector(mockSettingsState),
}));

vi.mock("./SongListItem", () => ({
  SongListItem: ({ song }: { song: Song }) => (
    <div data-testid="song-item">{song.title}</div>
  ),
}));

vi.mock("./EmptyLibrary", () => ({
  EmptyLibrary: () => <div data-testid="empty-library">empty</div>,
}));

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 72,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, i) => ({
        index: i,
        start: i * 72,
        size: 68,
        end: i * 72 + 68,
      })),
    scrollToIndex: vi.fn(),
  }),
}));

describe("SongList", () => {
  test("renders the empty library state when there are no songs", () => {
    mockLibraryState.songs = [];
    mockLibraryState.filter = "all";

    const markup = renderToStaticMarkup(<SongList />);

    expect(markup).toContain('data-testid="empty-library"');
    expect(markup).not.toContain('data-testid="song-item"');
  });

  test("renders song items for each song in the library", () => {
    mockLibraryState.songs = [
      {
        hash: "song-1",
        file_path: "/music/a.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        title: "Song A",
        artist: "Artist A",
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        imported_at: 0,
        original_ext: "mp3",
      },
      {
        hash: "song-2",
        file_path: "/music/b.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        title: "Song B",
        artist: "Artist B",
        album: null,
        duration_ms: 180000,
        cover_art: null,
        has_cover_art: false,
        imported_at: 0,
        original_ext: "mp3",
      },
    ];
    mockLibraryState.filter = "all";

    const markup = renderToStaticMarkup(<SongList />);

    expect(markup).toContain("Song A");
    expect(markup).toContain("Song B");
    expect(markup).not.toContain('data-testid="empty-library"');
  });

  test("filters to separated songs when the filter is set to separated", () => {
    mockLibraryState.songs = [
      {
        hash: "song-sep",
        file_path: "/music/sep.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        title: "Separated",
        artist: "Artist",
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        imported_at: 0,
        original_ext: "mp3",
      },
      {
        hash: "song-unsep",
        file_path: "/music/unsep.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        title: "Unseparated",
        artist: "Artist",
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        imported_at: 0,
        original_ext: "mp3",
      },
    ];
    mockLibraryState.filter = "separated";
    mockLibraryState.separationStatuses = {
      "song-sep": { state: "completed" },
    };

    const markup = renderToStaticMarkup(<SongList />);

    expect(markup).toContain("Separated");
    expect(markup).not.toContain("Unseparated");

    mockLibraryState.filter = "all";
    mockLibraryState.separationStatuses = {};
  });

  test("shows the empty library when filter results in no songs", () => {
    mockLibraryState.songs = [
      {
        hash: "song-1",
        file_path: "/music/a.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        title: "Not Separated",
        artist: "Artist",
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        imported_at: 0,
        original_ext: "mp3",
      },
    ];
    mockLibraryState.filter = "separated";
    mockLibraryState.separationStatuses = {};

    const markup = renderToStaticMarkup(<SongList />);

    expect(markup).toContain('data-testid="empty-library"');

    mockLibraryState.filter = "all";
  });

  test("sets the unified visual variant data attribute on the scroll container", () => {
    mockLibraryState.songs = [
      {
        hash: "song-1",
        file_path: "/music/a.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        title: "Song",
        artist: "Artist",
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        imported_at: 0,
        original_ext: "mp3",
      },
    ];
    mockLibraryState.filter = "all";

    const markup = renderToStaticMarkup(<SongList />);

    expect(markup).toContain('data-song-list-visual-variant="unified"');

    mockLibraryState.songs = [];
  });
});

// ─── sort mode effect ──────────────────────────────────────

describe("SongList sort mode effect", () => {
  afterEach(() => {
    cleanup();
    mockClearRangeSelectionAnchor.mockReset();
    mockSettingsState.librarySortMode = "recently_imported";
    mockLibraryState.songs = [];
    mockLibraryState.filter = "all";
    mockPlaylistState.activePlaylistId = null;
  });

  test("clears range selection anchor when sort mode changes", () => {
    const song: Song = {
      hash: "song-1",
      file_path: "/music/a.mp3",
      audio_source_kind: "original",
      cdg_path: null,
      media_g_container: null,
      instrumental: false,
      language: null,
      title: "Song",
      artist: "Artist",
      album: null,
      duration_ms: 120000,
      cover_art: null,
      has_cover_art: false,
      imported_at: 0,
      original_ext: "mp3",
    };
    mockLibraryState.songs = [song];
    mockSettingsState.librarySortMode = "recently_imported";

    const { rerender } = render(<SongList />);
    expect(mockClearRangeSelectionAnchor).not.toHaveBeenCalled();

    mockSettingsState.librarySortMode = "title_asc";
    rerender(<SongList />);

    expect(mockClearRangeSelectionAnchor).toHaveBeenCalledTimes(1);
  });

  test("does not clear anchor when sort mode stays the same on rerender", () => {
    const song: Song = {
      hash: "song-1",
      file_path: "/music/a.mp3",
      audio_source_kind: "original",
      cdg_path: null,
      media_g_container: null,
      instrumental: false,
      language: null,
      title: "Song",
      artist: "Artist",
      album: null,
      duration_ms: 120000,
      cover_art: null,
      has_cover_art: false,
      imported_at: 0,
      original_ext: "mp3",
    };
    mockLibraryState.songs = [song];
    mockSettingsState.librarySortMode = "title_asc";

    const { rerender } = render(<SongList />);
    rerender(<SongList />);

    expect(mockClearRangeSelectionAnchor).not.toHaveBeenCalled();
  });

  test("does not clear anchor when a playlist is active", () => {
    const song: Song = {
      hash: "song-1",
      file_path: "/music/a.mp3",
      audio_source_kind: "original",
      cdg_path: null,
      media_g_container: null,
      instrumental: false,
      language: null,
      title: "Song",
      artist: "Artist",
      album: null,
      duration_ms: 120000,
      cover_art: null,
      has_cover_art: false,
      imported_at: 0,
      original_ext: "mp3",
    };
    mockLibraryState.songs = [song];
    mockPlaylistState.activePlaylistId = "pl-1";
    mockPlaylistState.getPlaylistSongs = vi.fn(() =>
      Promise.resolve([
        { song_hash: "song-1", added_at: 0, sort_order: 0, singer: null },
      ]),
    );
    mockSettingsState.librarySortMode = "recently_imported";

    const { rerender } = render(<SongList />);

    mockSettingsState.librarySortMode = "title_asc";
    rerender(<SongList />);

    expect(mockClearRangeSelectionAnchor).not.toHaveBeenCalled();
    mockPlaylistState.activePlaylistId = null;
  });
});

// ─── playlist materialization ──────────────────────────────

describe("SongList playlist materialization", () => {
  afterEach(() => {
    cleanup();
    mockPlaylistState.activePlaylistId = null;
    mockPlaylistState.getPlaylistSongs = vi.fn(() => Promise.resolve([]));
    mockLibraryState.songs = [];
  });

  test("preserves backend sort_order instead of library order", async () => {
    const songA: Song = {
      hash: "aaa",
      file_path: "/music/a.mp3",
      audio_source_kind: "original",
      cdg_path: null,
      media_g_container: null,
      instrumental: false,
      language: null,
      title: "Alpha",
      artist: "Artist A",
      album: null,
      duration_ms: 120000,
      cover_art: null,
      has_cover_art: false,
      imported_at: 100,
      original_ext: "mp3",
    };
    const songB: Song = {
      hash: "bbb",
      file_path: "/music/b.mp3",
      audio_source_kind: "original",
      cdg_path: null,
      media_g_container: null,
      instrumental: false,
      language: null,
      title: "Beta",
      artist: "Artist B",
      album: null,
      duration_ms: 120000,
      cover_art: null,
      has_cover_art: false,
      imported_at: 200,
      original_ext: "mp3",
    };
    // Library order is [songA, songB], but playlist sort_order reverses it.
    mockLibraryState.songs = [songA, songB];
    mockPlaylistState.activePlaylistId = "pl-1";
    mockPlaylistState.getPlaylistSongs = vi.fn(() =>
      Promise.resolve([
        { song_hash: "bbb", added_at: 0, sort_order: 0, singer: null },
        { song_hash: "aaa", added_at: 0, sort_order: 1, singer: null },
      ]),
    );

    render(<SongList />);

    // Wait for the async playlist load to complete.
    const items = await screen.findAllByTestId("song-item");
    expect(items[0].textContent).toBe("Beta");
    expect(items[1].textContent).toBe("Alpha");
  });
});
