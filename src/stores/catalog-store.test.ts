import { describe, expect, test, vi } from "vitest";
import { createCatalogStore } from "./catalog-store";
import { createMockBackend } from "@/lib/backend/mock-backend";

vi.mock("@/lib/errors", () => ({
  notifyError: vi.fn(),
}));

describe("catalog store sessions", () => {
  test("web sign-in stores only the verified session snapshot", async () => {
    const backend = createMockBackend({
      overrides: {
        catalog: {
          openStreamingSignIn: async () => {},
          completeStreamingSignIn: async () => ({
            source_id: "netease",
            signed_in: true,
            display_name: "Ada",
            expired: false,
          }),
        },
      },
    });
    const store = createCatalogStore(backend);
    await store.getState().openSignIn();
    expect(store.getState().session).toBeNull();
    await store.getState().completeSignIn();
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
          listStreamingLikedTracks: async () => [
            {
              source_id: "netease",
              remote_track_id: "1",
              title: "Liked",
              artist: "A",
              album: null,
              duration_ms: 1000,
              refusal: null,
            },
          ],
          listStreamingPlaylists: async () => [
            { remote_id: "pl-1", name: "Night", track_count: 1 },
          ],
          getStreamingPlaylist: async () => ({
            remote_id: "pl-1",
            name: "Night",
            tracks: [
              {
                source_id: "netease",
                remote_track_id: "1",
                title: "Liked",
                artist: "A",
                album: null,
                duration_ms: 1000,
                refusal: null,
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
          completeStreamingSignIn: reject,
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
    await store.getState().completeSignIn();
    await store.getState().signOut();
    await store.getState().loadLiked();
    await store.getState().loadPlaylists();
    await store.getState().openPlaylist("pl-1");
    await store.getState().search("night");
    await store.getState().importTracks(["1"]);
    await store.getState().resolveConflict("keep");
    expect(notifyError).toHaveBeenCalled();
    expect(vi.mocked(notifyError).mock.calls.length).toBeGreaterThanOrEqual(10);
  });
});
