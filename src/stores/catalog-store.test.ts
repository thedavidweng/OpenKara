import { describe, expect, test, vi } from "vitest";
import { createCatalogStore } from "./catalog-store";
import { createMockBackend } from "@/lib/backend/mock-backend";
import type {
  StreamingImportEvent,
  StreamingImportTrackState,
} from "@/types/ipc";

vi.mock("@/lib/errors", () => ({
  notifyError: vi.fn(),
}));

describe("catalog store sessions", () => {
  test("web sign-in stores only the verified session snapshot", async () => {
    const backend = createMockBackend({
      overrides: {
        catalog: {
          openStreamingSignIn: async () => {},
        },
      },
    });
    const store = createCatalogStore(backend);
    await store.getState().openSignIn();
    expect(store.getState().session).toBeNull();
    store.getState().acceptSignIn({
      source_id: "netease",
      signed_in: true,
      display_name: "Ada",
      expired: false,
    });
    expect(store.getState().session?.display_name).toBe("Ada");
  });

  test("browse, import, and conflict helpers write store fields", async () => {
    const backend = createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: async () => ({
            source_id: "netease",
            signed_in: true,
            display_name: "Ada",
            expired: false,
          }),
          signOutStreamingSource: async () => ({
            source_id: "netease",
            signed_in: false,
            display_name: null,
            expired: false,
          }),
          listStreamingLikedTracks: async () =>
            listing([
              {
                source_id: "netease",
                remote_track_id: "1",
                title: "Liked",
                artist: "A",
                album: null,
                duration_ms: 1000,
                refusal: null,
                library_song_id: null,
                cover_url: null,
              },
            ]),
          listStreamingPlaylists: async () => [
            {
              remote_id: "pl-1",
              name: "Night",
              track_count: 1,
              cover_url: null,
            },
          ],
          getStreamingPlaylist: async () => ({
            remote_id: "pl-1",
            name: "Night",
            cover_url: null,
            creator: "Ada",
            description: "",
            track_count: 1,
            updated_at_ms: null,
            track_ids: [],
            partial: false,
            tracks: [
              {
                source_id: "netease",
                remote_track_id: "1",
                title: "Liked",
                artist: "A",
                album: null,
                duration_ms: 1000,
                refusal: null,
                library_song_id: null,
                cover_url: null,
              },
            ],
          }),
          searchStreamingSource: async () => [
            {
              source_id: "netease",
              remote_track_id: "2",
              title: "Hit",
              artist: "B",
              album: null,
              duration_ms: 2000,
              refusal: null,
              library_song_id: null,
              cover_url: null,
            },
          ],
          startStreamingImport: async () => ({
            status: "awaiting_decision",
            imported_song_ids: [],
            failed: [
              {
                remote_track_id: "9",
                title: "Grey",
                artist: "C",
                reason: "refusal",
                refusal: {
                  reason: "no_play_rights",
                  title: "Grey",
                  artist: "C",
                },
              },
            ],
            playlist_id: null,
            conflict: {
              source_id: "netease",
              remote_track_id: "1",
              library: {
                title: "Old",
                artist: "A",
                album: null,
                format: "MP3",
                bit_rate_bps: 192000,
                duration_ms: 1000,
                file_size_bytes: 100,
              },
              incoming: {
                title: "New",
                artist: "A",
                album: null,
                format: "FLAC",
                bit_rate_bps: 320000,
                duration_ms: 1000,
                file_size_bytes: 200,
              },
            },
          }),
          continueStreamingImport: async () => ({
            status: "completed",
            imported_song_ids: ["hash"],
            failed: [],
            playlist_id: "pl",
            conflict: null,
          }),
        },
      },
    });
    const store = createCatalogStore(backend);
    store.getState().setActiveView("netease");
    store.getState().rememberVideoItems([
      {
        id: "yt:abc",
        title: "Video",
        channel: "Ch",
        duration_ms: 1000,
        thumbnail_url: null,
        watch_url: "https://www.youtube.com/watch?v=abc",
      },
    ]);
    expect(store.getState().getVideoItem("yt:abc")?.title).toBe("Video");

    await store.getState().loadSession();
    expect(store.getState().session?.display_name).toBe("Ada");
    await store.getState().loadLiked();
    expect(store.getState().liked).toHaveLength(1);
    await store.getState().loadPlaylists();
    expect(store.getState().playlists[0]?.name).toBe("Night");
    await store.getState().openPlaylist("pl-1");
    expect(store.getState().playlistDetail?.remote_id).toBe("pl-1");
    await store.getState().search("hit");
    expect(store.getState().searchResults[0]?.title).toBe("Hit");
    await store.getState().importTracks(["1"], "pl-1");
    expect(store.getState().pendingConflict?.remote_track_id).toBe("1");
    expect(store.getState().importFailures).toHaveLength(1);
    await store.getState().resolveConflict("replace");
    expect(store.getState().pendingConflict).toBeNull();
    await store.getState().signOut();
    expect(store.getState().session?.signed_in).toBe(false);
    expect(store.getState().liked).toEqual([]);
  });

  test("catalog helpers notify when the backend rejects", async () => {
    const boom = new Error("offline");
    const reject = async () => {
      throw boom;
    };
    const backend = createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: reject,
          openStreamingSignIn: reject,
          signOutStreamingSource: reject,
          listStreamingLikedTracks: reject,
          listStreamingPlaylists: reject,
          getStreamingPlaylist: reject,
          searchStreamingSource: reject,
          startStreamingImport: reject,
          continueStreamingImport: reject,
        },
      },
    });
    const store = createCatalogStore(backend);
    const { notifyError } = await import("@/lib/errors");
    await store.getState().loadSession();
    await store.getState().openSignIn();
    await store.getState().signOut();
    await store.getState().loadLiked();
    await store.getState().loadPlaylists();
    await store.getState().openPlaylist("pl-1");
    await store.getState().search("night");
    await store.getState().importTracks(["1"]);
    await store.getState().resolveConflict("keep");
    expect(notifyError).toHaveBeenCalled();
    expect(vi.mocked(notifyError).mock.calls.length).toBeGreaterThanOrEqual(9);
  });
});

