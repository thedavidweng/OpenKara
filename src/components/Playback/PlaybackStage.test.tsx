// @vitest-environment jsdom
import { renderToStaticMarkup } from "react-dom/server";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, test, vi } from "vitest";
import { PlaybackStage } from "./PlaybackStage";
import type { Song } from "@/types/ipc";

const {
  mockCdgState,
  mockPlayerState,
  mockLibraryState,
  mockGetCoverArtPreview,
} = vi.hoisted(() => ({
  mockCdgState: { hasCdg: false },
  mockPlayerState: {
    snapshot: {
      song_id: "song-1" as string | null,
    },
  },
  mockLibraryState: {
    songs: [
      {
        hash: "song-1",
        file_path: "media-g/song-1.mp3",
        audio_source_kind: "original",
        cdg_path: "media-g/song-1.cdg",
        media_g_container: "paired" as const,
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
  },
  mockGetCoverArtPreview: vi.fn<(hash: string) => Promise<number[] | null>>(),
}));

vi.mock("@/stores/cdg-store", () => ({
  useCdgStore: (selector: (state: typeof mockCdgState) => unknown) =>
    selector(mockCdgState),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: (selector: (state: typeof mockPlayerState) => unknown) =>
    selector(mockPlayerState),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (selector: (state: typeof mockLibraryState) => unknown) =>
    selector(mockLibraryState),
}));

vi.mock("@/lib/cover-art", () => ({
  useCoverArtUrl: (_songHash: string, bytes: unknown) =>
    bytes ? "blob:stage-cover" : null,
}));

vi.mock("@/components/Cdg/CdgCanvas", () => ({
  CdgCanvas: () => <div data-testid="cdg-canvas">CDG</div>,
}));

vi.mock("@/components/Lyrics/LyricsPanel", () => ({
  LyricsPanel: () => (
    <div data-testid="lyrics-panel">
      <div data-testid="lyrics-scroll-viewport">Lyrics</div>
    </div>
  ),
}));

vi.mock("@/lib/tauri/library", () => ({
  getCoverArtPreview: mockGetCoverArtPreview,
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (selector: (s: { coverArtBackdrop: boolean }) => unknown) =>
    selector({ coverArtBackdrop: true }),
}));

afterEach(() => {
  mockGetCoverArtPreview.mockReset();
});

describe("PlaybackStage", () => {
  test("renders the CDG canvas when the current song metadata has CDG media", () => {
    const markup = renderToStaticMarkup(<PlaybackStage />);

    expect(markup).toContain("flex h-full w-full flex-1 overflow-hidden");
    expect(markup).toContain("cdg-canvas");
    expect(markup).not.toContain("lyrics-panel");
  });

  test("audience stages reserve no bottom band so lyrics own the whole screen", () => {
    const markup = renderToStaticMarkup(
      <PlaybackStage presentation="audience" />,
    );

    expect(markup).not.toContain("padding-bottom");
  });

  test("renders a cover-art ambience backdrop for standard lyric stages without CDG", () => {
    mockCdgState.hasCdg = false;
    mockLibraryState.songs = [
      {
        hash: "song-2",
        file_path: "Fuji Kaze/Hachiko.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        title: "Hachiko",
        artist: "Fuji Kaze",
        album: null,
        duration_ms: 270000,
        cover_art: [0xff, 0xd8, 0x00],
        has_cover_art: true,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ] as Song[];
    mockPlayerState.snapshot = { song_id: "song-2" };

    const markup = renderToStaticMarkup(<PlaybackStage />);

    expect(markup).toContain('data-stage-visual-variant="ambience"');
    expect(markup).toContain('data-native-stage-backdrop="true"');
    expect(markup).toContain("blob:stage-cover");
    expect(markup).toContain("lyrics-panel");
  });

  test("keeps an empty lyric stage bright when no cover art is available", () => {
    mockCdgState.hasCdg = false;
    mockLibraryState.songs = [];
    mockPlayerState.snapshot = { song_id: null };

    const markup = renderToStaticMarkup(<PlaybackStage />);

    expect(markup).toContain('data-stage-visual-variant="default"');
    expect(markup).not.toContain('data-native-stage-backdrop="true"');
    expect(markup).toContain("lyrics-panel");
  });

  test("clears stale fetched cover art and fetches on mount when cover_art is absent", async () => {
    mockCdgState.hasCdg = false;
    mockLibraryState.songs = [
      {
        hash: "song-fetch",
        file_path: "song.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        title: "Fetch",
        artist: null,
        album: null,
        duration_ms: 1000,
        cover_art: null,
        has_cover_art: true,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ] as Song[];
    mockPlayerState.snapshot = { song_id: "song-fetch" };
    mockGetCoverArtPreview.mockResolvedValue([0xff, 0xd8, 0x00]);

    const container = document.createElement("div");
    const root = createRoot(container);
    await act(async () => {
      root.render(<PlaybackStage />);
    });

    // The effect clears stale bytes then fetches the preview derivative.
    expect(mockGetCoverArtPreview).toHaveBeenCalledWith("song-fetch");

    await act(async () => {
      root.unmount();
    });
  });

  test("keeps the lyrics scroll viewport mounted when the ambience backdrop resolves mid-song (#200)", async () => {
    mockCdgState.hasCdg = false;
    mockLibraryState.songs = [
      {
        hash: "song-async-cover",
        file_path: "song.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        title: "Async Cover",
        artist: null,
        album: null,
        duration_ms: 1000,
        // No inlined cover_art: the backdrop is fetched on-demand and flips
        // stageAmbience false→true a second into playback.
        cover_art: null,
        has_cover_art: true,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ] as Song[];
    mockPlayerState.snapshot = { song_id: "song-async-cover" };

    let resolveFetch: (bytes: number[]) => void = () => {};
    mockGetCoverArtPreview.mockReturnValue(
      new Promise<number[]>((resolve) => {
        resolveFetch = resolve;
      }),
    );

    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    await act(async () => {
      root.render(<PlaybackStage />);
    });

    // Before the backdrop resolves the stage is the plain (non-ambience) lyric
    // stage, and the scroll viewport is mounted.
    const stage = container.querySelector("[data-stage-visual-variant]");
    expect(stage?.getAttribute("data-stage-visual-variant")).toBe("default");
    const viewportBefore = container.querySelector(
      "[data-testid='lyrics-scroll-viewport']",
    ) as HTMLDivElement;
    expect(viewportBefore).toBeTruthy();
    // Simulate mid-song follow position: the user is a few lines in.
    viewportBefore.scrollTop = 240;

    // The async cover-art preview lands mid-playback → ambience flips on.
    await act(async () => {
      resolveFetch([0xff, 0xd8, 0x00]);
      await Promise.resolve();
    });

    const stageAfter = container.querySelector("[data-stage-visual-variant]");
    expect(stageAfter?.getAttribute("data-stage-visual-variant")).toBe(
      "ambience",
    );
    expect(
      container.querySelector("[data-native-stage-backdrop='true']"),
    ).toBeTruthy();

    const viewportAfter = container.querySelector(
      "[data-testid='lyrics-scroll-viewport']",
    ) as HTMLDivElement;
    // Same DOM node instance: the panel was not remounted, so the line-runtime
    // springs are intact (no gather replay) and scrollTop is preserved.
    expect(viewportAfter).toBe(viewportBefore);
    expect(viewportAfter.scrollTop).toBe(240);

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });
});
