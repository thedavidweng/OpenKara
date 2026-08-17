// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { BackendProvider } from "@/lib/backend";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { Sidebar } from "./Sidebar";

const setFilter = vi.fn();
const setActivePlaylist = vi.fn();
const setCatalogView = vi.fn();

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (
    selector: (state: {
      songs: never[];
      filter: "all";
      setFilter: typeof setFilter;
      separationStatuses: Record<string, never>;
      batchSeparation: null;
    }) => unknown,
  ) =>
    selector({
      songs: [],
      filter: "all",
      setFilter,
      separationStatuses: {},
      batchSeparation: null,
    }),
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (
    selector: (state: {
      hideBatchSeparate: boolean;
      hideUpgradeAll: boolean;
      stemMode: string;
      neteaseSourceEnabled: boolean;
    }) => unknown,
  ) =>
    selector({
      hideBatchSeparate: true,
      hideUpgradeAll: true,
      stemMode: "two_stem",
      neteaseSourceEnabled: true,
    }),
}));

vi.mock("@/stores/catalog-store", () => ({
  useCatalogStore: (
    selector: (state: {
      activeView: "library" | "netease";
      setActiveView: typeof setCatalogView;
    }) => unknown,
  ) => selector({ activeView: "library", setActiveView: setCatalogView }),
}));

vi.mock("@/stores/playlist-store", () => ({
  usePlaylistStore: (
    selector: (state: {
      playlists: never[];
      activePlaylistId: null;
      isLoading: boolean;
      loadPlaylists: () => void;
      createPlaylist: () => void;
      setActivePlaylist: typeof setActivePlaylist;
    }) => unknown,
  ) =>
    selector({
      playlists: [],
      activePlaylistId: null,
      isLoading: false,
      loadPlaylists: () => {},
      createPlaylist: () => {},
      setActivePlaylist,
    }),
}));

vi.mock("@/components/Library/SearchBox", () => ({
  SearchBox: () => null,
}));
vi.mock("@/components/Library/SongList", () => ({
  SongList: () => null,
}));
vi.mock("@/components/Library/ImportButton", () => ({
  ImportButton: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock("@/components/Library/SortModeSelector", () => ({
  SortModeSelector: () => null,
}));
vi.mock("@/components/Catalog/NeteasePanel", () => ({
  NeteasePanel: () => <div>netease-panel</div>,
}));
vi.mock("@/components/Settings/ConfirmationDialog", () => ({
  ConfirmationDialog: () => null,
}));
vi.mock("@/lib/errors", () => ({ notifyError: vi.fn() }));

describe("Sidebar catalog source rows", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    setFilter.mockReset();
    setActivePlaylist.mockReset();
    setCatalogView.mockReset();
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("All Tracks, Separated, and NetEase switch the catalog view", async () => {
    const backend = createMockBackend();
    await act(async () => {
      root.render(
        <BackendProvider backend={backend}>
          <Sidebar />
        </BackendProvider>,
      );
    });
    (
      container.querySelector(
        "[aria-label='sidebar.allTracks']",
      ) as HTMLButtonElement
    ).click();
    (
      container.querySelector(
        "[aria-label='sidebar.separated']",
      ) as HTMLButtonElement
    ).click();
    (
      container.querySelector(
        "[aria-label='sidebar.netease']",
      ) as HTMLButtonElement
    ).click();
    expect(setCatalogView).toHaveBeenCalledWith("library");
    expect(setCatalogView).toHaveBeenCalledWith("netease");
    expect(setActivePlaylist).toHaveBeenCalledWith(null);
  });
});
