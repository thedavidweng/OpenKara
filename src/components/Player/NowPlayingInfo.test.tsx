import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import type { Song, SourceSnapshot } from "@/types/ipc";
import { NowPlayingInfo } from "./NowPlayingInfo";

const { mockPlayerState, mockLibraryState } = vi.hoisted(() => ({
  mockPlayerState: {
    snapshot: {
      song_id: "song-1",
    },
  },
  mockLibraryState: {
    songs: [
      {
        hash: "song-1",
        file_path: "Taylor Swift/22.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        title: "22",
        artist: "Taylor Swift",
        album: null,
        duration_ms: 242000,
        cover_art: [0xff, 0xd8, 0x00],
        has_cover_art: true,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ] as Song[],
  },
}));

vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: (selector: (state: typeof mockPlayerState) => unknown) =>
    selector(mockPlayerState),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (selector: (state: typeof mockLibraryState) => unknown) =>
    selector(mockLibraryState),
}));

const mockCatalogState = vi.hoisted(() => ({
  videoItems: {},
  sources: [
    {
      source_id: "alpha",
      owner_binding: "test/alpha",
      name: "Alpha",
      description: "Fixture source",
      license: "MIT",
      installed_version: "1",
      available_version: "1",
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
        source_id: "alpha",
        package_id: "alpha",
        publisher: "test",
        version: "1",
        minimum_host_version: "0.1.0",
        protocol: { majors: [1], minor: 0 },
        runtime: { kind: "node", major: 24 },
        entry: "index.js",
        name: "Alpha",
        description: "Fixture source",
        license: "MIT",
        capabilities: ["import"],
        authentication: { flow: "none" },
        requires_authentication: [],
        resource_origins: [],
        media_origins: [],
      },
    } satisfies SourceSnapshot,
  ],
  sourceStates: {},
  previewItems: {} as Record<
    string,
    {
      id: string;
      source_id: string;
      remote_track_id: string;
      title: string;
      artist: string;
      album: string | null;
      duration_ms: number;
      cover_url: string | null;
    }
  >,
  importTracks: vi.fn(),
}));

vi.mock("@/stores/catalog-store", async (original) => ({
  ...(await original<{
    sourceAllows: (source: SourceSnapshot, capability: string) => boolean;
  }>()),
  useCatalogStore: (selector: (state: typeof mockCatalogState) => unknown) =>
    selector(mockCatalogState),
}));

describe("NowPlayingInfo", () => {
  test("renders cover art alongside the current song metadata", () => {
    vi.stubGlobal("URL", {
      createObjectURL: vi.fn(() => "blob:cover"),
      revokeObjectURL: vi.fn(),
    });

    const markup = renderToStaticMarkup(<NowPlayingInfo />);

    expect(markup).toContain("<img");
    expect(markup).toContain('src="blob:cover"');
    expect(markup).toContain(">22<");
    expect(markup).toContain(">Taylor Swift<");
    expect(markup).not.toContain('loading="lazy"');
    expect(markup).not.toContain('decoding="async"');

    vi.unstubAllGlobals();
  });

  test("renders cover art when bytes arrive as ArrayBuffer", () => {
    vi.stubGlobal("URL", {
      createObjectURL: vi.fn(() => "blob:cover"),
      revokeObjectURL: vi.fn(),
    });

    mockLibraryState.songs[0].cover_art = new Uint8Array([
      0xff, 0xd8, 0x00,
    ]).buffer;

    const markup = renderToStaticMarkup(<NowPlayingInfo />);

    expect(markup).toContain("<img");
    expect(markup).toContain('src="blob:cover"');
    expect(markup).not.toContain('loading="lazy"');
    expect(markup).not.toContain('decoding="async"');

    vi.unstubAllGlobals();
  });

  test("hides the artist metadata in the tight density", () => {
    vi.stubGlobal("URL", {
      createObjectURL: vi.fn(() => "blob:cover"),
      revokeObjectURL: vi.fn(),
    });

    const markup = renderToStaticMarkup(<NowPlayingInfo density="tight" />);

    expect(markup).toContain(">22<");
    expect(markup).not.toContain(">Taylor Swift<");

    vi.unstubAllGlobals();
  });

  test("exposes the unified now playing density markers", () => {
    const markup = renderToStaticMarkup(<NowPlayingInfo />);

    expect(markup).toContain('data-now-playing-visual-variant="unified"');
  });

  test("shows a streamed preview's metadata with an import affordance", () => {
    mockPlayerState.snapshot.song_id = "stream:alpha:99";
    mockCatalogState.previewItems["stream:alpha:99"] = {
      id: "stream:alpha:99",
      remote_track_id: "99",
      source_id: "alpha",
      title: "Streamed Song",
      artist: "Remote Artist",
      album: null,
      duration_ms: 1000,
      cover_url: null,
    };

    const markup = renderToStaticMarkup(<NowPlayingInfo />);

    expect(markup).toContain("Streamed Song");
    expect(markup).toContain("Remote Artist");
    expect(markup).not.toContain("catalog.streaming.previewBadge");
    expect(markup).toContain('aria-label="catalog.streaming.importToSing"');

    const tight = renderToStaticMarkup(<NowPlayingInfo density="tight" />);
    expect(tight).not.toContain("catalog.streaming.importToSing");

    mockPlayerState.snapshot.song_id = "song-1";
    delete mockCatalogState.previewItems["stream:alpha:99"];
  });
});
