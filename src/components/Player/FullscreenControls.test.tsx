// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { FullscreenControls } from "./FullscreenControls";

const { mockUseMouseIdle, mockCloseFullscreenPlayer, mockPlayerStore } =
  vi.hoisted(() => ({
    mockUseMouseIdle: vi.fn(() => true),
    mockCloseFullscreenPlayer: vi.fn(),
    mockPlayerStore: {
      snapshot: null as {
        song_id: string;
        is_playing: boolean;
        volume: number;
      } | null,
      positionMs: 0,
      playingSinceMs: null as number | null,
      pause: vi.fn(() => Promise.resolve()),
      resume: vi.fn(() => Promise.resolve()),
      seek: vi.fn(() => Promise.resolve()),
      setVolume: vi.fn(() => Promise.resolve()),
    },
  }));

vi.mock("./PlayControls", () => ({
  PlayControls: () => <div>Play controls</div>,
}));

vi.mock("./SeekBar", () => ({
  SeekBar: () => <div>Seek bar</div>,
}));

vi.mock("@/hooks/use-mouse-idle", () => ({
  useMouseIdle: mockUseMouseIdle,
}));

vi.mock("@/lib/fullscreen-player", () => ({
  closeFullscreenPlayer: mockCloseFullscreenPlayer,
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: {
    getState: () => mockPlayerStore,
  },
  selectCurrentPositionMs: () => mockPlayerStore.positionMs,
}));

describe("FullscreenControls", () => {
  test("hides playback bar when cursor is idle", () => {
    mockUseMouseIdle.mockReturnValue(true);

    const markup = renderToStaticMarkup(<FullscreenControls />);

    expect(markup).toContain("pointer-events-none");
    expect(markup).toContain("opacity-0");
  });

  test("shows playback bar when cursor is active", () => {
    mockUseMouseIdle.mockReturnValue(false);

    const markup = renderToStaticMarkup(<FullscreenControls />);

    expect(markup).not.toContain("pointer-events-none");
    expect(markup).toContain("opacity-100");
  });
});

describe("FullscreenControls keyboard shortcuts", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    mockUseMouseIdle.mockReturnValue(false);
    mockCloseFullscreenPlayer.mockReset();
    mockPlayerStore.pause.mockReset();
    mockPlayerStore.resume.mockReset();
    mockPlayerStore.seek.mockReset();
    mockPlayerStore.setVolume.mockReset();
    mockPlayerStore.snapshot = {
      song_id: "song-1",
      is_playing: true,
      volume: 0.5,
    };
    mockPlayerStore.positionMs = 10000;
    mockPlayerStore.playingSinceMs = null;

    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  function dispatchKeyDown(code: string, key?: string) {
    act(() => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", {
          code,
          key: key ?? code,
          bubbles: true,
        }),
      );
    });
  }

  test("Escape closes the fullscreen player", async () => {
    await act(async () => {
      root.render(<FullscreenControls />);
    });
    dispatchKeyDown("Escape", "Escape");
    expect(mockCloseFullscreenPlayer).toHaveBeenCalledTimes(1);
  });

  test("Space pauses when playing", async () => {
    mockPlayerStore.snapshot = {
      song_id: "song-1",
      is_playing: true,
      volume: 0.5,
    };
    await act(async () => {
      root.render(<FullscreenControls />);
    });
    dispatchKeyDown("Space");
    expect(mockPlayerStore.pause).toHaveBeenCalledTimes(1);
    expect(mockPlayerStore.resume).not.toHaveBeenCalled();
  });

  test("Space resumes when paused with a song loaded", async () => {
    mockPlayerStore.snapshot = {
      song_id: "song-1",
      is_playing: false,
      volume: 0.5,
    };
    await act(async () => {
      root.render(<FullscreenControls />);
    });
    dispatchKeyDown("Space");
    expect(mockPlayerStore.resume).toHaveBeenCalledTimes(1);
    expect(mockPlayerStore.pause).not.toHaveBeenCalled();
  });

  test("Space does nothing without a song", async () => {
    mockPlayerStore.snapshot = null;
    await act(async () => {
      root.render(<FullscreenControls />);
    });
    dispatchKeyDown("Space");
    expect(mockPlayerStore.pause).not.toHaveBeenCalled();
    expect(mockPlayerStore.resume).not.toHaveBeenCalled();
  });

  test("ArrowLeft seeks backward 5 seconds", async () => {
    await act(async () => {
      root.render(<FullscreenControls />);
    });
    dispatchKeyDown("ArrowLeft");
    expect(mockPlayerStore.seek).toHaveBeenCalledWith(5000);
  });

  test("ArrowRight seeks forward 5 seconds", async () => {
    await act(async () => {
      root.render(<FullscreenControls />);
    });
    dispatchKeyDown("ArrowRight");
    expect(mockPlayerStore.seek).toHaveBeenCalledWith(15000);
  });

  test("ArrowUp increases volume by 0.05", async () => {
    await act(async () => {
      root.render(<FullscreenControls />);
    });
    dispatchKeyDown("ArrowUp");
    expect(mockPlayerStore.setVolume).toHaveBeenCalledWith(0.55);
  });

  test("ArrowDown decreases volume by 0.05", async () => {
    await act(async () => {
      root.render(<FullscreenControls />);
    });
    dispatchKeyDown("ArrowDown");
    expect(mockPlayerStore.setVolume).toHaveBeenCalledWith(0.45);
  });
});
