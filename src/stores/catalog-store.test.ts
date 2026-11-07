import { describe, expect, test, vi } from "vite-plus/test";
import {
  createMockBackend,
  DEFAULT_MOCK_DATA,
} from "@/lib/backend/mock-backend";
import { source } from "@/test-utils/streaming-source";
import { onlinePreviewQueueId } from "@/playback";
import type {
  PlayableMatches,
  StreamingImportProgress,
  StreamingTrack,
  StreamingTrackListing,
} from "@/types/ipc";
import { canFindPlayableMatch, createCatalogStore } from "./catalog-store";

describe("Streaming Source support", () => {
  test("a build with Streaming Sources reports them once and keeps the answer", async () => {
    const backend = createMockBackend();
    const supported = vi.spyOn(backend.catalog, "streamingSourcesSupported");
    const store = createCatalogStore(backend);

    expect(store.getState().sourcesSupported).toBeNull();
    await expect(store.getState().loadSourcesSupport()).resolves.toBe(true);
    await expect(store.getState().loadSourcesSupport()).resolves.toBe(true);

    expect(store.getState().sourcesSupported).toBe(true);
    expect(supported).toHaveBeenCalledTimes(1);
  });

  test("concurrent callers share one support check", async () => {
    const backend = createMockBackend();
    const supported = vi.spyOn(backend.catalog, "streamingSourcesSupported");
    const store = createCatalogStore(backend);

    await expect(
      Promise.all([
        store.getState().loadSourcesSupport(),
        store.getState().loadSourcesSupport(),
      ]),
    ).resolves.toEqual([true, true]);
    expect(supported).toHaveBeenCalledTimes(1);
  });

  test("a build without Streaming Sources reports them unsupported", async () => {
    const store = createCatalogStore(
      createMockBackend({
        data: { ...DEFAULT_MOCK_DATA, streamingSourcesSupported: false },
      }),
    );

    await expect(store.getState().loadSourcesSupport()).resolves.toBe(false);
    expect(store.getState().sourcesSupported).toBe(false);
  });

  test("a failed support check leaves the source surface off", async () => {
    const store = createCatalogStore(
      createMockBackend({
        overrides: {
          catalog: {
            streamingSourcesSupported: () =>
              Promise.reject(new Error("IPC unavailable")),
          },
        },
      }),
    );

    await expect(store.getState().loadSourcesSupport()).resolves.toBe(false);
    expect(store.getState().sourcesSupported).toBe(false);
  });
});

describe("global search", () => {
  test("a newer query drops results that arrive late for an older one", async () => {
    let finishOld!: (page: StreamingTrackListing) => void;
    const search = vi
      .fn()
      .mockReturnValueOnce(
        new Promise<StreamingTrackListing>((resolve) => {
          finishOld = resolve;
        }),
      )
      .mockResolvedValue({ tracks: [], next_cursor: "next" });
    const store = createCatalogStore(
      createMockBackend({
        overrides: {
          catalog: {
            getStreamingSession: async (source_id) => ({
              source_id,
              signed_in: false,
              display_name: null,
              expired: false,
            }),
            searchStreamingSource: search,
          },
        },
      }),
    );
    const first = source("alpha", ["search"]);
    store.getState().applySources([first]);
    const old = store.getState().searchEverywhere("old");
    await vi.waitFor(() => expect(search).toHaveBeenCalledTimes(1));
    await store.getState().searchEverywhere("new");
    finishOld({ tracks: [], next_cursor: "stale" });
    await old;

    const { query, sections } = store.getState().globalSearch;
    expect(query).toBe("new");
    expect(sections[first.source_id]).toMatchObject({
      status: "ready",
      cursor: "next",
    });
  });
});

