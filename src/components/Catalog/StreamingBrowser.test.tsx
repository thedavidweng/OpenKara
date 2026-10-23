// @vitest-environment jsdom
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  act,
} from "@testing-library/react";
import {
  afterEach,
  beforeEach,
  beforeAll,
  afterAll,
  expect,
  test,
  vi,
} from "vitest";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { BackendProvider } from "@/lib/backend";
import { ToastContainer } from "@/components/Layout/ToastContainer";
import { useNotificationStore } from "@/stores/notification-store";
import {
  createCatalogStore,
  type CatalogStore,
  type CatalogState,
} from "@/stores/catalog-store";
import type { SourceSnapshot, StreamingTrackListing } from "@/types/ipc";
import { StreamingBrowser } from "./StreamingBrowser";
import { StreamingImportBar } from "./StreamingImportBar";
import type { StreamingImportProgress } from "@/types/ipc";

const shared = vi.hoisted(() => ({ store: null as unknown }));
vi.mock("@/stores/catalog-store", async (original) => {
  const actual = await original<{
    useCatalogStore: CatalogStore;
    createCatalogStore: typeof createCatalogStore;
  }>();
  return {
    ...actual,
    useCatalogStore: Object.assign(
      (selector: (state: CatalogState) => unknown) =>
        (shared.store as CatalogStore)(selector),
      { getState: () => (shared.store as CatalogStore).getState() },
    ),
  };
});
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
  initReactI18next: { type: "3rdParty", init() {} },
}));
const source = (id: string, capabilities: string[]): SourceSnapshot => ({
  source_id: id,
  owner_binding: `test/${id}`,
  name: id,
  description: "Test source",
  license: "MIT",
  installed_version: "1.0.0",
  available_version: "1.0.0",
  staged_version: null,
  pinned_version: null,
  enabled: true,
  runtime_ready: true,
  running: false,
  state: "ready",
  progress: null,
  last_error: null,
  active_work: [],
  manifest: {
    manifest_version: 1,
    source_id: id,
    package_id: id,
    publisher: "test",
    version: "1.0.0",
    minimum_host_version: "0.1.0",
    protocol: { majors: [1], minor: 0 },
    runtime: { kind: "node", major: 24 },
    entry: "index.js",
    name: id,
    description: "Test source",
    license: "MIT",
    capabilities,
    authentication: { flow: "none" },
    requires_authentication: [],
    resource_origins: [],
    media_origins: [],
  },
});
let store: CatalogStore;
beforeEach(() => {
  useNotificationStore.setState({ notifications: [] });
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: async (source_id) => ({
            source_id,
            signed_in: false,
            display_name: null,
            expired: false,
          }),
          listStreamingLikedTracks: async () => ({
            tracks: [],
            next_cursor: null,
          }),
        },
      },
    }),
  );
  shared.store = store;
});
afterEach(cleanup);

test("renders only supported operations and keyboard tab navigation", () => {
  store.getState().applySources([source("alpha", ["liked_tracks", "search"])]);
  store.getState().setActiveSource("alpha");
  render(<StreamingBrowser />);
  expect(
    screen.queryByRole("tab", { name: "catalog.streaming.playlists" }),
  ).toBeNull();
  const liked = screen.getByRole("tab", { name: "catalog.streaming.liked" });
  fireEvent.keyDown(liked, { key: "Home" });
  expect(document.activeElement).toBe(liked);
  expect(screen.getByRole("search")).toBeTruthy();
});