const track = (id: string) => ({
  source_id: "netease" as const,
  remote_track_id: id,
  title: `T${id}`,
  artist: "A",
  album: null,
  duration_ms: 1000,
  refusal: null,
  library_song_id: null,
  cover_url: null,
});

const listing = (tracks: ReturnType<typeof track>[]) => ({
  tracks,
  track_ids: tracks.map((item) => item.remote_track_id),
  partial: false,
});

describe("catalog store previews", () => {
  test("rememberPreviewTracks keys metadata by the ne: queue id", () => {
    const store = createCatalogStore(createMockBackend());
    store.getState().rememberPreviewTracks([
      {
        source_id: "netease",
        remote_track_id: "42",
        title: "Song",
        artist: "Artist",
        album: "Album",
        duration_ms: 1000,
        refusal: null,
        library_song_id: null,
        cover_url: "https://example.test/c.jpg",
      },
    ]);
    expect(store.getState().previewItems["ne:42"]).toMatchObject({
      id: "ne:42",
      remote_track_id: "42",
      title: "Song",
      artist: "Artist",
      cover_url: "https://example.test/c.jpg",
    });
  });
});

describe("catalog store paged loading", () => {
  const ids = Array.from({ length: 1201 }, (_, index) => `${index + 1}`);

  test("the liked list shows its first batch and grows page by page in order", async () => {
    const requested: string[][] = [];
    const getStreamingTracks = vi.fn(
      async (_source: string, pageIds: string[]) => {
        requested.push(pageIds);
        // Later pages finish first to prove ordering follows the id list.
        await new Promise((resolve) =>
          setTimeout(resolve, pageIds[0] === "101" ? 20 : 0),
        );
        return pageIds.map((id) => track(id));
      },
    );
    const backend = createMockBackend({
      overrides: {
        catalog: {
          listStreamingLikedTracks: async () => ({
            tracks: ids.slice(0, 100).map((id) => track(id)),
            track_ids: ids,
            partial: true,
          }),
          getStreamingTracks,
        },
      },
    });
    const store = createCatalogStore(backend);
    await store.getState().loadLiked();
    expect(store.getState().liked).toHaveLength(100);
    expect(store.getState().likedLoadingMore).toBe(true);
    await vi.waitFor(() =>
      expect(store.getState().likedLoadingMore).toBe(false),
    );
    expect(requested.map((page) => page.length)).toEqual([500, 500, 101]);
    expect(store.getState().liked.map((item) => item.remote_track_id)).toEqual(
      ids,
    );
  });

  test("a partial playlist keeps loading in the cache and is never refetched", async () => {
    const getStreamingTracks = vi.fn(
      async (_source: string, pageIds: string[]) =>
        pageIds.map((id) => track(id)),
    );
    const getStreamingPlaylist = vi.fn(async () => ({
      remote_id: "p",
      name: "P",
      cover_url: null,
      creator: null,
      description: null,
      track_count: 1201,
      updated_at_ms: null,
      track_ids: ids,
      partial: true,
      tracks: ids.slice(0, 1000).map((id) => track(id)),
    }));
    const store = createCatalogStore(
      createMockBackend({
        overrides: { catalog: { getStreamingPlaylist, getStreamingTracks } },
      }),
    );
    await store.getState().openPlaylist("p");
    expect(store.getState().playlistDetail?.partial).toBe(true);
    await vi.waitFor(() =>
      expect(store.getState().playlistDetail?.partial).toBe(false),
    );
    expect(store.getState().playlistDetail?.tracks).toHaveLength(1201);
    expect(getStreamingTracks).toHaveBeenCalledTimes(1);
    store.getState().closePlaylist();
    await store.getState().openPlaylist("p");
    expect(getStreamingPlaylist).toHaveBeenCalledTimes(1);
    expect(store.getState().playlistDetail?.tracks).toHaveLength(1201);
  });

  test("signing out stops page loads in flight", async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const backend = createMockBackend({
      overrides: {
        catalog: {
          listStreamingLikedTracks: async () => ({
            tracks: [track("1")],
            track_ids: ["1", "2"],
            partial: true,
          }),
          getStreamingTracks: async () => {
            await gate;
            return [track("2")];
          },
        },
      },
    });
    const store = createCatalogStore(backend);
    await store.getState().loadLiked();
    await store.getState().signOut();
    release();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(store.getState().liked).toEqual([]);
    expect(store.getState().likedLoadingMore).toBe(false);
  });
});