describe("Playable Match", () => {
  const original: StreamingTrack = {
    source_id: "meta",
    remote_track_id: "orig",
    title: "Sunny Day",
    artist: "Jay Chou",
    album: null,
    duration_ms: 180000,
    refusal: { reason: "empty_url", title: "Sunny Day", artist: "Jay Chou" },
    library_song_id: null,
    cover_url: null,
    isrc: null,
  };
  const candidate: StreamingTrack = {
    ...original,
    source_id: "files",
    remote_track_id: "cand",
    refusal: null,
  };
  const progress = (failed = false): StreamingImportProgress => ({
    source_id: "files",
    status: "completed",
    imported_song_ids: failed ? [] : ["song"],
    failed: failed
      ? [
          {
            remote_track_id: "cand",
            title: "Sunny Day",
            artist: "Jay Chou",
            reason: "import_failed",
            refusal: null,
          },
        ]
      : [],
    playlist_id: null,
    conflict: null,
  });
  const setup = (matches: PlayableMatches) => {
    const calls: string[] = [];
    const backend = createMockBackend({
      overrides: {
        catalog: {
          findPlayableMatches: async () => matches,
          rememberPlayableMatch: async () => {
            calls.push("remember");
          },
          startStreamingImport: async (sourceId: string, ids: string[]) => {
            calls.push(`import ${sourceId} ${ids.join()}`);
            return progress();
          },
        },
      },
    });
    return { calls, store: createCatalogStore(backend) };
  };

  test("only a track without audio can look for a playable version", () => {
    const sources = [source("meta", ["search"]), source("files", ["search"])];
    expect(canFindPlayableMatch(sources, original)).toBe(true);
    expect(
      canFindPlayableMatch(sources, {
        ...original,
        refusal: { ...original.refusal!, reason: "no_play_rights" },
      }),
    ).toBe(false);
    expect(canFindPlayableMatch([sources[0]], original)).toBe(false);
  });

  test("the user's pick is remembered, then imported from the candidate source", async () => {
    const { calls, store } = setup({
      remembered: null,
      candidates: [candidate],
    });

    await store.getState().findPlayableMatch(original);
    expect(store.getState().playableMatch).toMatchObject({
      status: "ready",
      candidates: [candidate],
    });

    await store.getState().confirmPlayableMatch(candidate);
    expect(calls).toEqual(["remember", "import files cand"]);
    expect(store.getState().playableMatch).toBeNull();
    expect(
      store.getState().trackStates[onlinePreviewQueueId("meta", "orig")],
    ).toEqual({ kind: "imported", song_id: "" });
  });

  test("a remembered match imports without a dialog", async () => {
    const { calls, store } = setup({
      remembered: { source_id: "files", remote_track_id: "cand" },
      candidates: [],
    });

    await store.getState().findPlayableMatch(original);

    expect(calls).toEqual(["import files cand"]);
    expect(store.getState().playableMatch).toBeNull();
  });

  test("closing the dialog drops a late search result", async () => {
    let finish!: (matches: PlayableMatches) => void;
    const store = createCatalogStore(
      createMockBackend({
        overrides: {
          catalog: {
            findPlayableMatches: () =>
              new Promise<PlayableMatches>((resolve) => {
                finish = resolve;
              }),
          },
        },
      }),
    );

    const pending = store.getState().findPlayableMatch(original);
    expect(store.getState().playableMatch?.status).toBe("loading");
    store.getState().closePlayableMatch();
    finish({ remembered: null, candidates: [candidate] });
    await pending;

    expect(store.getState().playableMatch).toBeNull();
  });
});