test("delayed searches stay bound to the original source and preserve each query", async () => {
  let finish!: (page: StreamingTrackListing) => void;
  const promise = new Promise<StreamingTrackListing>((resolve) => {
    finish = resolve;
  });
  const backend = createMockBackend({
    overrides: {
      catalog: {
        getStreamingSession: async (source_id) => ({
          source_id,
          signed_in: false,
          display_name: null,
          expired: false,
        }),
        searchStreamingSource: (id) =>
          id === "alpha"
            ? promise
            : Promise.resolve({ tracks: [], next_cursor: null }),
      },
    },
  });
  store = createCatalogStore(backend);
  shared.store = store;
  store
    .getState()
    .applySources([source("alpha", ["search"]), source("beta", ["search"])]);
  store.getState().setActiveSource("alpha");
  render(<StreamingBrowser />);
  fireEvent.change(screen.getByRole("textbox"), {
    target: { value: "Alpha query" },
  });
  fireEvent.submit(screen.getByRole("search"));
  act(() => store.getState().setActiveSource("beta"));
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("");
  await act(async () => finish({ tracks: [], next_cursor: "opaque-next" }));
  expect(
    screen.queryByRole("button", { name: "catalog.streaming.loadMore" }),
  ).toBeNull();
  act(() => store.getState().setActiveSource("alpha"));
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe(
    "Alpha query",
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "catalog.streaming.loadMore" }),
    ).toBeTruthy(),
  );
});

test("a loading error offers recovery without presenting an empty collection", async () => {
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: async (source_id) => ({
            source_id,
            signed_in: false,
            display_name: null,
            expired: false,
          }),
          listStreamingLikedTracks: vi
            .fn()
            .mockRejectedValueOnce(new Error("offline"))
            .mockResolvedValue({ tracks: [], next_cursor: null }),
        },
      },
    }),
  );
  shared.store = store;
  store.getState().applySources([source("alpha", ["liked_tracks"])]);
  store.getState().setActiveSource("alpha");
  render(<StreamingBrowser />);
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
  await screen.findByText("catalog.streaming.likedEmpty");
});

test("shared import remains visible after navigation and rejects another source or job's progress", async () => {
  let resolve!: (progress: StreamingImportProgress) => void;
  const promise = new Promise<StreamingImportProgress>((complete) => {
    resolve = complete;
  });
  const start = vi.fn().mockReturnValue(promise);
  store = createCatalogStore(
    createMockBackend({
      overrides: { catalog: { startStreamingImport: start } },
    }),
  );
  shared.store = store;
  store
    .getState()
    .applySources([source("alpha", ["import"]), source("beta", ["import"])]);
  store.getState().setActiveSource("alpha");
  render(<StreamingImportBar />);
  act(() => {
    void store.getState().importTracks(["same-id"], null, "Alpha track");
  });
  const jobId = start.mock.calls[0][3];
  act(() => store.getState().setActiveSource("beta"));
  expect(screen.getByText("alpha · Alpha track")).toBeTruthy();
  act(() =>
    store.getState().applyImportEvent({
      source_id: "beta",
      job_id: jobId,
      remote_track_id: "same-id",
      title: "Wrong",
      artist: "Wrong",
      state: { kind: "imported", song_id: "wrong-song" },
      completed: 1,
      total: 1,
    }),
  );
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe(
    "0",
  );
  act(() =>
    store.getState().applyImportEvent({
      source_id: "alpha",
      job_id: "stale-job",
      remote_track_id: "same-id",
      title: "Wrong",
      artist: "Wrong",
      state: { kind: "imported", song_id: "wrong-song" },
      completed: 1,
      total: 1,
    }),
  );
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe(
    "0",
  );
  act(() =>
    store.getState().applyImportEvent({
      source_id: "alpha",
      job_id: jobId,
      remote_track_id: "same-id",
      title: "Alpha track",
      artist: "A",
      state: { kind: "imported", song_id: "alpha-song" },
      completed: 1,
      total: 1,
    }),
  );
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe(
    "1",
  );
  await act(async () =>
    resolve({
      source_id: "alpha",
      status: "completed",
      imported_song_ids: ["alpha-song"],
      failed: [],
      playlist_id: null,
      conflict: null,
    }),
  );
  expect(screen.getByRole("button", { name: "common.close" })).toBeTruthy();
});

