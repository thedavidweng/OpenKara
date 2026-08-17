import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { QueuePanel } from "./QueuePanel";

const { mockQueueState, mockLibraryState, mockRotationState } = vi.hoisted(
  () => ({
    mockQueueState: {
      queue: [] as string[],
      removeFromQueue: vi.fn(),
      reorder: vi.fn(),
      reorderBySongId: vi.fn(),
      clearQueue: vi.fn(),
    },
    mockLibraryState: {
      songs: [] as Array<{
        hash: string;
        title: string;
        artist: string;
        [key: string]: unknown;
      }>,
    },
    mockRotationState: {
      active: false,
      singerNames: [] as string[],
      queueSingers: new Map<string, string | null>(),
      filterSinger: null as string | null,
      assignSingerToQueueEntry: vi.fn(),
    },
  }),
);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, string | number>) =>
      vars?.title ? `${key}:${vars.title}` : key,
  }),
}));

vi.mock("@/stores/queue-store", () => ({
  useQueueStore: (selector: (state: typeof mockQueueState) => unknown) =>
    selector(mockQueueState),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (selector: (state: typeof mockLibraryState) => unknown) =>
    selector(mockLibraryState),
}));

vi.mock("@/stores/rotation-store", () => ({
  useRotationStore: (selector: (state: typeof mockRotationState) => unknown) =>
    selector(mockRotationState),
}));

vi.mock("@/components/Overlay/Tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock("./RotationControls", () => ({
  RotationControls: () => <div data-testid="rotation-controls" />,
}));

vi.mock("./SingerPickerDialog", () => ({
  SingerPickerDialog: () => <div data-testid="singer-picker-dialog" />,
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (
    selector: (state: { youtubeSourceEnabled: boolean }) => unknown,
  ) => selector({ youtubeSourceEnabled: false }),
}));

vi.mock("@/stores/catalog-store", () => ({
  useCatalogStore: (
    selector: (state: { videoItems: Record<string, never> }) => unknown,
  ) => selector({ videoItems: {} }),
}));

vi.mock("@/components/Catalog/YoutubePasteLink", () => ({
  YoutubePasteLink: () => null,
}));

describe("QueuePanel", () => {
  beforeEach(() => {
    mockQueueState.queue = [];
    mockLibraryState.songs = [];
    mockRotationState.active = false;
    mockRotationState.singerNames = [];
    mockRotationState.queueSingers = new Map();
    mockRotationState.filterSinger = null;
  });

  test("renders the empty state when the queue is empty", () => {
    const markup = renderToStaticMarkup(<QueuePanel />);

    expect(markup).toContain("queue.empty");
    expect(markup).not.toContain("queue.clearAll");
  });

  test("renders the queue header with item count and clear button when populated", () => {
    mockQueueState.queue = ["song-a", "song-b"];
    mockLibraryState.songs = [
      {
        hash: "song-a",
        title: "Alpha Song",
        artist: "Artist A",
        file_path: "/a.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
      {
        hash: "song-b",
        title: "Beta Song",
        artist: "Artist B",
        file_path: "/b.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        album: null,
        duration_ms: 180000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ];

    const markup = renderToStaticMarkup(<QueuePanel />);

    expect(markup).toContain("queue.upNext");
    expect(markup).toContain("queue.clearAll");
    expect(markup).toContain("Alpha Song");
    expect(markup).toContain("Beta Song");
  });

  test("displays singer names next to queue items when rotation is active", () => {
    mockQueueState.queue = ["song-a"];
    mockLibraryState.songs = [
      {
        hash: "song-a",
        title: "My Song",
        artist: "My Artist",
        file_path: "/a.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ];
    mockRotationState.active = true;
    mockRotationState.queueSingers = new Map([["song-a", "David"]]);

    const markup = renderToStaticMarkup(<QueuePanel />);

    expect(markup).toContain("David");
  });

  test("hides singer assignment when rotation is inactive", () => {
    mockQueueState.queue = ["song-a"];
    mockLibraryState.songs = [
      {
        hash: "song-a",
        title: "My Song",
        artist: "My Artist",
        file_path: "/a.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ];
    const markup = renderToStaticMarkup(<QueuePanel />);

    expect(markup).not.toContain("rotation.assignSinger");
  });

  test("falls back to a truncated hash when a song is missing from the library", () => {
    mockQueueState.queue = ["abcd1234efgh5678"];
    mockLibraryState.songs = [];

    const markup = renderToStaticMarkup(<QueuePanel />);

    expect(markup).toContain("abcd1234");
    expect(markup).toContain("common.unknownArtist");
  });

  test("displays reordering controls with move up and move down buttons", () => {
    mockQueueState.queue = ["song-a", "song-b"];
    mockLibraryState.songs = [
      {
        hash: "song-a",
        title: "First",
        artist: "Artist",
        file_path: "/a.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
      {
        hash: "song-b",
        title: "Second",
        artist: "Artist",
        file_path: "/b.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ];

    const markup = renderToStaticMarkup(<QueuePanel />);

    expect(markup).toContain("queue.moveUp");
    expect(markup).toContain("queue.moveDown");
    expect(markup).toContain("queue.reorder:First");
    expect(markup).toContain("queue.remove:First");
  });

  test("renders filtered queue count when a singer filter is active", () => {
    mockQueueState.queue = ["song-a", "song-b", "song-c"];
    mockLibraryState.songs = [
      {
        hash: "song-a",
        title: "Alpha",
        artist: "A",
        file_path: "/a.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
      {
        hash: "song-b",
        title: "Beta",
        artist: "B",
        file_path: "/b.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
      {
        hash: "song-c",
        title: "Gamma",
        artist: "C",
        file_path: "/c.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ];
    mockRotationState.active = true;
    mockRotationState.filterSinger = "David";
    mockRotationState.queueSingers = new Map([
      ["song-a", "David"],
      ["song-b", "Leo"],
      ["song-c", "David"],
    ]);

    const markup = renderToStaticMarkup(<QueuePanel />);

    expect(markup).toContain("2/3");
    expect(markup).toContain("Alpha");
    expect(markup).not.toContain("Beta");
    expect(markup).toContain("Gamma");
  });

  test("shadow panel uses theme border token", () => {
    const markup = renderToStaticMarkup(<QueuePanel />);
    expect(markup).toContain("shadow-[-1px_0_0_var(--color-border)]");
  });
});
