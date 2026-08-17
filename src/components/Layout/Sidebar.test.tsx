import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { BackendProvider } from "@/lib/backend";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { Sidebar } from "./Sidebar";
import type { Song } from "@/types/ipc";

const backend = createMockBackend({
  overrides: { maintenance: { batchSeparate: vi.fn() } },
});

function renderSidebar(previewMode = false) {
  return renderToStaticMarkup(
    <BackendProvider backend={backend}>
      <Sidebar previewMode={previewMode} />
    </BackendProvider>,
  );
}

const { mockLibraryState, mockSettingsState, mockPlaylistState } = vi.hoisted(
  () => ({
    mockLibraryState: {
      songs: [
        {
          hash: "song-1",
          file_path: "media-g/song-1.mp3",
          audio_source_kind: "original",
          cdg_path: "media-g/song-1.cdg",
          media_g_container: "paired" as const,
          instrumental: false,
          title: "Song",
          artist: null,
          album: null,
          duration_ms: 1000,
          cover_art: null,
          has_cover_art: false,
          artwork_thumb_path: null,
          imported_at: 0,
          original_ext: "mp3",
        },
      ] as Song[],
      filter: "all" as const,
      setFilter: vi.fn(),
      separationStatuses: {},
      batchSeparation: null,
    },
    mockSettingsState: {
      hideBatchSeparate: false,
      hideUpgradeAll: false,
      stemMode: "two_stem",
      neteaseSourceEnabled: false,
    },
    mockPlaylistState: {
      playlists: [
        {
          id: "playlist-1",
          name: "中文",
          song_count: 5,
          created_at: 0,
          updated_at: 0,
        },
      ],
      activePlaylistId: null as string | null,
      isLoading: false,
    },
  }),
);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (selector: (state: typeof mockLibraryState) => unknown) =>
    selector(mockLibraryState),
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (selector: (state: typeof mockSettingsState) => unknown) =>
    selector(mockSettingsState),
}));

vi.mock("@/stores/catalog-store", () => ({
  useCatalogStore: (
    selector: (state: {
      activeView: "library" | "netease";
      setActiveView: (view: "library" | "netease") => void;
    }) => unknown,
  ) =>
    selector({
      activeView: "library",
      setActiveView: vi.fn(),
    }),
}));

vi.mock("@/stores/playlist-store", () => ({
  usePlaylistStore: (selector: (state: typeof mockPlaylistState) => unknown) =>
    selector(mockPlaylistState),
}));

vi.mock("@/components/Library/SearchBox", () => ({
  SearchBox: () => <div data-search-visual-variant="mock">search</div>,
}));

vi.mock("@/components/Library/SongList", () => ({
  SongList: () => <div data-song-list-visual-variant="mock">songs</div>,
}));

