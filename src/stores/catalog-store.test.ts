import { describe, expect, test, vi } from "vitest";
import { createCatalogStore } from "./catalog-store";
import { createMockBackend } from "@/lib/backend/mock-backend";

vi.mock("@/lib/errors", () => ({
  notifyError: vi.fn(),
}));

describe("catalog store QR status", () => {
  test("startQr records waiting and pollQr can move to scanned then signed in", async () => {
    const backend = createMockBackend({
      overrides: {
        catalog: {
          startStreamingQrSignin: async () => ({
            key: "unikey",
            login_url: "https://music.163.com/login?codekey=unikey",
            qr_svg: "<svg></svg>",
          }),
          pollStreamingQrSignin: vi
            .fn()
            .mockResolvedValueOnce({ status: "scanned", session: null })
            .mockResolvedValueOnce({
              status: "confirmed",
              session: {
                source_id: "netease",
                signed_in: true,
                display_name: "Ada",
                expired: false,
              },
            }),
        },
      },
    });
    const store = createCatalogStore(backend);
    await store.getState().startQr();
    expect(store.getState().qr?.key).toBe("unikey");
    expect(store.getState().qrStatus).toBe("waiting");

    await store.getState().pollQr();
    expect(store.getState().qrStatus).toBe("scanned");
    expect(store.getState().session).toBeNull();

    await store.getState().pollQr();
    expect(store.getState().session?.display_name).toBe("Ada");
    expect(store.getState().qr).toBeNull();
    expect(store.getState().qrStatus).toBeNull();
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

  test("password sign-in is forgotten by the store after the command returns", async () => {
    const signIn = vi.fn().mockResolvedValue({
      source_id: "netease",
      signed_in: true,
      display_name: "Ada",
      expired: false,
    });
    const backend = createMockBackend({
      overrides: {
        catalog: {
          signInStreamingSource: signIn,
        },
      },
    });
    const store = createCatalogStore(backend);
    await store.getState().signInPassword("email", "a@b.c", "once-only");
    expect(signIn).toHaveBeenCalledWith(
      "netease",
      "email",
      "a@b.c",
      "once-only",
      undefined,
    );
    expect(store.getState().session?.signed_in).toBe(true);
    expect(JSON.stringify(store.getState())).not.toContain("once-only");
  });
});
