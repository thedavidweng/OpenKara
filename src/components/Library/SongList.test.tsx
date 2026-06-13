import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { SongList } from "./SongList";
import type { Song } from "@/types/ipc";

const { mockLibraryState, mockPlaylistState } = vi.hoisted(() => ({
  mockLibraryState: {
    songs: [] as Song[],
    filter: "all" as "all" | "separated",
    separationStatuses: {} as Record<string, { state: string }>,
  },
  mockPlaylistState: {
    activePlaylistId: null as string | null,
    getPlaylistSongs: vi.fn(() => Promise.resolve([])),
    playlistSongSets: {} as Record<string, Set<string>>,
  },
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (selector: (state: typeof mockLibraryState) => unknown) =>
    selector(mockLibraryState),
}));

vi.mock("@/stores/playlist-store", () => ({
  usePlaylistStore: (selector: (state: typeof mockPlaylistState) => unknown) =>
    selector(mockPlaylistState),
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
