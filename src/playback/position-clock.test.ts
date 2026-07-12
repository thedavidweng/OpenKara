import { describe, expect, test, vi } from "vitest";
import type { PlaybackStateSnapshot } from "@/types/ipc";
import {
  reduceAuthoritativeSnapshot,
  reducePositionEvent,
  selectCurrentPositionMs,
  type PositionClockState,
} from "./position-clock";

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

  test("patches buffered_ms without replacing the full snapshot", () => {
    const prev = {
      snapshot: snapshot({ buffered_ms: 1000, position_ms: 500 }),
      positionMs: 500,
      playingSinceMs: 1000,
    };

    const next = reducePositionEvent(
      prev,
      {
        ms: 600,
        transport_generation: 1,
        snapshot: snapshot({
          buffered_ms: 2500,
          position_ms: 600,
        }),
      },
      1500,
    );

    expect(next).toEqual({
      positionMs: 600,
      // Fresh position ⇒ fresh anchor. A kept anchor would double-count time.
      playingSinceMs: 1500,
      snapshot: {
        ...prev.snapshot,
        buffered_ms: 2500,
      },
    });
  });

  test("keeps absolute position events paired with their arrival-time anchor", () => {
    // REGRESSION: keeping the play-start anchor while adopting each event's
    // fresh absolute position made selectCurrentPositionMs run at ~2× real
    // time. After ~30s the displayed clock was ~60s, racing past the last
    // lyric line (looked like "scroll freeze"); pause/seek re-anchored and
    // briefly looked correct. Continuous 33ms events are required to catch this.
    let clock: PositionClockState = {
      snapshot: snapshot({ position_ms: 0 }),
      positionMs: 0,
      playingSinceMs: 0,
    };

    for (let nowMs = 33; nowMs <= 330; nowMs += 33) {
      clock = reducePositionEvent(
        clock,
        {
          ms: nowMs,
          transport_generation: 1,
          snapshot: snapshot({ position_ms: nowMs }),
        },
        nowMs,
      )!;
    }

    // (positionMs, playingSinceMs) must be one sync point — not a kept anchor.
    expect(clock.positionMs).toBe(330);
    expect(clock.playingSinceMs).toBe(330);
    expect(selectCurrentPositionMs(clock, () => 330)).toBe(330);
    expect(selectCurrentPositionMs(clock, () => 350)).toBe(350);
  });

  test("re-anchors playingSinceMs across a long 33ms position stream", () => {
    // Longer stream (~10s) so drift would be multi-second if the anchor leaked.
    let now = 1_000;
    let clock: PositionClockState = reduceAuthoritativeSnapshot(
      { snapshot: null, positionMs: 0, playingSinceMs: null },
      snapshot({ position_ms: 0 }),
      now,
    )!;

    for (let i = 1; i <= 300; i++) {
      now = 1_000 + i * 33;
      const next = reducePositionEvent(
        clock,
        {
          ms: i * 33,
          transport_generation: 1,
          snapshot: snapshot({ position_ms: i * 33 }),
        },
        now,
      );
      if (next) clock = next;
    }

    expect(clock.playingSinceMs).toBe(now);
    const displayed = selectCurrentPositionMs(clock, () => now);
    expect(Math.abs(displayed - 300 * 33)).toBeLessThan(50);
    // 20ms of pure client extrapolation after the last event — no 2× drift.
    expect(selectCurrentPositionMs(clock, () => now + 20)).toBe(300 * 33 + 20);
  });

  test("promotes transport_generation on position events so delayed pre-seek events are dropped", () => {
    // REGRESSION: seek bumps generation on the backend. If the position-event
    // reducer only patched positionMs and left snapshot.transport_generation
    // at the pre-seek value, a late generation-1 event looked non-stale and
    // yanked the clock (and lyrics) back before the seek.
    let clock: PositionClockState = {
      snapshot: snapshot({
        transport_generation: 1,
        position_ms: 1000,
      }),
      positionMs: 1000,
      playingSinceMs: 1000,
    };

    // Post-seek stream: generation 2, otherwise identical transport fields.
    const afterSeek = reducePositionEvent(
      clock,
      {
        ms: 10_000,
        transport_generation: 2,
        snapshot: snapshot({
          transport_generation: 2,
          position_ms: 10_000,
        }),
      },
      2000,
    );
    expect(afterSeek).not.toBeNull();
    clock = afterSeek!;
    expect(clock.snapshot?.transport_generation).toBe(2);
    expect(clock.positionMs).toBe(10_000);

    // Delayed pre-seek event must be ignored.
    const late = reducePositionEvent(
      clock,
      {
        ms: 1100,
        transport_generation: 1,
        snapshot: snapshot({
          transport_generation: 1,
          position_ms: 1100,
        }),
      },
      2100,
    );
    expect(late).toBeNull();
    expect(clock.positionMs).toBe(10_000);
    expect(clock.snapshot?.transport_generation).toBe(2);
  });
});
