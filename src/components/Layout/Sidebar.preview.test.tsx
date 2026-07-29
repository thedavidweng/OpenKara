// @vitest-environment jsdom

import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, fireEvent, cleanup } from "@testing-library/react";
import type { Song } from "@/types/ipc";

const { mockLibraryState, mockSettingsState, mockPlaylistState } = vi.hoisted(
  () => ({
    mockLibraryState: {
      songs: [
        {
          hash: "song-1",
          file_path: "music/song-1.mp3",
          audio_source_kind: "original",
          cdg_path: null,
          media_g_container: null,
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
      separationStatuses: {} as Record<string, unknown>,
      batchSeparation: null,
    },
    mockSettingsState: {
      hideBatchSeparate: false,
      hideUpgradeAll: false,
      stemMode: "two_stem" as "two_stem" | "four_stem",
    },
    mockPlaylistState: {
      playlists: [] as Array<{
        id: string;
        name: string;
        song_count: number;
        created_at: number;
        updated_at: number;
      }>,
      activePlaylistId: null as string | null,
      isLoading: false,
      loadPlaylists: vi.fn(),
      createPlaylist: vi.fn(),
      setActivePlaylist: vi.fn(),
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

vi.mock("@/stores/playlist-store", () => ({
  usePlaylistStore: (selector: (state: typeof mockPlaylistState) => unknown) =>
    selector(mockPlaylistState),
}));

vi.mock("@/components/Library/SearchBox", () => ({
  SearchBox: () => <div data-testid="search-box">search</div>,
}));

vi.mock("@/components/Library/SongList", () => ({
  SongList: () => <div data-testid="song-list">songs</div>,
}));

vi.mock("@/components/Library/SortModeSelector", () => ({
  SortModeSelector: () => <div data-testid="sort-mode" />,
}));

vi.mock("@/components/Library/ImportButton", () => ({
  ImportButton: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock("@/components/Settings/ConfirmationDialog", () => ({
  ConfirmationDialog: ({ onConfirm }: { onConfirm: () => void }) => (
    <div data-testid="confirm-dialog">
      <button data-testid="confirm-btn" onClick={onConfirm} type="button">
        confirm
      </button>
    </div>
  ),
}));

vi.mock("@/components/Settings/InputDialog", () => ({
  InputDialog: () => <div data-testid="input-dialog" />,
}));

vi.mock("@/lib/tauri", () => ({
  batchSeparate: vi.fn(),
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

vi.mock("@/lib/song-media", () => ({
  songCanBeSeparated: () => true,
}));

import { Sidebar } from "./Sidebar";
import * as api from "@/lib/tauri";

describe("Sidebar preview mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockPlaylistState.loadPlaylists.mockClear();
  });

  afterEach(() => {
    cleanup();
  });

  it("does not call loadPlaylists in preview mode", () => {
    render(<Sidebar previewMode />);
    expect(mockPlaylistState.loadPlaylists).not.toHaveBeenCalled();
  });

  it("calls loadPlaylists in normal mode", () => {
    render(<Sidebar />);
    expect(mockPlaylistState.loadPlaylists).toHaveBeenCalled();
  });

  it("does not call batchSeparate when handleSeparateAll is triggered in preview mode", () => {
    const { container } = render(<Sidebar previewMode />);

    const separateBtn = container.querySelector("[data-separate-all-trigger]");
    if (separateBtn) {
      fireEvent.click(separateBtn);
      expect(api.batchSeparate).not.toHaveBeenCalled();
    }
  });

  it("does not call batchSeparate on upgrade confirm in preview mode", () => {
    mockSettingsState.stemMode = "four_stem";
    mockLibraryState.separationStatuses = {
      "song-1": { state: "completed", drums_path: null },
    };

    const { container } = render(<Sidebar previewMode />);

    const confirmBtn = container.querySelector('[data-testid="confirm-btn"]');
    if (confirmBtn) {
      fireEvent.click(confirmBtn);
      expect(api.batchSeparate).not.toHaveBeenCalled();
    }

    mockSettingsState.stemMode = "two_stem";
    mockLibraryState.separationStatuses = {};
  });
});