describe("catalog store browser state", () => {
  test("liked and playlists track lazy load status and errors", async () => {
    let fail = true;
    const backend = createMockBackend({
      overrides: {
        catalog: {
          listStreamingLikedTracks: async () => {
            if (fail) throw new Error("boom");
            return listing([track("1")]);
          },
          listStreamingPlaylists: async () => [
            { remote_id: "p", name: "P", track_count: 1, cover_url: null },
          ],
        },
      },
    });
    const store = createCatalogStore(backend);
    expect(store.getState().browserTab).toBe("playlists");
    expect(store.getState().likedStatus).toBe("idle");
    await store.getState().loadLiked();
    expect(store.getState().likedStatus).toBe("error");
    fail = false;
    await store.getState().loadLiked();
    expect(store.getState().likedStatus).toBe("ready");
    await store.getState().loadPlaylists();
    expect(store.getState().playlistsStatus).toBe("ready");
  });

  test("search switches to the results tab and clearSearch resets it", async () => {
    const backend = createMockBackend({
      overrides: {
        catalog: { searchStreamingSource: async () => [track("2")] },
      },
    });
    const store = createCatalogStore(backend);
    await store.getState().search("  hit  ");
    expect(store.getState().searchQuery).toBe("hit");
    expect(store.getState().browserTab).toBe("results");
    expect(store.getState().searchStatus).toBe("ready");
    await store.getState().search("   ");
    expect(store.getState().searchQuery).toBe("");
    expect(store.getState().searchResults).toEqual([]);
    expect(store.getState().browserTab).toBe("playlists");
  });

  test("reopening a playlist is served from the cache without a fetch", async () => {
    const getStreamingPlaylist = vi.fn(async () => ({
      remote_id: "p",
      name: "P",
      cover_url: null,
      creator: null,
      description: null,
      track_count: 1,
      updated_at_ms: null,
      track_ids: [],
      partial: false,
      tracks: [track("1")],
    }));
    const backend = createMockBackend({
      overrides: { catalog: { getStreamingPlaylist } },
    });
    const store = createCatalogStore(backend);
    await store.getState().openPlaylist("p");
    store.getState().closePlaylist();
    await store.getState().openPlaylist("p");
    expect(getStreamingPlaylist).toHaveBeenCalledTimes(1);
    expect(store.getState().playlistDetailStatus).toBe("ready");
    expect(store.getState().playlistDetail?.remote_id).toBe("p");
  });

  test("hideUnavailable defaults on and can be toggled", () => {
    const store = createCatalogStore(createMockBackend());
    expect(store.getState().hideUnavailable).toBe(true);
    store.getState().setHideUnavailable(false);
    expect(store.getState().hideUnavailable).toBe(false);
  });

  test("openPlaylist selects the playlist and closePlaylist clears it", async () => {
    const backend = createMockBackend({
      overrides: {
        catalog: {
          getStreamingPlaylist: async () => ({
            remote_id: "p",
            name: "P",
            cover_url: null,
            creator: null,
            description: null,
            track_count: 1,
            updated_at_ms: null,
            track_ids: [],
            partial: false,
            tracks: [track("1")],
          }),
        },
      },
    });
    const store = createCatalogStore(backend);
    const pending = store.getState().openPlaylist("p");
    expect(store.getState().selectedPlaylistId).toBe("p");
    expect(store.getState().playlistDetailStatus).toBe("loading");
    await pending;
    expect(store.getState().playlistDetailStatus).toBe("ready");
    store.getState().closePlaylist();
    expect(store.getState().selectedPlaylistId).toBeNull();
  });

  test("a completed import records imported and failed ids", async () => {
    const backend = createMockBackend({
      overrides: {
        catalog: {
          startStreamingImport: async () => ({
            status: "completed",
            imported_song_ids: ["h1"],
            failed: [
              {
                remote_track_id: "2",
                title: "T2",
                artist: "A",
                reason: "import_failed",
                refusal: null,
              },
            ],
            playlist_id: null,
            conflict: null,
          }),
        },
      },
    });
    const store = createCatalogStore(backend);
    const pending = store.getState().importTracks(["1", "2"]);
    expect(store.getState().importRun).toMatchObject({
      phase: "running",
      total: 2,
    });
    expect(store.getState().trackStates).toEqual({
      "1": { kind: "queued" },
      "2": { kind: "queued" },
    });
    await store.getState().importTracks(["3"]);
    expect(store.getState().importRun?.total).toBe(2);
    await pending;
    expect(store.getState().importRun).toMatchObject({
      phase: "done",
      imported: 1,
      skipped: 1,
    });
    expect(store.getState().trackStates["1"]).toMatchObject({
      kind: "imported",
    });
    expect(store.getState().trackStates["2"]).toEqual({
      kind: "failed",
      reason: "import_failed",
    });
    store.getState().dismissImportRun();
    expect(store.getState().importRun).toBeNull();
  });

  test("progress events update per-track state, counts and skipped list", async () => {
    let release: () => void = () => {};
    const backend = createMockBackend({
      overrides: {
        catalog: {
          startStreamingImport: () =>
            new Promise((resolve) => {
              release = () =>
                resolve({
                  status: "completed",
                  imported_song_ids: ["h1"],
                  failed: [
                    {
                      remote_track_id: "2",
                      title: "T2",
                      artist: "A",
                      reason: "refusal",
                      refusal: {
                        reason: "trial_clip",
                        title: "T2",
                        artist: "A",
                      },
                    },
                  ],
                  playlist_id: null,
                  conflict: null,
                });
            }),
        },
      },
    });
    const store = createCatalogStore(backend);
    const pending = store
      .getState()
      .importTracks(["1", "2"], null, "Night set");
    const event = (
      remote_track_id: string,
      state: StreamingImportTrackState,
      completed: number,
    ): StreamingImportEvent => ({
      job_id: "job-1",
      remote_track_id,
      title: `T${remote_track_id}`,
      artist: "A",
      state,
      completed,
      total: 2,
    });
    store
      .getState()
      .applyImportEvent(event("1", { kind: "downloading", percent: 40 }, 0));
    expect(store.getState().trackStates["1"]).toEqual({
      kind: "downloading",
      percent: 40,
    });
    store
      .getState()
      .applyImportEvent(event("1", { kind: "imported", song_id: "h1" }, 1));
    store.getState().applyImportEvent(
      event(
        "2",
        {
          kind: "skipped",
          refusal: { reason: "trial_clip", title: "T2", artist: "A" },
        },
        2,
      ),
    );
    // Events from another job are ignored.
    store.getState().applyImportEvent({
      ...event("1", { kind: "importing" }, 0),
      job_id: "other",
    });
    expect(store.getState().importRun).toMatchObject({
      jobId: "job-1",
      completed: 2,
      imported: 1,
      skipped: 1,
      label: "Night set",
    });
    expect(store.getState().importFailures).toHaveLength(1);
    release();
    await pending;
    expect(store.getState().importRun).toMatchObject({
      phase: "done",
      imported: 1,
      skipped: 1,
    });
    expect(store.getState().trackStates["1"]).toMatchObject({
      kind: "imported",
    });
  });

  test("cancelImport marks the run cancelling and calls the backend once", async () => {
    const cancelStreamingImport = vi.fn().mockResolvedValue(undefined);
    const backend = createMockBackend({
      overrides: {
        catalog: {
          startStreamingImport: () => new Promise(() => {}),
          cancelStreamingImport,
        },
      },
    });
    const store = createCatalogStore(backend);
    void store.getState().importTracks(["1"]);
    await store.getState().cancelImport();
    await store.getState().cancelImport();
    expect(store.getState().importRun?.cancelling).toBe(true);
    expect(cancelStreamingImport).toHaveBeenCalledTimes(1);
  });

  test("recoverImportJob restores a running job from the snapshot", async () => {
    const backend = createMockBackend({
      overrides: {
        catalog: {
          getStreamingImportStatus: async () => ({
            job_id: "job-9",
            phase: "running" as const,
            completed: 1,
            total: 2,
            tracks: [
              {
                remote_track_id: "1",
                title: "T1",
                artist: "A",
                state: { kind: "imported" as const, song_id: "h" },
              },
              {
                remote_track_id: "2",
                title: "T2",
                artist: "A",
                state: { kind: "downloading" as const, percent: 10 },
              },
            ],
          }),
        },
      },
    });
    const store = createCatalogStore(backend);
    await store.getState().recoverImportJob();
    expect(store.getState().importRun).toMatchObject({
      jobId: "job-9",
      phase: "running",
      total: 2,
      completed: 1,
      imported: 1,
    });
    expect(store.getState().trackStates["2"]).toEqual({
      kind: "downloading",
      percent: 10,
    });
  });

  test("recoverImportJob ignores a missing or finished job", async () => {
    const store = createCatalogStore(createMockBackend());
    await store.getState().recoverImportJob();
    expect(store.getState().importRun).toBeNull();
  });

  test("dismissImportRun is a no-op while an import is running", async () => {
    let release: () => void = () => {};
    const backend = createMockBackend({
      overrides: {
        catalog: {
          startStreamingImport: () =>
            new Promise((resolve) => {
              release = () =>
                resolve({
                  status: "completed",
                  imported_song_ids: ["h"],
                  failed: [],
                  playlist_id: null,
                  conflict: null,
                });
            }),
        },
      },
    });
    const store = createCatalogStore(backend);
    const pending = store.getState().importTracks(["1"]);
    store.getState().dismissImportRun();
    expect(store.getState().importRun?.phase).toBe("running");
    release();
    await pending;
    expect(store.getState().importRun?.phase).toBe("done");
  });

  test("sign-in errors are stored and cleared on success", () => {
    const store = createCatalogStore(createMockBackend());
    store.getState().setSignInError({
      code: "internal",
      message: "bad",
      retryable: false,
      fallback: "retry",
    });
    expect(store.getState().signInError).not.toBeNull();
    store.getState().acceptSignIn({
      source_id: "netease",
      signed_in: true,
      display_name: "Ada",
      expired: false,
    });
    expect(store.getState().signInError).toBeNull();
  });

  test("sign-out resets browse state", async () => {
    const backend = createMockBackend({
      overrides: {
        catalog: {
          listStreamingLikedTracks: async () => listing([track("1")]),
        },
      },
    });
    const store = createCatalogStore(backend);
    await store.getState().loadLiked();
    await store.getState().signOut();
    expect(store.getState().likedStatus).toBe("idle");
    expect(store.getState().liked).toEqual([]);
  });

  test("youtube link popover flag toggles", () => {
    const store = createCatalogStore(createMockBackend());
    store.getState().setYoutubeLinkOpen(true);
    expect(store.getState().youtubeLinkOpen).toBe(true);
  });
});