test("load more forwards an opaque continuation rather than treating it as track ids", async () => {
  const listing = vi
    .fn()
    .mockImplementation(async (_id: string, cursor: string | null) => ({
      tracks: [],
      next_cursor: cursor === null ? "signed opaque cursor/?=2" : null,
    }));
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: async (source_id) => ({
            source_id,
            signed_in: false,
            display_name: null,
            expired: false,
          }),
          listStreamingLikedTracks: listing,
        },
      },
    }),
  );
  shared.store = store;
  store.getState().applySources([source("alpha", ["liked_tracks"])]);
  store.getState().setActiveSource("alpha");
  render(<StreamingBrowser />);
  fireEvent.click(
    await screen.findByRole("button", { name: "catalog.streaming.loadMore" }),
  );
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: "catalog.streaming.loadMore" }),
    ).toBeNull(),
  );
  expect(listing.mock.calls[1]).toEqual(["alpha", "signed opaque cursor/?=2"]);
});

const viewportProps = ["offsetHeight", "clientHeight"] as const;
const originalViewport = viewportProps.map((prop) =>
  Object.getOwnPropertyDescriptor(HTMLElement.prototype, prop),
);
beforeAll(() => {
  for (const prop of viewportProps) {
    Object.defineProperty(HTMLElement.prototype, prop, {
      configurable: true,
      get: () => 600,
    });
  }
});
afterAll(() => {
  viewportProps.forEach((prop, index) => {
    const original = originalViewport[index];
    if (original) Object.defineProperty(HTMLElement.prototype, prop, original);
  });
});

test("selection stays with its source when equal remote ids appear elsewhere", async () => {
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: async (source_id) => ({
            source_id,
            signed_in: false,
            display_name: null,
            expired: false,
          }),
          listStreamingLikedTracks: async (source_id) => ({
            tracks: [
              {
                source_id,
                remote_track_id: "same-id",
                title: `${source_id} song`,
                artist: "Artist",
                album: null,
                duration_ms: 60000,
                refusal: null,
                library_song_id: null,
                cover_url: null,
              },
            ],
            next_cursor: null,
          }),
        },
      },
    }),
  );
  shared.store = store;
  store
    .getState()
    .applySources([
      source("alpha", ["liked_tracks", "import"]),
      source("beta", ["liked_tracks", "import"]),
    ]);
  store.getState().setActiveSource("alpha");
  render(<StreamingBrowser />);
  fireEvent.click(
    await screen.findByRole("button", { name: "catalog.streaming.select" }),
  );
  fireEvent.click(screen.getAllByRole("checkbox")[1]);
  expect((screen.getAllByRole("checkbox")[1] as HTMLInputElement).checked).toBe(
    true,
  );
  act(() => store.getState().setActiveSource("beta"));
  fireEvent.click(
    await screen.findByRole("button", { name: "catalog.streaming.select" }),
  );
  expect((screen.getAllByRole("checkbox")[1] as HTMLInputElement).checked).toBe(
    false,
  );
  act(() => store.getState().setActiveSource("alpha"));
  expect((screen.getAllByRole("checkbox")[1] as HTMLInputElement).checked).toBe(
    true,
  );
});

test("a valid source named library is distinct from local library navigation", () => {
  store.getState().applySources([source("library", ["search"])]);
  store.getState().setActiveSource("library");
  render(<StreamingBrowser />);
  expect(screen.getByRole("heading", { name: "library" })).toBeTruthy();
  fireEvent.change(screen.getByRole("textbox"), {
    target: { value: "Source draft" },
  });
  act(() => store.getState().setActiveView("library"));
  act(() => store.getState().setActiveSource("library"));
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe(
    "Source draft",
  );
});

test("protected playlist detail keeps Back available to return to public playlists", async () => {
  const fixture = source("alpha", ["playlists", "playlist_tracks"]);
  if (!fixture.manifest) throw new Error("Fixture requires a manifest");
  fixture.manifest.requires_authentication = ["playlist.tracks"];
  fixture.manifest.authentication = { flow: "web" };
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: async (source_id) => ({
            source_id,
            signed_in: false,
            expired: false,
            display_name: null,
          }),
          listStreamingPlaylists: async () => ({
            items: [
              {
                source_id: "alpha",
                remote_id: "playlist",
                name: "Public collection",
                track_count: 1,
                cover_url: null,
              },
            ],
            next_cursor: null,
          }),
          getStreamingPlaylist: async () => {
            throw new Error("Authentication required");
          },
        },
      },
    }),
  );
  shared.store = store;
  store.getState().applySources([fixture]);
  store.getState().setActiveSource("alpha");
  render(<StreamingBrowser />);
  fireEvent.click(
    await screen.findByRole("button", { name: /Public collection/ }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "catalog.streaming.back" }),
  );
  expect(
    screen.getByRole("button", { name: /Public collection/ }),
  ).toBeTruthy();
});

