import { describe, expect, test, vi } from "vitest";

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: { getState: vi.fn() },
  selectCurrentPositionMs: vi.fn(),
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: { getState: vi.fn() },
}));

import { readLyricsAdjustedPlaybackMs } from "./lyrics-playback-clock";
import { usePlayerStore, selectCurrentPositionMs } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";

function mockPlayerState(
  overrides: Partial<ReturnType<(typeof usePlayerStore)["getState"]>> = {},
) {
  vi.mocked(usePlayerStore.getState).mockReturnValue({
    snapshot: null,
    positionMs: 0,
    playingSinceMs: null,
    airPlayOutput: { active: false, displayedPositionMs: null },
    ...overrides,
  } as ReturnType<(typeof usePlayerStore)["getState"]>);
}

describe("readLyricsAdjustedPlaybackMs", () => {
  test("returns positionMs - offsetMs when not in AirPlay mode", () => {
    mockPlayerState({ positionMs: 5000 });
    vi.mocked(selectCurrentPositionMs).mockReturnValue(5000);
    vi.mocked(useLyricsStore.getState).mockReturnValue({
      offsetMs: 200,
    } as ReturnType<(typeof useLyricsStore)["getState"]>);

    const result = readLyricsAdjustedPlaybackMs();

    expect(selectCurrentPositionMs).toHaveBeenCalledWith(
      expect.objectContaining({
        positionMs: 5000,
      }),
      expect.any(Function),
    );
    expect(result).toBe(4800);
  });

  test("returns airPlayOutput.displayedPositionMs - offsetMs when AirPlay is active", () => {
    mockPlayerState({
      airPlayOutput: {
        active: true,
        displayedPositionMs: 3000,
      } as ReturnType<(typeof usePlayerStore)["getState"]>["airPlayOutput"],
    });
    vi.mocked(selectCurrentPositionMs).mockReturnValue(9999);
    vi.mocked(useLyricsStore.getState).mockReturnValue({
      offsetMs: 100,
    } as ReturnType<(typeof useLyricsStore)["getState"]>);

    const result = readLyricsAdjustedPlaybackMs();

    expect(result).toBe(2900);
  });

  test("falls back to native position when AirPlay is active but displayedPositionMs is null", () => {
    mockPlayerState({
      airPlayOutput: {
        active: true,
        displayedPositionMs: null,
      } as ReturnType<(typeof usePlayerStore)["getState"]>["airPlayOutput"],
      positionMs: 7000,
    });
    vi.mocked(selectCurrentPositionMs).mockReturnValue(7000);
    vi.mocked(useLyricsStore.getState).mockReturnValue({
      offsetMs: 0,
    } as ReturnType<(typeof useLyricsStore)["getState"]>);

    const result = readLyricsAdjustedPlaybackMs();

    expect(selectCurrentPositionMs).toHaveBeenCalled();
    expect(result).toBe(7000);
  });

  test("subtracts negative offsetMs (adds to position)", () => {
    mockPlayerState({ positionMs: 1000 });
    vi.mocked(selectCurrentPositionMs).mockReturnValue(1000);
    vi.mocked(useLyricsStore.getState).mockReturnValue({
      offsetMs: -500,
    } as ReturnType<(typeof useLyricsStore)["getState"]>);

    const result = readLyricsAdjustedPlaybackMs();

    expect(result).toBe(1500);
  });
});