describe("playlist-wide Playable Match", () => {
  const listed = (
    sourceId: string,
    id: string,
    overrides: Partial<StreamingTrack> = {},
  ): StreamingTrack => ({
    source_id: sourceId,
    remote_track_id: id,
    title: `Song ${id}`,
    artist: "Artist",
    album: null,
    duration_ms: 180000,
    refusal: null,
    library_song_id: null,
    cover_url: null,
    isrc: null,
    ...overrides,
  });
  const refused = (id: string) =>
    listed("meta", id, {
      refusal: { reason: "empty_url", title: `Song ${id}`, artist: "Artist" },
    });
  const tracks = [
    refused("a"),
    listed("meta", "own", { library_song_id: "song-own" }),
    refused("b"),
    refused("c"),
    refused("d"),
  ];
  const candidates: Record<string, StreamingTrack[]> = {
    a: [listed("files", "a1"), listed("web", "a2")],
    b: [listed("web", "b1")],
    c: [listed("files", "c1")],
  };
  const rememberedD = listed("web", "d1");

  const setup = async ({
    stamped = null,
    existing = [],
  }: { stamped?: string | null; existing?: string[] } = {}) => {
    let searching = 0;
    let maxSearching = 0;
    let importing = 0;
    let maxImporting = 0;
    const imports: Array<{ source: string; ids: string[]; playlist: unknown }> =
      [];
    const remembered: string[] = [];
    const added: Array<{ playlist: string; songs: string[] }> = [];
    const created: string[] = [];
    let holder!: ReturnType<typeof createCatalogStore>;
    const backend = createMockBackend({
      overrides: {
        catalog: {
          getStreamingPlaylist: async (sourceId: string, remoteId: string) => ({
            source_id: sourceId,
            remote_id: remoteId,
            name: "Road trip",
            tracks,
            next_cursor: null,
            cover_url: null,
            description: null,
            track_count: tracks.length,
            creator: null,
            updated_at_ms: null,
          }),
          findPlayableMatches: async (_source: string, id: string) => {
            maxSearching = Math.max(maxSearching, ++searching);
            await new Promise((resolve) => setTimeout(resolve, 5));
            searching--;
            return id === "d"
              ? {
                  remembered: {
                    source_id: "web",
                    remote_track_id: "d1",
                  },
                  candidates: [],
                }
              : { remembered: null, candidates: candidates[id] ?? [] };
          },
          getStreamingTracks: async () => [rememberedD],
          rememberPlayableMatch: async (
            _source: string,
            id: string,
            _matchSource: string,
            matchId: string,
          ) => {
            remembered.push(`${id}>${matchId}`);
          },
          startStreamingImport: async (
            sourceId: string,
            ids: string[],
            playlist: string | null,
            jobId: string,
          ) => {
            maxImporting = Math.max(maxImporting, ++importing);
            imports.push({ source: sourceId, ids, playlist });
            await new Promise((resolve) => setTimeout(resolve, 5));
            for (const id of ids)
              holder.getState().applyImportEvent({
                source_id: sourceId,
                job_id: jobId,
                remote_track_id: id,
                title: id,
                artist: "Artist",
                state: { kind: "imported", song_id: `song-${id}` },
                completed: 1,
                total: ids.length,
              });
            importing--;
            return {
              source_id: sourceId,
              status: "completed" as const,
              imported_song_ids: ids.map((id) => `song-${id}`),
              failed: [],
              playlist_id: null,
              conflict: null,
            };
          },
          streamingPlaylistLibraryId: async () => stamped,
        },
        playlist: {
          listPlaylists: async () => [
            {
              id: "pl-existing",
              name: "Imported earlier",
              song_count: 0,
              created_at: 0,
              updated_at: 0,
            },
          ],
          getPlaylistSongs: async () =>
            existing.map((song_hash, sort_order) => ({
              song_hash,
              added_at: 0,
              sort_order,
              singer: null,
            })),
          createPlaylist: async (name: string) => {
            created.push(name);
            return {
              id: "pl-new",
              name,
              song_count: 0,
              created_at: 0,
              updated_at: 0,
            };
          },
          addSongsToPlaylist: async (playlist: string, songs: string[]) => {
            added.push({ playlist, songs });
          },
        },
      },
    });
    holder = createCatalogStore(backend);
    holder
      .getState()
      .applySources([
        source("meta", ["search", "playlists", "playlist_tracks"]),
        source("files", ["search", "import"]),
        source("web", ["search", "import"]),
      ]);
    holder.getState().setActiveSource("meta");
    await holder.getState().openPlaylist("pl");
    return {
      store: holder,
      imports,
      remembered,
      added,
      created,
      concurrency: () => ({ maxSearching, maxImporting }),
    };
  };

  test("searches the matchable tracks 3 at a time and preselects the remembered match", async () => {
    const { store, concurrency } = await setup();

    const pending = store.getState().findPlaylistMatches();
    expect(store.getState().playlistMatch).toMatchObject({
      phase: "searching",
      searchTotal: 4,
    });
    await pending;

    expect(concurrency().maxSearching).toBe(3);
    const match = store.getState().playlistMatch!;
    expect(match).toMatchObject({ phase: "review", searched: 4 });
    expect(match.rows.map((row) => row.original.remote_track_id)).toEqual([
      "a",
      "b",
      "c",
      "d",
    ]);
    expect(match.rows.map((row) => row.choice)).toEqual([
      onlinePreviewQueueId("files", "a1"),
      onlinePreviewQueueId("web", "b1"),
      onlinePreviewQueueId("files", "c1"),
      onlinePreviewQueueId("web", "d1"),
    ]);
    expect(match.rows[3].rememberedKey).toBe(match.rows[3].choice);
  });

  test("a track without candidates has no choice and is listed as unmatched", async () => {
    const { store, imports } = await setup();
    candidates.c = [];
    try {
      await store.getState().findPlaylistMatches();
      expect(store.getState().playlistMatch!.rows[2]).toMatchObject({
        options: [],
        choice: null,
      });
      store.getState().setPlaylistMatchChoice(1, null);
      await store.getState().confirmPlaylistMatches();
    } finally {
      candidates.c = [listed("files", "c1")];
    }

    expect(imports.flatMap((job) => job.ids).sort()).toEqual(["a1", "d1"]);
    expect(
      store.getState().playlistMatch!.failures.map((f) => [f.title, f.reason]),
    ).toEqual([
      ["Song b", "skipped"],
      ["Song c", "noCandidate"],
    ]);
  });

  test("imports one job per source in turn with no Playlist Origin Stamp, then creates an unstamped Playlist in playlist order", async () => {
    const { store, imports, remembered, added, created, concurrency } =
      await setup();
    await store.getState().findPlaylistMatches();
    store
      .getState()
      .setPlaylistMatchChoice(1, onlinePreviewQueueId("web", "b1"));

    await store.getState().confirmPlaylistMatches();

    expect(imports).toEqual([
      { source: "files", ids: ["a1", "c1"], playlist: null },
      { source: "web", ids: ["b1", "d1"], playlist: null },
    ]);
    expect(concurrency().maxImporting).toBe(1);
    expect(remembered).toEqual(["a>a1", "b>b1", "c>c1"]);
    expect(created).toEqual(["Road trip"]);
    expect(added).toEqual([
      {
        playlist: "pl-new",
        songs: ["song-a1", "song-own", "song-b1", "song-c1", "song-d1"],
      },
    ]);
    expect(store.getState().playlistMatch).toMatchObject({
      phase: "done",
      imported: 4,
      playlistName: "Road trip",
      failures: [],
    });
  });

  test("appends only the matched songs that the stamped Playlist lacks", async () => {
    const { store, added, created } = await setup({
      stamped: "pl-existing",
      existing: ["song-a1"],
    });
    await store.getState().findPlaylistMatches();

    await store.getState().confirmPlaylistMatches();

    expect(created).toEqual([]);
    expect(added).toEqual([
      {
        playlist: "pl-existing",
        songs: ["song-b1", "song-c1", "song-d1"],
      },
    ]);
    expect(store.getState().playlistMatch?.playlistName).toBe(
      "Imported earlier",
    );
  });

  test("leaves Playlists alone when the box is unchecked", async () => {
    const { store, added, created } = await setup();
    await store.getState().findPlaylistMatches();
    store.getState().setPlaylistMatchAddToPlaylist(false);

    await store.getState().confirmPlaylistMatches();

    expect(created).toEqual([]);
    expect(added).toEqual([]);
    expect(store.getState().playlistMatch?.playlistName).toBeNull();
  });
});