test("a failed new search cannot reuse the previous query's continuation", async () => {
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: async (source_id) => ({
            source_id,
            signed_in: false,
            expired: false,
            display_name: null,
          }),
          searchStreamingSource: async (_id, query) => {
            if (query === "first")
              return { tracks: [], next_cursor: "first-only-cursor" };
            throw new Error("Search offline");
          },
        },
      },
    }),
  );
  shared.store = store;
  store.getState().applySources([source("alpha", ["search"])]);
  store.getState().setActiveSource("alpha");
  render(<StreamingBrowser />);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "first" } });
  fireEvent.submit(screen.getByRole("search"));
  await screen.findByRole("button", { name: "catalog.streaming.loadMore" });
  fireEvent.change(screen.getByRole("textbox"), {
    target: { value: "second" },
  });
  fireEvent.submit(screen.getByRole("search"));
  await screen.findByRole("alert");
  expect(
    screen.queryByRole("button", { name: "catalog.streaming.loadMore" }),
  ).toBeNull();
});

test("importing selected liked songs after viewing a playlist does not inherit its origin", async () => {
  const track = {
    source_id: "alpha",
    remote_track_id: "liked-only",
    title: "Liked song",
    artist: "Artist",
    album: null,
    duration_ms: 1000,
    refusal: null,
    library_song_id: null,
    cover_url: null,
  };
  const start = vi.fn().mockResolvedValue({
    source_id: "alpha",
    status: "completed",
    imported_song_ids: ["local-song"],
    failed: [],
    playlist_id: null,
    conflict: null,
  });
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: async (source_id) => ({
            source_id,
            signed_in: false,
            expired: false,
            display_name: null,
          }),
          listStreamingPlaylists: async () => ({
            items: [
              {
                source_id: "alpha",
                remote_id: "playlist",
                name: "Other collection",
                track_count: 0,
                cover_url: null,
              },
            ],
            next_cursor: null,
          }),
          getStreamingPlaylist: async () => ({
            source_id: "alpha",
            remote_id: "playlist",
            name: "Other collection",
            track_count: 0,
            tracks: [],
            cover_url: null,
            creator: null,
            updated_at_ms: null,
            description: null,
            next_cursor: null,
          }),
          listStreamingLikedTracks: async () => ({
            tracks: [track],
            next_cursor: null,
          }),
          startStreamingImport: start,
        },
      },
    }),
  );
  shared.store = store;
  store
    .getState()
    .applySources([
      source("alpha", [
        "liked_tracks",
        "playlists",
        "playlist_tracks",
        "import",
      ]),
    ]);
  store.getState().setActiveSource("alpha");
  render(<StreamingBrowser />);
  fireEvent.click(
    await screen.findByRole("button", { name: /Other collection/ }),
  );
  await screen.findByRole("heading", { name: "Other collection" });
  fireEvent.click(screen.getByRole("tab", { name: "catalog.streaming.liked" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "catalog.streaming.select" }),
  );
  fireEvent.click(screen.getAllByRole("checkbox")[1]);
  fireEvent.click(
    screen.getByRole("button", { name: "catalog.streaming.importSelected" }),
  );
  await waitFor(() => expect(start).toHaveBeenCalled());
  expect(start.mock.calls[0].slice(0, 3)).toEqual([
    "alpha",
    ["liked-only"],
    null,
  ]);
});

