import { describe, expect, test, vi } from "vitest";
import { readLyricsAdjustedPlaybackMs } from "./lyrics-playback-clock";

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: { getState: vi.fn() },
  selectCurrentPositionMs: vi.fn(),
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: { getState: vi.fn() },
}));

import { usePlayerStore, selectCurrentPositionMs } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";

describe("readLyricsAdjustedPlaybackMs", () => {
  test("returns positionMs - offsetMs when not in AirPlay mode", () => {
    vi.mocked(usePlayerStore.getState).mockReturnValue({
      airPlayOutput: { active: false, displayedPositionMs: null },
    } as ReturnType<(typeof usePlayerStore)["getState"]>);
    vi.mocked(selectCurrentPositionMs).mockReturnValue(5000);
    vi.mocked(useLyricsStore.getState).mockReturnValue({
      offsetMs: 200,
    } as ReturnType<(typeof useLyricsStore)["getState"]>);

    const result = readLyricsAdjustedPlaybackMs();

    expect(selectCurrentPositionMs).toHaveBeenCalledWith(
      expect.objectContaining({
        airPlayOutput: { active: false, displayedPositionMs: null },
      }),
    );
    expect(result).toBe(4800);
  });

  test("returns airPlayOutput.displayedPositionMs - offsetMs when AirPlay is active", () => {
    vi.mocked(usePlayerStore.getState).mockReturnValue({
      airPlayOutput: { active: true, displayedPositionMs: 3000 },
    } as ReturnType<(typeof usePlayerStore)["getState"]>);
    vi.mocked(selectCurrentPositionMs).mockReturnValue(9999);
    vi.mocked(useLyricsStore.getState).mockReturnValue({
      offsetMs: 100,
    } as ReturnType<(typeof useLyricsStore)["getState"]>);

    const result = readLyricsAdjustedPlaybackMs();

    // Should use the AirPlay position, not the native position
    expect(result).toBe(2900);
  });

  test("falls back to native position when AirPlay is active but displayedPositionMs is null", () => {
    vi.mocked(usePlayerStore.getState).mockReturnValue({
      airPlayOutput: { active: true, displayedPositionMs: null },
    } as ReturnType<(typeof usePlayerStore)["getState"]>);
    vi.mocked(selectCurrentPositionMs).mockReturnValue(7000);
    vi.mocked(useLyricsStore.getState).mockReturnValue({
      offsetMs: 0,
    } as ReturnType<(typeof useLyricsStore)["getState"]>);

    const result = readLyricsAdjustedPlaybackMs();

    expect(selectCurrentPositionMs).toHaveBeenCalled();
    expect(result).toBe(7000);
  });

  test("subtracts negative offsetMs (adds to position)", () => {
    vi.mocked(usePlayerStore.getState).mockReturnValue({
      airPlayOutput: { active: false, displayedPositionMs: null },
    } as ReturnType<(typeof usePlayerStore)["getState"]>);
    vi.mocked(selectCurrentPositionMs).mockReturnValue(1000);
    vi.mocked(useLyricsStore.getState).mockReturnValue({
      offsetMs: -500,
    } as ReturnType<(typeof useLyricsStore)["getState"]>);

    const result = readLyricsAdjustedPlaybackMs();

    // 1000 - (-500) = 1500
    expect(result).toBe(1500);
  });
});
