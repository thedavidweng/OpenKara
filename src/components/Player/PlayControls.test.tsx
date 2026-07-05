// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { PlayControls } from "./PlayControls";

const { mockPlayerState } = vi.hoisted(() => ({
  mockPlayerState: {
    snapshot: {
      song_id: "song-1",
      state: "playing",
      is_playing: false,
    },
    resume: vi.fn(),
    pause: vi.fn(),
    skipBack: vi.fn(),
    skipForward: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: (selector: (state: typeof mockPlayerState) => unknown) =>
    selector(mockPlayerState),
}));

describe("PlayControls", () => {
  beforeEach(() => {
    mockPlayerState.resume.mockReset();
    mockPlayerState.pause.mockReset();
    mockPlayerState.skipBack.mockReset();
    mockPlayerState.skipForward.mockReset();
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
  });

  test("disables the main transport button while a selected song is loading", () => {
    mockPlayerState.snapshot = {
      song_id: "song-1",
      state: "loading",
      is_playing: false,
    };

    const markup = renderToStaticMarkup(<PlayControls />);

    expect(markup).toContain('aria-label="player.loading"');
    expect(markup).toContain('aria-busy="true"');
    expect(markup).toContain("disabled");
  });

  test("exposes the unified transport cluster markers", () => {
    mockPlayerState.snapshot = {
      song_id: "song-1",
      state: "playing",
      is_playing: false,
    };

    const markup = renderToStaticMarkup(<PlayControls />);

    expect(markup).toContain('data-play-controls-visual-variant="unified"');
    expect(markup).toContain('aria-label="player.play"');
  });

  test("pauses playback when the transport button is clicked while playing", () => {
    mockPlayerState.snapshot = {
      song_id: "song-1",
      state: "playing",
      is_playing: true,
    };

    const container = document.createElement("div");
    const root = createRoot(container);
    act(() => {
      root.render(<PlayControls />);
    });

    const transportButton = container.querySelectorAll("button")[1];
    act(() => {
      transportButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(mockPlayerState.pause).toHaveBeenCalledTimes(1);
    expect(mockPlayerState.resume).not.toHaveBeenCalled();
    act(() => {
      root.unmount();
    });
  });

  test("resumes playback when idle with a selected song", () => {
    mockPlayerState.snapshot = {
      song_id: "song-1",
      state: "playing",
      is_playing: false,
    };

    const container = document.createElement("div");
    const root = createRoot(container);
    act(() => {
      root.render(<PlayControls />);
    });

    const transportButton = container.querySelectorAll("button")[1];
    act(() => {
      transportButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(mockPlayerState.resume).toHaveBeenCalledTimes(1);
    expect(mockPlayerState.pause).not.toHaveBeenCalled();
    act(() => {
      root.unmount();
    });
  });

  test("ignores transport clicks while loading", () => {
    mockPlayerState.snapshot = {
      song_id: "song-1",
      state: "loading",
      is_playing: false,
    };

    const container = document.createElement("div");
    const root = createRoot(container);
    act(() => {
      root.render(<PlayControls />);
    });

    const transportButton = container.querySelectorAll("button")[1];
    act(() => {
      transportButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(mockPlayerState.pause).not.toHaveBeenCalled();
    expect(mockPlayerState.resume).not.toHaveBeenCalled();
    act(() => {
      root.unmount();
    });
  });

  test("wires skip back and skip forward actions", () => {
    mockPlayerState.snapshot = {
      song_id: "song-1",
      state: "playing",
      is_playing: false,
    };

    const container = document.createElement("div");
    const root = createRoot(container);
    act(() => {
      root.render(<PlayControls density="compact" />);
    });

    const [skipBackButton, , skipForwardButton] =
      container.querySelectorAll("button");
    act(() => {
      skipBackButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      skipForwardButton.dispatchEvent(
        new MouseEvent("click", { bubbles: true }),
      );
    });

    expect(mockPlayerState.skipBack).toHaveBeenCalledTimes(1);
    expect(mockPlayerState.skipForward).toHaveBeenCalledTimes(1);
    expect(container.firstElementChild?.className).toContain("gap-2.5");
    act(() => {
      root.unmount();
    });
  });
});
