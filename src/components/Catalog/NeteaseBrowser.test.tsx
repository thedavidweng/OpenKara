// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  test,
  vi,
} from "vitest";
import type { StreamingTrack } from "@/types/ipc";
import { useCatalogStore } from "@/stores/catalog-store";
import { NeteaseBrowser } from "./NeteaseBrowser";

vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));

// jsdom has no layout; give scroll containers a viewport so rows render.
const heightDescriptor = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  "offsetHeight",
);
beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get: () => 600,
  });
});
afterAll(() => {
  if (heightDescriptor) {
    Object.defineProperty(
      HTMLElement.prototype,
      "offsetHeight",
      heightDescriptor,
    );
  }
});

const track = (id: string, overrides: Partial<StreamingTrack> = {}) =>
  ({
    source_id: "netease",
    remote_track_id: id,
    title: `Song ${id}`,
    artist: "Artist",
    album: "Album",
    duration_ms: 60000,
    refusal: null,
    library_song_id: null,
    cover_url: null,
    ...overrides,
  }) satisfies StreamingTrack;

const loadLiked = vi.fn();
const loadPlaylists = vi.fn();
const search = vi.fn();
const openPlaylist = vi.fn();
const importTracks = vi.fn();

const signedIn = {
  source_id: "netease" as const,
  signed_in: true,
  display_name: "Davy",
  expired: false,
};

beforeEach(() => {
  for (const fn of [loadLiked, loadPlaylists, search, openPlaylist]) {
    fn.mockReset();
    fn.mockResolvedValue(undefined);
  }
  importTracks.mockReset();
  useCatalogStore.setState({
    session: signedIn,
    browserTab: "playlists",
    liked: [],
    likedStatus: "idle",
    playlists: [],
    playlistsStatus: "idle",
    searchResults: [],
    searchStatus: "idle",
    searchQuery: "",
    selectedPlaylistId: null,
    playlistDetail: null,
    playlistDetailStatus: "idle",
    trackStates: {},
    hideUnavailable: true,
    importRun: null,
    loadLiked,
    loadPlaylists,
    search,
    openPlaylist,
    importTracks,
  });
});

afterEach(cleanup);

