// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { FullscreenControls } from "./FullscreenControls";

const {
  mockCloseFullscreenPlayer,
  mockPlayerStore,
  mockEmitLocalAudienceRomanizeSetRequest,
  mockLyricsStore,
  mockT,
} = vi.hoisted(() => ({
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
  mockEmitLocalAudienceRomanizeSetRequest: vi.fn(),
  mockLyricsStore: {
    showRomanized: false,
    isRomanizing: false,
    songId: null as string | null,
    lines: [] as { time_ms: number; text: string }[],
    lyricsAlignment: "left" as "center" | "left",
    setRomanizedVisibility: (show: boolean) => {
      mockLyricsStore.showRomanized = show;
    },
    toggleLyricsAlignment: () => {
      mockLyricsStore.lyricsAlignment =
        mockLyricsStore.lyricsAlignment === "left" ? "center" : "left";
    },
    subscribe: vi.fn(),
    getState: () => ({
      showRomanized: mockLyricsStore.showRomanized,
      isRomanizing: mockLyricsStore.isRomanizing,
      songId: mockLyricsStore.songId,
      lines: mockLyricsStore.lines,
      lyricsAlignment: mockLyricsStore.lyricsAlignment,
      setRomanizedVisibility: mockLyricsStore.setRomanizedVisibility,
      toggleLyricsAlignment: mockLyricsStore.toggleLyricsAlignment,
    }),
  },
  mockT: vi.fn((key: string) => key),
}));

vi.mock("./PlayControls", () => ({
  PlayControls: () => <div>Play controls</div>,
}));

vi.mock("./SeekBar", () => ({
  SeekBar: () => <div>Seek bar</div>,
}));

vi.mock("./PeakMeter", () => ({
  PeakMeter: () => <div>Peak meter</div>,
}));

vi.mock("@/lib/fullscreen-player", () => ({
  closeFullscreenPlayer: mockCloseFullscreenPlayer,
}));

vi.mock("@/lib/local-audience-romanize", () => ({
  emitLocalAudienceRomanizeSetRequest: mockEmitLocalAudienceRomanizeSetRequest,
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: {
    getState: () => mockPlayerStore,
  },
  selectCurrentPositionMs: () => mockPlayerStore.positionMs,
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: (selector: (state: typeof mockLyricsStore) => unknown) =>
    selector(mockLyricsStore),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: mockT }),
}));

describe("FullscreenControls", () => {
  test("renders the playback bar with controls", () => {
    const markup = renderToStaticMarkup(<FullscreenControls />);

    expect(markup).toContain("Play controls");
    expect(markup).toContain("Seek bar");
    expect(markup).toContain("Peak meter");
  });
});

describe("FullscreenControls keyboard shortcuts", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
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

describe("FullscreenControls Romanize button", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    mockEmitLocalAudienceRomanizeSetRequest.mockReset();
    mockEmitLocalAudienceRomanizeSetRequest.mockResolvedValue(undefined);
    mockLyricsStore.showRomanized = false;
    mockLyricsStore.isRomanizing = false;
    mockLyricsStore.songId = "song-1";
    mockLyricsStore.lines = [{ time_ms: 0, text: "你好" }];
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  function getRomanizeButton(): HTMLButtonElement {
    const button = container.querySelector<HTMLButtonElement>(
      '[data-testid="fullscreen-romanize-button"]',
    );
    if (!button) throw new Error("romanize button not rendered");
    return button;
  }

  test("reflects the authoritative selected state", async () => {
    mockLyricsStore.showRomanized = true;
    await act(async () => {
      root.render(<FullscreenControls />);
    });

    const button = getRomanizeButton();
    expect(button.getAttribute("aria-pressed")).toBe("true");
    expect(button.className).toContain("var(--color-accent)");
  });

  test("reflects the authoritative loading state with a spinner and disabled attribute", async () => {
    mockLyricsStore.isRomanizing = true;
    await act(async () => {
      root.render(<FullscreenControls />);
    });

    const button = getRomanizeButton();
    expect(button.disabled).toBe(true);
    expect(button.querySelector("svg.animate-spin")).not.toBeNull();
  });

  test("is disabled when no lyrics are available", async () => {
    mockLyricsStore.lines = [];
    mockLyricsStore.songId = null;
    await act(async () => {
      root.render(<FullscreenControls />);
    });

    expect(getRomanizeButton().disabled).toBe(true);
  });

  test("clicking sends the explicit desired boolean to the main window", async () => {
    mockLyricsStore.showRomanized = false;
    await act(async () => {
      root.render(<FullscreenControls />);
    });

    await act(async () => {
      getRomanizeButton().click();
    });

    expect(mockEmitLocalAudienceRomanizeSetRequest).toHaveBeenCalledWith({
      songId: "song-1",
      showRomanized: true,
    });
  });

  test("clicking immediately updates the fullscreen projection", async () => {
    mockLyricsStore.showRomanized = false;
    await act(async () => {
      root.render(<FullscreenControls />);
    });

    await act(async () => {
      getRomanizeButton().click();
    });

    expect(mockLyricsStore.showRomanized).toBe(true);
  });
});

