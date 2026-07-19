// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test, vi, beforeEach } from "vitest";
import { PlaybackBar } from "./PlaybackBar";

const { mockPlayerState } = vi.hoisted(() => ({
  mockPlayerState: {
    snapshot: {
      volume: 0.72,
    },
    setVolume: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      (
        ({
          "player.volume": "Volume",
          "player.mute": "Mute",
          "player.unmute": "Unmute",
        }) as const
      )[key] ?? key,
  }),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: (selector: (state: typeof mockPlayerState) => unknown) =>
    selector(mockPlayerState),
}));

vi.mock("@/components/Overlay/Tooltip", () => ({
  Tooltip: ({
    children,
    label,
  }: {
    children: React.ReactNode;
    label: string;
  }) => <span data-tooltip-label={label}>{children}</span>,
}));

vi.mock("./NowPlayingInfo", () => ({
  NowPlayingInfo: ({ density }: { density?: string }) => (
    <div data-now-playing-density={density}>Now playing</div>
  ),
}));

vi.mock("./PlayControls", () => ({
  PlayControls: ({ density }: { density?: string }) => (
    <div data-play-controls-density={density}>Play controls</div>
  ),
}));

vi.mock("./SeekBar", () => ({
  SeekBar: ({ density }: { density?: string }) => (
    <div data-seek-bar-density={density}>Seek bar</div>
  ),
}));

vi.mock("./VolumeSliders", () => ({
  VolumeSliders: ({ density }: { density?: string }) => (
    <div data-volume-sliders-density={density}>Stem sliders</div>
  ),
}));

vi.mock("./QueueButton", () => ({
  QueueButton: () => <div>Queue button</div>,
}));

let measuredWidth = 1280;
let resizeObserverCallback: ResizeObserverCallback | null = null;

beforeEach(() => {
  measuredWidth = 1280;
  resizeObserverCallback = null;
  (
    globalThis as typeof globalThis & {
      IS_REACT_ACT_ENVIRONMENT?: boolean;
    }
  ).IS_REACT_ACT_ENVIRONMENT = true;

  Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
    configurable: true,
    value: () => ({
      width: measuredWidth,
      height: 80,
      top: 0,
      right: measuredWidth,
      bottom: 80,
      left: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    }),
  });

  class MockResizeObserver implements ResizeObserver {
    observe() {}

    unobserve() {}

    disconnect() {}

    takeRecords() {
      return [];
    }

    constructor(callback: ResizeObserverCallback) {
      resizeObserverCallback = callback;
    }
  }

  vi.stubGlobal("ResizeObserver", MockResizeObserver);
});