vi.mock("@/components/Library/ImportButton", () => ({
  ImportButton: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock("@/components/Settings/ConfirmationDialog", () => ({
  ConfirmationDialog: () => <div>confirm</div>,
}));

vi.mock("@/lib/errors", () => ({
  notifyError: vi.fn(),
}));

vi.mock("@/components/Overlay/Tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock("@/lib/app-shortcuts", () => ({
  APP_SHORTCUTS: {},
  getShortcutDisplay: () => "",
}));

describe("Sidebar", () => {
  test("does not render a duplicate import icon beside the local music heading", () => {
    const markup = renderSidebar();

    expect(markup).not.toContain("lucide-cloud-upload");
  });

  test("hides separate-all controls when the library has only media-g songs", () => {
    const markup = renderSidebar();

    expect(markup).not.toContain("sidebar.separateAll");
    expect(markup).not.toContain("sidebar.upgradeAll");
  });

  test("hides separate-all controls when every plain-audio song is instrumental", () => {
    mockLibraryState.songs = [
      {
        hash: "song-2",
        file_path: "music/song-2.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: true,
        title: "Instrumental",
        artist: null,
        album: null,
        duration_ms: 1000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ] as Song[];

    const markup = renderSidebar();

    expect(markup).not.toContain("sidebar.separateAll");
    expect(markup).not.toContain("sidebar.upgradeAll");
  });

  test("uses the unified sidebar surface and composition markers", () => {
    const markup = renderSidebar();

    expect(markup).toContain('data-sidebar-visual-variant="unified"');
    expect(markup).toContain('data-search-visual-variant="mock"');
    expect(markup).toContain('data-song-list-visual-variant="mock"');
  });

  test("hides the library section while a playlist is active", () => {
    mockPlaylistState.activePlaylistId = "playlist-1";

    const markup = renderSidebar();

    expect(markup).not.toContain("sidebar.library");
    expect(markup).not.toContain("sidebar.allTracks");
    expect(markup).not.toContain("sidebar.separated");
    expect(markup).toContain("中文");
    expect(markup).toContain("song-list-visual-variant");

    mockPlaylistState.activePlaylistId = null;
  });

  test("renders playlist counts from store state", () => {
    const markup = renderSidebar();

    expect(markup).toContain(">5<");
  });

  test("keeps the full app rail visible while marking playlist switches for preview", () => {
    const markup = renderSidebar(true);

    expect(markup).toContain('data-preview-playlist-switch="true"');
    expect(markup).toContain('data-search-visual-variant="mock"');
    expect(markup).toContain("sidebar.library");
    expect(markup).toContain("sidebar.allTracks");
    expect(markup).not.toContain("sidebar.separateAll");
  });

  test("renders batch actions with shared sidebar control tokens", () => {
    mockLibraryState.songs = [
      {
        hash: "song-3",
        file_path: "music/song-3.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        title: "Song 3",
        artist: null,
        album: null,
        duration_ms: 1000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ] as Song[];

    const markup = renderSidebar();

    expect(markup).toContain("bg-[var(--sidebar-control-bg)]");
    expect(markup).toContain("border-[var(--sidebar-control-border)]");
    expect(markup).toContain("hover:bg-[var(--sidebar-row-overlay-bg)]");
    expect(markup).toContain("hover:border-[var(--sidebar-control-border)]");
    expect(markup).not.toContain(
      "hover:border-[var(--sidebar-row-selected-border)]",
    );

    mockLibraryState.songs = [
      {
        hash: "song-1",
        file_path: "media-g/song-1.mp3",
        audio_source_kind: "original",
        cdg_path: "media-g/song-1.cdg",
        media_g_container: "paired" as const,
        instrumental: false,
        title: "Song",
        artist: null,
        album: null,
        duration_ms: 1000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ] as Song[];
  });

  test("hides separate-all when every separable song is completed in two-stem mode", () => {
    mockLibraryState.songs = [
      {
        hash: "song-3",
        file_path: "music/song-3.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        title: "Song 3",
        artist: null,
        album: null,
        duration_ms: 1000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ] as Song[];
    mockLibraryState.separationStatuses = {
      "song-3": {
        song_id: "song-3",
        state: "completed",
        percent: 100,
        cache_hit: false,
        vocals_path: "music/song-3_vocals.wav",
        accomp_path: "music/song-3_accomp.wav",
        drums_path: null,
        bass_path: null,
        other_path: null,
        model_variant: null,
        error: null,
      },
    };

    const markup = renderSidebar();

    expect(markup).not.toContain("sidebar.separateAll");

    mockLibraryState.songs = [
      {
        hash: "song-1",
        file_path: "media-g/song-1.mp3",
        audio_source_kind: "original",
        cdg_path: "media-g/song-1.cdg",
        media_g_container: "paired" as const,
        instrumental: false,
        title: "Song",
        artist: null,
        album: null,
        duration_ms: 1000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ] as Song[];
    mockLibraryState.separationStatuses = {};
  });
});
