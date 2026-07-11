import { describe, expect, test, vi } from "vitest";
import type { PlaybackStateSnapshot } from "@/types/ipc";
import { reducePositionEvent, selectCurrentPositionMs } from "./position-clock";

function snapshot(
  overrides: Partial<PlaybackStateSnapshot> = {},
): PlaybackStateSnapshot {
  return {
    song_id: "song-1",
    transport_generation: 1,
    state: "playing",
    is_playing: true,
    position_ms: 1000,
    duration_ms: 5000,
    buffered_ms: 5000,
    volume: 1,
    stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
    has_stems: false,
    stem_mode: null,
    ...overrides,
  };
}

describe("selectCurrentPositionMs", () => {
  test("uses performance.now by default when extrapolating", () => {
    vi.spyOn(performance, "now").mockReturnValue(2500);

    expect(
      selectCurrentPositionMs({
        snapshot: snapshot({ is_playing: true }),
        positionMs: 1000,
        playingSinceMs: 2000,
      }),
    ).toBe(1500);
  });
});

describe("reducePositionEvent", () => {
  test("ignores events whose transport_generation disagrees with the snapshot", () => {
    const prev = {
      snapshot: snapshot(),
      positionMs: 1000,
      playingSinceMs: 1000,
    };

    expect(
      reducePositionEvent(
        prev,
        {
          ms: 1100,
          transport_generation: 1,
          snapshot: snapshot({
            transport_generation: 2,
            position_ms: 1100,
          }),
        },
        1500,
      ),
    ).toBeNull();
  });
});