describe("PlaybackBar", () => {
  test("uses the shared audio level slider for master volume tooltip text", () => {
    const markup = renderToStaticMarkup(<PlaybackBar />);

    expect(markup).toContain('data-tooltip-label="Volume 72%"');
    expect(markup).toContain("audio-level-slider");
    expect(markup).not.toContain("title=");
  });

  test("renders the active master volume icon with the same control brightness as stem icons", () => {
    const markup = renderToStaticMarkup(<PlaybackBar />);

    expect(markup).toContain('data-playback-action="master-mute"');
    expect(markup).toContain('aria-label="Mute"');
    expect(markup).toContain("text-[var(--color-control-primary)]");
    expect(markup).not.toContain('data-active="true"');
  });

  test("renders the muted master volume button with dimmed icon and aria-pressed", () => {
    mockPlayerState.snapshot.volume = 0;
    const markup = renderToStaticMarkup(<PlaybackBar />);

    expect(markup).toContain('aria-pressed="true"');
    expect(markup).not.toContain('data-active="true"');
    expect(markup).toContain('aria-label="Unmute"');
    expect(markup).toContain("text-[var(--color-text-dimmer)]");
    mockPlayerState.snapshot.volume = 0.72;
  });

  test("forwards the tight density to the responsive children", () => {
    const markup = renderToStaticMarkup(
      <PlaybackBar densityOverride="tight" />,
    );

    expect(markup).toContain('data-playback-bar-density="tight"');
    expect(markup).toContain('data-playback-zone="left"');
    expect(markup).toContain('data-playback-zone="center"');
    expect(markup).toContain('data-playback-zone="right"');
    expect(markup).toContain('data-now-playing-density="tight"');
    expect(markup).toContain('data-play-controls-density="tight"');
    expect(markup).toContain('data-seek-bar-density="tight"');
    expect(markup).toContain('data-volume-sliders-density="tight"');
    expect(markup).toContain("Queue button");
    expect(markup).toContain("audio-level-slider shrink-0 w-[64px]");
  });

  test("keeps one shared structure for the flush playback bar chrome", () => {
    const markup = renderToStaticMarkup(
      <PlaybackBar densityOverride="relaxed" />,
    );

    expect(markup).toContain('data-playback-bar-visual-variant="unified"');
    expect(markup).toContain("bg-[var(--color-sidebar)]");
    expect(markup).not.toContain("mx-3");
    expect(markup).not.toContain("mb-3");
    expect(markup).toContain('data-playback-zone="left"');
    expect(markup).toContain('data-playback-zone="center"');
    expect(markup).toContain('data-playback-zone="right"');
  });

  test("master volume uses the token-provided width class at every density", () => {
    const relaxed = renderToStaticMarkup(
      <PlaybackBar densityOverride="relaxed" />,
    );
    expect(relaxed).toContain("audio-level-slider shrink-0 w-[104px]");

    const compact = renderToStaticMarkup(
      <PlaybackBar densityOverride="compact" />,
    );
    expect(compact).toContain("audio-level-slider shrink-0 w-[80px]");

    const tight = renderToStaticMarkup(<PlaybackBar densityOverride="tight" />);
    expect(tight).toContain("audio-level-slider shrink-0 w-[64px]");
  });

  test("master icon-to-rail gap uses the token-provided value at every density", () => {
    // The master mute icon + slider group uses an inline style gap from
    // layoutTokens.masterVolumeGap. We assert the rendered style attribute.
    const relaxed = renderToStaticMarkup(
      <PlaybackBar densityOverride="relaxed" />,
    );
    expect(relaxed).toContain("gap:8px");

    const compact = renderToStaticMarkup(
      <PlaybackBar densityOverride="compact" />,
    );
    expect(compact).toContain("gap:6px");

    const tight = renderToStaticMarkup(<PlaybackBar densityOverride="tight" />);
    expect(tight).toContain("gap:6px");
  });

  test("measures the container width and updates the density through ResizeObserver", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<PlaybackBar />);
    });

    expect(container.innerHTML).toContain(
      'data-playback-bar-density="relaxed"',
    );

    measuredWidth = 900;

    await act(async () => {
      resizeObserverCallback?.(
        [] as ResizeObserverEntry[],
        {} as ResizeObserver,
      );
    });

    expect(container.innerHTML).toContain('data-playback-bar-density="tight"');

    await act(async () => {
      root.unmount();
    });

    container.remove();
  });

  test("collapses the now playing zone at narrow widths instead of letting controls overlap", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<PlaybackBar />);
    });

    measuredWidth = 720;

    await act(async () => {
      resizeObserverCallback?.(
        [] as ResizeObserverEntry[],
        {} as ResizeObserver,
      );
    });

    expect(container.innerHTML).not.toContain('data-playback-zone="left"');
    expect(container.innerHTML).not.toContain("Now playing");
    expect(container.innerHTML).toContain('data-playback-zone="center"');
    expect(container.innerHTML).toContain('data-playback-zone="right"');
    expect(container.innerHTML).toContain("Queue button");
    expect(container.innerHTML).toContain(
      "audio-level-slider shrink-0 w-[64px]",
    );

    await act(async () => {
      root.unmount();
    });

    container.remove();
  });
});
