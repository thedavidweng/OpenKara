// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createRef, useState } from "react";
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
import { useCatalogStore } from "@/stores/catalog-store";
import { useNotificationStore } from "@/stores/notification-store";
import { usePlayerStore } from "@/stores/player-store";
import { useQueueStore } from "@/stores/queue-store";
import type { SourceSnapshot, StreamingTrack } from "@/types/ipc";
import { StreamingTrackList } from "./StreamingTrackList";

vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key}|${JSON.stringify(options)}` : key,
  }),
}));

// jsdom has no layout; give scroll containers a viewport so rows render.
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

const track = (id: string, overrides: Partial<StreamingTrack> = {}) => ({
  source_id: "alpha" as const,
  remote_track_id: id,
  title: `Song ${id}`,
  artist: "Artist",
  album: "Album",
  duration_ms: 61000,
  refusal: null,
  library_song_id: null,
  cover_url: null,
  ...overrides,
});

const importTracks = vi.fn();
const playNow = vi.fn();
const setQueue = vi.fn();

beforeEach(() => {
  importTracks.mockReset();
  playNow.mockReset();
  setQueue.mockReset();
  useNotificationStore.setState({ notifications: [] });
  usePlayerStore.setState({ snapshot: null, playNow });
  useQueueStore.setState({ setQueue });
  useCatalogStore.setState({
    previewItems: {},
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
          capabilities: ["preview", "import"],
          authentication: { flow: "none" },
          requires_authentication: [],
          resource_origins: [],
          media_origins: [],
        },
      } satisfies SourceSnapshot,
    ],
    sourceStates: {},
  });
  useCatalogStore.getState().setActiveSource("alpha");
  useCatalogStore.setState({
    trackStates: {},
    hideUnavailable: true,
    importRun: null,
    importTracks,
  });
});

afterEach(cleanup);

function Harness({
  tracks,
  selecting = false,
  loadingMore,
  onSelect,
}: {
  tracks: StreamingTrack[];
  selecting?: boolean;
  loadingMore?: boolean;
  onSelect?: (ids: Set<string>) => void;
}) {
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  return (
    <div ref={scrollHost}>
      <StreamingTrackList
        tracks={tracks}
        scrollRef={scrollRef}
        loadingMore={loadingMore}
        selection={
          selecting
            ? {
                selected,
                onChange: (next) => {
                  setSelected(next);
                  onSelect?.(next);
                },
              }
            : undefined
        }
      />
    </div>
  );
}
const scrollRef = createRef<HTMLElement>();
const scrollHost = (node: HTMLDivElement | null) => {
  (scrollRef as { current: HTMLElement | null }).current = node;
};

describe("StreamingTrackList", () => {
  test("shows a loading-more status while pages are still arriving", () => {
    const { rerender } = render(<Harness tracks={[track("1")]} loadingMore />);
    expect(screen.getByText("catalog.streaming.loadingMore")).toBeTruthy();
    rerender(<Harness tracks={[track("1")]} />);
    expect(screen.queryByText("catalog.streaming.loadingMore")).toBeNull();
  });

  test("shows duration, no status chips and no header row", () => {
    render(<Harness tracks={[track("1")]} />);
    expect(screen.getByText("1:01")).toBeTruthy();
    expect(screen.queryByRole("columnheader")).toBeNull();
  });

  test("shows a ring with the download percent for an in-flight row", () => {
    useCatalogStore.setState({
      trackStates: { "stream:alpha:1": { kind: "downloading", percent: 62 } },
    });
    render(<Harness tracks={[track("1")]} />);
    const ring = screen.getByRole("progressbar");
    expect(ring.getAttribute("aria-valuenow")).toBe("62");
    expect(ring.getAttribute("aria-label")).toBe(
      'catalog.streaming.statusDownloading|{"percent":62}',
    );
  });

  test("an in-flight row without a percent is an indeterminate spinner", () => {
    useCatalogStore.setState({
      trackStates: { "stream:alpha:1": { kind: "downloading" } },
    });
    render(<Harness tracks={[track("1")]} />);
    expect(
      screen.getByRole("progressbar").getAttribute("aria-valuenow"),
    ).toBeNull();
  });

  test("imported rows show a check and failed rows offer a retry", () => {
    useCatalogStore.setState({
      trackStates: {
        "stream:alpha:2": { kind: "failed", reason: "import_failed" },
      },
    });
    render(
      <Harness tracks={[track("1", { library_song_id: "s1" }), track("2")]} />,
    );
    expect(
      screen.getByRole("img", { name: "catalog.streaming.statusImported" }),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", {
        name: 'catalog.streaming.retryImportLabel|{"title":"Song 2"}',
      }),
    );
    expect(importTracks).toHaveBeenCalledWith(["2"]);
  });

  test("a download button imports one track", () => {
    render(<Harness tracks={[track("1")]} />);
    fireEvent.click(
      screen.getByRole("button", {
        name: 'catalog.streaming.importTrackLabel|{"title":"Song 1"}',
      }),
    );
    expect(importTracks).toHaveBeenCalledWith(["1"]);
  });

  test("unavailable rows are dimmed and carry the reason when shown", () => {
    useCatalogStore.setState({ hideUnavailable: false });
    render(
      <Harness
        tracks={[
          track("1", {
            refusal: { reason: "no_play_rights", title: "x", artist: "y" },
          }),
        ]}
      />,
    );
    const row = screen.getByTestId("streaming-track-row");
    expect(row.getAttribute("data-unavailable")).toBe("true");
    expect(
      screen.getByText("catalog.streaming.statusNoRightsHint"),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: /importTrackLabel/ }),
    ).toBeNull();
  });

  test("the toggle reports the hidden count and flips the list", () => {
    const refusal = {
      reason: "no_play_rights" as const,
      title: "x",
      artist: "y",
    };
    render(<Harness tracks={[track("1"), track("2", { refusal })]} />);
    expect(screen.getAllByTestId("streaming-track-row")).toHaveLength(1);
    expect(
      screen.getByText('catalog.streaming.unavailableHidden|{"hidden":1}'),
    ).toBeTruthy();
    const toggle = screen.getByRole("switch");
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(screen.getAllByTestId("streaming-track-row")).toHaveLength(2);
  });

  test("selection mode supports select all and shift-range", () => {
    const onSelect = vi.fn();
    render(
      <Harness
        selecting
        onSelect={onSelect}
        tracks={[track("1"), track("2"), track("3"), track("4")]}
      />,
    );
    const grid = screen.getByRole("grid", {
      name: "catalog.streaming.trackListLabel",
    });
    expect(grid.getAttribute("aria-multiselectable")).toBe("true");
    const boxes = screen.getAllByRole("checkbox");
    // boxes[0] is select-all
    fireEvent.click(boxes[1]);
    fireEvent.click(boxes[3], { shiftKey: true });
    expect([...onSelect.mock.lastCall![0]].sort()).toEqual(["1", "2", "3"]);
    expect(
      screen
        .getAllByRole("row")
        .slice(0, 3)
        .map((row) => row.getAttribute("aria-selected")),
    ).toEqual(["true", "true", "true"]);

    fireEvent.click(boxes[0]);
    expect([...onSelect.mock.lastCall![0]].sort()).toEqual([
      "1",
      "2",
      "3",
      "4",
    ]);
  });

  test("double-click plays a preview and queues the playable rest", () => {
    const refusal = {
      reason: "trial_clip" as const,
      title: "x",
      artist: "y",
    };
    render(
      <Harness
        tracks={[
          track("1"),
          track("2", { refusal }),
          track("3", { library_song_id: "lib3" }),
          track("4"),
        ]}
      />,
    );
    // Unavailable tracks are hidden by default; show them to prove they are
    // skipped when queueing.
    fireEvent.click(screen.getByRole("switch"));
    fireEvent.doubleClick(screen.getAllByTestId("streaming-track-row")[0]!);
    expect(playNow).toHaveBeenCalledWith("stream:alpha:1");
    expect(setQueue).toHaveBeenCalledWith(["lib3", "stream:alpha:4"]);
    expect(
      useCatalogStore.getState().previewItems["stream:alpha:1"]?.title,
    ).toBe("Song 1");
  });

  test("Enter on a focused row plays it", () => {
    render(<Harness tracks={[track("1"), track("2")]} />);
    const rows = screen.getAllByRole("row");
    fireEvent.keyDown(rows[1]!, { key: "Enter" });
    expect(playNow).toHaveBeenCalledWith("stream:alpha:2");
    expect(setQueue).toHaveBeenCalledWith([]);
    expect(screen.getAllByRole("gridcell")).toHaveLength(4);
    const importButton = screen.getByRole("button", {
      name: 'catalog.streaming.importTrackLabel|{"title":"Song 2"}',
    });
    expect(importButton.closest('[role="gridcell"]')).not.toBeNull();
    fireEvent.click(importButton);
    expect(importTracks).toHaveBeenCalledWith(["2"]);
  });

  test("an unavailable track explains itself instead of playing", () => {
    const refusal = {
      reason: "trial_clip" as const,
      title: "x",
      artist: "y",
    };
    useCatalogStore.setState({ hideUnavailable: false });
    render(<Harness tracks={[track("1", { refusal })]} />);
    fireEvent.doubleClick(screen.getByTestId("streaming-track-row"));
    expect(playNow).not.toHaveBeenCalled();
    const [note] = useNotificationStore.getState().notifications;
    expect(note?.message).toBe("catalog.streaming.refusalReason.trial_clip");
  });

  test("the playing row is marked current", () => {
    usePlayerStore.setState({
      snapshot: { song_id: "stream:alpha:2" } as never,
    });
    render(<Harness tracks={[track("1"), track("2")]} />);
    const rows = screen.getAllByTestId("streaming-track-row");
    expect(rows[0]!.getAttribute("aria-current")).toBeNull();
    expect(rows[1]!.getAttribute("aria-current")).toBe("true");
  });
});
