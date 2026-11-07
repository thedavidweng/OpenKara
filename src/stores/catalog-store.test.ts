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