describe("FullscreenControls auto-hide", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    vi.useFakeTimers();
    mockLyricsStore.showRomanized = false;
    mockLyricsStore.isRomanizing = false;
    mockLyricsStore.songId = "song-1";
    mockLyricsStore.lines = [{ time_ms: 0, text: "hello" }];
    mockPlayerStore.snapshot = {
      song_id: "song-1",
      is_playing: true,
      volume: 0.5,
    };
    document.body.style.cursor = "";
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.useRealTimers();
  });

  function getFooter(): HTMLElement {
    return container.firstElementChild as HTMLElement;
  }

  test("hides once the pointer has been still for the idle window, and wakes on movement", () => {
    act(() => {
      root.render(<FullscreenControls />);
    });

    const footer = getFooter();
    expect(footer.getAttribute("data-idle")).toBe("false");

    act(() => {
      window.dispatchEvent(new Event("pointermove"));
    });
    expect(footer.getAttribute("data-idle")).toBe("false");

    act(() => {
      vi.advanceTimersByTime(3000);
    });

    expect(footer.getAttribute("data-idle")).toBe("true");
    expect(footer.className).toContain("opacity-0");
    expect(document.body.style.cursor).toBe("none");

    act(() => {
      window.dispatchEvent(new Event("pointermove"));
    });
    expect(footer.getAttribute("data-idle")).toBe("false");
    expect(document.body.style.cursor).toBe("");
  });

  test("keeps the controls up while the pointer rests on them", () => {
    act(() => {
      root.render(<FullscreenControls />);
    });

    const footer = getFooter();

    act(() => {
      footer.dispatchEvent(new Event("pointerenter"));
    });
    act(() => {
      vi.advanceTimersByTime(6000);
    });

    expect(footer.getAttribute("data-idle")).toBe("false");

    act(() => {
      footer.dispatchEvent(new Event("pointerleave"));
    });
    act(() => {
      vi.advanceTimersByTime(3000);
    });

    expect(footer.getAttribute("data-idle")).toBe("true");
  });

  test("hides while paused, because the idle timer ignores playback state", () => {
    mockPlayerStore.snapshot = {
      song_id: "song-1",
      is_playing: false,
      volume: 0.5,
    };

    act(() => {
      root.render(<FullscreenControls />);
    });

    act(() => {
      vi.advanceTimersByTime(3000);
    });

    expect(getFooter().getAttribute("data-idle")).toBe("true");
  });
});

describe("FullscreenControls alignment button", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    mockLyricsStore.showRomanized = false;
    mockLyricsStore.isRomanizing = false;
    mockLyricsStore.songId = "song-1";
    mockLyricsStore.lines = [{ time_ms: 0, text: "hello" }];
    mockLyricsStore.lyricsAlignment = "left";
    mockLyricsStore.toggleLyricsAlignment = vi.fn(() => {
      mockLyricsStore.lyricsAlignment =
        mockLyricsStore.lyricsAlignment === "left" ? "center" : "left";
    });
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  function getAlignmentButton(): HTMLButtonElement {
    const button = container.querySelector<HTMLButtonElement>(
      '[data-testid="fullscreen-alignment-button"]',
    );
    if (!button) throw new Error("alignment button not rendered");
    return button;
  }

  test("toggles lyrics alignment when lyrics are available", async () => {
    await act(async () => {
      root.render(<FullscreenControls />);
    });

    await act(async () => {
      getAlignmentButton().click();
    });

    expect(mockLyricsStore.toggleLyricsAlignment).toHaveBeenCalled();
  });

  test("is disabled and does not toggle when no lyrics are available", async () => {
    mockLyricsStore.lines = [];
    mockLyricsStore.songId = null;

    await act(async () => {
      root.render(<FullscreenControls />);
    });

    expect(getAlignmentButton().disabled).toBe(true);

    await act(async () => {
      getAlignmentButton().click();
    });

    expect(mockLyricsStore.toggleLyricsAlignment).not.toHaveBeenCalled();
  });
});