test("web sign-in failure does not abandon an in-flight public collection", async () => {
  let finish!: (page: StreamingTrackListing) => void;
  const pending = new Promise<StreamingTrackListing>((resolve) => {
    finish = resolve;
  });
  const fixture = source("alpha", ["liked_tracks"]);
  if (!fixture.manifest) throw new Error("Fixture requires a manifest");
  fixture.manifest.authentication = { flow: "web" };
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: async (source_id) => ({
            source_id,
            signed_in: false,
            expired: false,
            display_name: null,
          }),
          listStreamingLikedTracks: () => pending,
          openStreamingSignIn: async (source_id) => ({
            source_id,
            challenge: "web-alpha",
            expires_at: "2099-01-01T00:00:00Z",
            poll_after_ms: 60000,
            presentation: { kind: "web" },
          }),
        },
      },
    }),
  );
  shared.store = store;
  store.getState().applySources([fixture]);
  store.getState().setActiveSource("alpha");
  render(<StreamingBrowser />);
  await act(async () =>
    fireEvent.click(
      screen.getByRole("button", { name: "catalog.streaming.connect" }),
    ),
  );
  act(() =>
    store.getState().setSignInError("alpha", "web-alpha", {
      code: "internal",
      message: "Window closed",
      retryable: true,
      fallback: "retry",
    }),
  );
  await act(async () => finish({ tracks: [], next_cursor: null }));
  expect(screen.getByText("catalog.streaming.likedEmpty")).toBeTruthy();
});

test("playlist import exposes its global cancellable job before backend pagination finishes", async () => {
  let finish!: (progress: StreamingImportProgress) => void;
  const preparing = new Promise<StreamingImportProgress>((resolve) => {
    finish = resolve;
  });
  const start = vi.fn().mockReturnValue(preparing);
  const cancel = vi.fn().mockResolvedValue(undefined);
  const detail = vi.fn().mockResolvedValue({
    source_id: "alpha",
    remote_id: "playlist",
    name: "Long collection",
    track_count: null,
    tracks: [],
    cover_url: null,
    creator: null,
    updated_at_ms: null,
    description: null,
    next_cursor: "host-owned-continuation",
  });
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: async (source_id) => ({
            source_id,
            signed_in: false,
            expired: false,
            display_name: null,
          }),
          listStreamingPlaylists: async () => ({
            items: [
              {
                source_id: "alpha",
                remote_id: "playlist",
                name: "Long collection",
                track_count: null,
                cover_url: null,
              },
            ],
            next_cursor: null,
          }),
          getStreamingPlaylist: detail,
          startStreamingImport: start,
          cancelStreamingImport: cancel,
        },
      },
    }),
  );
  shared.store = store;
  store
    .getState()
    .applySources([
      source("alpha", ["playlists", "playlist_tracks", "import"]),
      source("beta", ["search"]),
    ]);
  store.getState().setActiveSource("alpha");
  render(
    <>
      <StreamingBrowser />
      <StreamingImportBar />
    </>,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /Long collection/ }),
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: "catalog.streaming.importPlaylist",
    }),
  );
  expect(start.mock.calls[0].slice(0, 3)).toEqual(["alpha", [], "playlist"]);
  act(() => store.getState().setActiveSource("beta"));
  fireEvent.click(
    screen.getByRole("button", { name: "catalog.streaming.cancelImport" }),
  );
  expect(cancel).toHaveBeenCalled();
  expect(detail.mock.calls).toHaveLength(1);
  await act(async () =>
    finish({
      source_id: "alpha",
      status: "completed",
      imported_song_ids: [],
      failed: [],
      playlist_id: null,
      conflict: null,
    }),
  );
  expect(screen.getByRole("button", { name: "common.close" })).toBeTruthy();
});