describe("NeteaseBrowser", () => {
  test("shows the sign-in card when signed out", () => {
    useCatalogStore.setState({ session: null });
    render(<NeteaseBrowser />);
    expect(screen.getByTestId("netease-signin")).toBeTruthy();
    expect(screen.queryByRole("tablist")).toBeNull();
  });

  test("lazily loads only the active tab", () => {
    render(<NeteaseBrowser />);
    expect(loadPlaylists).toHaveBeenCalledTimes(1);
    expect(loadLiked).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("tab", { name: "catalog.netease.liked" }));
    expect(loadLiked).toHaveBeenCalledTimes(1);
  });

  test("tabs use roving tabindex and arrow, Home and End keys", () => {
    useCatalogStore.setState({
      searchQuery: "abba",
      searchStatus: "ready",
      likedStatus: "ready",
      playlistsStatus: "ready",
    });
    render(<NeteaseBrowser />);
    const tabs = screen.getAllByRole("tab");
    expect(tabs).toHaveLength(3);
    expect(tabs[1].getAttribute("aria-selected")).toBe("true");
    expect(tabs[1].tabIndex).toBe(0);
    expect(tabs[0].tabIndex).toBe(-1);

    fireEvent.keyDown(tabs[1], { key: "ArrowRight" });
    expect(useCatalogStore.getState().browserTab).toBe("results");
    fireEvent.keyDown(tabs[1], { key: "ArrowLeft" });
    expect(useCatalogStore.getState().browserTab).toBe("liked");
    fireEvent.keyDown(tabs[1], { key: "End" });
    expect(useCatalogStore.getState().browserTab).toBe("results");
    fireEvent.keyDown(tabs[1], { key: "Home" });
    expect(useCatalogStore.getState().browserTab).toBe("liked");
  });

  test("has no title block and no counts in the tabs", () => {
    useCatalogStore.setState({
      likedStatus: "ready",
      liked: [track("1")],
      playlistsStatus: "ready",
      playlists: [
        { remote_id: "a", name: "A", track_count: 1, cover_url: null },
      ],
    });
    render(<NeteaseBrowser />);
    expect(screen.queryByRole("heading", { level: 1 })).toBeNull();
    const labels = screen.getAllByRole("tab").map((tab) => tab.textContent);
    expect(labels).toEqual([
      "catalog.netease.liked",
      "catalog.netease.playlists",
    ]);
  });

  test("renders the results tab only while a search is active", () => {
    render(<NeteaseBrowser />);
    expect(screen.getAllByRole("tab")).toHaveLength(2);
    expect(screen.getByRole("tabpanel")).toBeTruthy();
  });

  test("submits a search and focuses the field with the find shortcut", () => {
    render(<NeteaseBrowser />);
    const input = screen.getByRole("textbox", {
      name: "catalog.netease.searchPlaceholder",
    });
    fireEvent.keyDown(window, { code: "KeyF", key: "f", ctrlKey: true });
    expect(document.activeElement).toBe(input);

    fireEvent.change(input, { target: { value: "abba" } });
    fireEvent.submit(screen.getByRole("search"));
    expect(search).toHaveBeenCalledWith("abba");
  });

  test("shows an error with retry for a failed liked list", () => {
    useCatalogStore.setState({ browserTab: "liked", likedStatus: "error" });
    render(<NeteaseBrowser />);
    expect(screen.getByRole("alert").textContent).toContain(
      "catalog.netease.loadFailed",
    );
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    expect(loadLiked).toHaveBeenCalled();
  });

  test("shows the liked tracks and an empty message", () => {
    useCatalogStore.setState({
      browserTab: "liked",
      likedStatus: "ready",
      liked: [track("1")],
    });
    const { unmount } = render(<NeteaseBrowser />);
    expect(screen.getByText("Song 1")).toBeTruthy();
    unmount();

    useCatalogStore.setState({ liked: [] });
    render(<NeteaseBrowser />);
    expect(screen.getByText("catalog.netease.likedEmpty")).toBeTruthy();
  });

  test("opens a playlist from the grid", () => {
    useCatalogStore.setState({
      playlistsStatus: "ready",
      playlists: [
        {
          remote_id: "pl-1",
          name: "Night set",
          track_count: 2,
          cover_url: "https://p1.music.126.net/a.jpg",
        },
      ],
    });
    render(<NeteaseBrowser />);
    fireEvent.click(screen.getByRole("button", { name: /Night set/ }));
    expect(openPlaylist).toHaveBeenCalledWith("pl-1");
  });

  test("playlist detail imports the whole list or the selected rows", () => {
    useCatalogStore.setState({
      playlistsStatus: "ready",
      playlists: [
        {
          remote_id: "pl-1",
          name: "Night set",
          track_count: 3,
          cover_url: null,
        },
      ],
      selectedPlaylistId: "pl-1",
      playlistDetailStatus: "ready",
      playlistDetail: {
        remote_id: "pl-1",
        name: "Night set",
        cover_url: null,
        creator: "Davy",
        description: "Late drives",
        track_count: 3,
        updated_at_ms: 1_700_000_000_000,
        track_ids: [],
        partial: false,
        tracks: [
          track("1"),
          track("2"),
          track("3", {
            refusal: { reason: "trial_clip", title: "Song 3", artist: "A" },
          }),
        ],
      },
    });
    render(<NeteaseBrowser />);
    expect(screen.queryByRole("navigation")).toBeNull();
    expect(
      screen.getByRole("button", { name: "catalog.netease.back" }),
    ).toBeTruthy();
    expect(screen.getByText("Late drives")).toBeTruthy();
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);

    fireEvent.click(
      screen.getByRole("button", { name: "catalog.netease.importPlaylist" }),
    );
    expect(importTracks).toHaveBeenCalledWith(["1", "2"], "pl-1", "Night set");

    fireEvent.click(
      screen.getByRole("button", { name: "catalog.netease.select" }),
    );
    const primary = screen.getByRole("button", {
      name: "catalog.netease.importSelected",
    }) as HTMLButtonElement;
    expect(primary.disabled).toBe(true);

    // Unavailable rows are hidden by default, so two selectable rows remain.
    const boxes = screen.getAllByRole("checkbox");
    fireEvent.click(boxes[2]);
    expect(primary.disabled).toBe(false);
    fireEvent.click(primary);
    expect(importTracks).toHaveBeenLastCalledWith(["2"], "pl-1", "Night set");
  });

  test("hides unavailable tracks by default and can show them", () => {
    useCatalogStore.setState({
      browserTab: "liked",
      likedStatus: "ready",
      hideUnavailable: true,
      liked: [
        track("1"),
        track("2", {
          refusal: { reason: "no_play_rights", title: "Song 2", artist: "A" },
        }),
      ],
    });
    render(<NeteaseBrowser />);
    expect(screen.getAllByTestId("netease-track-row")).toHaveLength(1);
    expect(screen.getByText("catalog.netease.unavailableHidden")).toBeTruthy();

    fireEvent.click(screen.getByRole("switch"));
    expect(screen.getAllByTestId("netease-track-row")).toHaveLength(2);
  });

  test("renders only a window of a very long list", () => {
    useCatalogStore.setState({
      browserTab: "liked",
      likedStatus: "ready",
      liked: Array.from({ length: 3000 }, (_, i) => track(String(i))),
    });
    render(<NeteaseBrowser />);
    const rows = screen.getAllByTestId("netease-track-row");
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(60);
  });
});
