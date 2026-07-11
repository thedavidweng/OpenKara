/**
 * Compatibility: policies + session live under `@/playback`.
 * This file re-exports coverage by importing the session suite entry point
 * so existing paths still resolve in tooling that greps for workflow tests.
 */
import { describe, expect, test } from "vitest";
import {
  shouldEnqueueInsteadOfReplacingCurrentSong,
  shouldLoadSeparatedStems,
} from "./playback-workflow";
import type { PlaybackStateSnapshot } from "@/types/ipc";

function snapshot(
  overrides: Partial<PlaybackStateSnapshot> = {},
): PlaybackStateSnapshot {
  return {
    song_id: null,
    transport_generation: 0,
    state: "idle",
    is_playing: false,
    position_ms: 0,
    duration_ms: null,
    buffered_ms: 0,
    volume: 1,
    stem_volumes: { vocals: 1, drums: 1, bass: 1, other: 1 },
    has_stems: false,
    stem_mode: null,
    ...overrides,
  };
}

describe("playback-workflow shim re-exports policies", () => {
  test("shouldEnqueueInsteadOfReplacingCurrentSong", () => {
    expect(
      shouldEnqueueInsteadOfReplacingCurrentSong(
        snapshot({ song_id: "a" }),
        "b",
      ),
    ).toBe(true);
  });

  test("shouldLoadSeparatedStems", () => {
    expect(
      shouldLoadSeparatedStems(snapshot({ has_stems: true }), undefined),
    ).toBe(false);
  });
});