test("failed account sign-out shows the backend error and retains the verified account", async () => {
  const backend = createMockBackend({
    overrides: {
      catalog: {
        getStreamingSession: async (source_id) => ({
          source_id,
          signed_in: true,
          display_name: "Alice",
          expired: false,
        }),
        signOutStreamingSource: async () => {
          throw new Error("Sign-out could not stop the source");
        },
      },
    },
  });
  store = createCatalogStore(backend);
  shared.store = store;
  store.getState().applySources([source("alpha", ["search"])]);
  store.getState().setActiveSource("alpha");
  render(
    <BackendProvider backend={backend}>
      <StreamingBrowser />
      <ToastContainer />
    </BackendProvider>,
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: "catalog.streaming.accountMenu",
    }),
  );
  fireEvent.click(
    screen.getByRole("menuitem", { name: "catalog.streaming.signOut" }),
  );
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Sign-out could not stop the source",
  );
  expect(
    screen
      .getByRole("button", { name: "catalog.streaming.accountMenu" })
      .getAttribute("title"),
  ).toBe("Alice");
  expect(screen.queryByRole("menu")).toBeNull();
});

test("account sign-out requires a stop decision for active work and permits declining it", async () => {
  const signOut = vi.fn().mockResolvedValue({
    source_id: "alpha",
    signed_in: false,
    display_name: null,
    expired: false,
  });
  const backend = createMockBackend({
    overrides: {
      catalog: {
        getStreamingSession: async (source_id) => ({
          source_id,
          signed_in: true,
          display_name: "Alice",
          expired: false,
        }),
        signOutStreamingSource: signOut,
      },
    },
  });
  store = createCatalogStore(backend);
  shared.store = store;
  const fixture = source("alpha", ["search"]);
  fixture.active_work = [{ id: 7, kind: "preview", cancelling: false }];
  if (!fixture.manifest) throw new Error("Fixture requires a manifest");
  fixture.manifest.authentication = { flow: "web" };
  store.getState().applySources([fixture]);
  store.getState().setActiveSource("alpha");
  render(
    <BackendProvider backend={backend}>
      <StreamingBrowser />
    </BackendProvider>,
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: "catalog.streaming.accountMenu",
    }),
  );
  fireEvent.click(
    screen.getByRole("menuitem", { name: "catalog.streaming.signOut" }),
  );
  expect(screen.getByRole("dialog").textContent).toContain(
    "settings.streaming.work.preview",
  );
  expect(signOut).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(
    screen
      .getByRole("button", { name: "catalog.streaming.accountMenu" })
      .getAttribute("title"),
  ).toBe("Alice");
  expect(signOut).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "catalog.streaming.accountMenu" }),
  );
  fireEvent.click(
    screen.getByRole("menuitem", { name: "catalog.streaming.signOut" }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "settings.streaming.stopWork" }),
  );
  await screen.findByRole("button", { name: "catalog.streaming.connect" });
  expect(
    screen.queryByRole("button", { name: "catalog.streaming.accountMenu" }),
  ).toBeNull();
  expect(signOut).toHaveBeenCalledWith("alpha", true);
});

test("account menu focuses its action, restores focus on Escape, and dismisses on Tab or outside pointer", async () => {
  const backend = createMockBackend({
    overrides: {
      catalog: {
        getStreamingSession: async (source_id) => ({
          source_id,
          signed_in: true,
          display_name: "Alice",
          expired: false,
        }),
      },
    },
  });
  store = createCatalogStore(backend);
  shared.store = store;
  store.getState().applySources([source("alpha", ["search"])]);
  store.getState().setActiveSource("alpha");
  render(
    <BackendProvider backend={backend}>
      <StreamingBrowser />
    </BackendProvider>,
  );
  const trigger = await screen.findByRole("button", {
    name: "catalog.streaming.accountMenu",
  });
  fireEvent.click(trigger);
  const action = screen.getByRole("menuitem", {
    name: "catalog.streaming.signOut",
  });
  expect(document.activeElement).toBe(action);
  fireEvent.keyDown(action, { key: "Escape" });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(trigger);
  fireEvent.click(trigger);
  fireEvent.keyDown(screen.getByRole("menuitem"), { key: "Tab" });
  expect(screen.queryByRole("menu")).toBeNull();
  fireEvent.click(trigger);
  fireEvent.mouseDown(screen.getByRole("textbox"));
  expect(screen.queryByRole("menu")).toBeNull();
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
});
