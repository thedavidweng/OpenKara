import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
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
});
