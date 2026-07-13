import { describe, expect, test, vi } from "vitest";
import type {
  PlaybackStateSnapshot,
  SeparationStatusSnapshot,
  TrackTransitionedEvent,
} from "@/types/ipc";
import {
  createPlaybackSession,
  type PlaybackSessionDeps,
  type PlaybackTransport,
  type PlaybackQueueOps,
} from "./session";
import {
  shouldEnqueueInsteadOfReplacingCurrentSong,
  shouldLoadSeparatedStems,
} from "./session-policies";

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

function transitionEvent(
  overrides: Partial<TrackTransitionedEvent> = {},
): TrackTransitionedEvent {
  return {
    transitionSerial: 1,
    preloadGeneration: 1,
    fromSongId: "old-song",
    toSongId: "new-song",
    state: snapshot({ song_id: "new-song", is_playing: true }),
    ...overrides,
  };
}

function completedSeparation(
  overrides: Partial<SeparationStatusSnapshot> = {},
): SeparationStatusSnapshot {
  return {
    song_id: "song-1",
    state: "completed",
    percent: 100,
    cache_hit: true,
    vocals_path: "vocals.ogg",
    accomp_path: "accomp.ogg",
    drums_path: null,
    bass_path: null,
    other_path: null,
    model_variant: "htdemucs",
    error: null,
    ...overrides,
  };
}

function mockDeps(
  overrides: {
    transport?: Partial<PlaybackTransport>;
    queue?: Partial<PlaybackQueueOps>;
    getSeparationStatus?: PlaybackSessionDeps["getSeparationStatus"];
    onClockChange?: PlaybackSessionDeps["onClockChange"];
    nowMs?: () => number;
  } = {},
): PlaybackSessionDeps {
  return {
    transport: {
      play: vi
        .fn()
        .mockResolvedValue(snapshot({ song_id: "song-1", is_playing: true })),
      resume: vi.fn().mockResolvedValue(snapshot({ is_playing: true })),
      pause: vi.fn().mockResolvedValue(snapshot({ is_playing: false })),
      seek: vi.fn().mockResolvedValue(snapshot({ position_ms: 0 })),
      setVolume: vi.fn().mockResolvedValue(snapshot({ volume: 1 })),
      setStemVolume: vi.fn().mockResolvedValue(snapshot()),
      loadStems: vi
        .fn()
        .mockResolvedValue(
          snapshot({ song_id: "song-1", is_playing: true, has_stems: true }),
        ),
      getPlaybackState: vi.fn().mockResolvedValue(snapshot()),
      setPreloadCandidate: vi.fn().mockResolvedValue(undefined),
      ...overrides.transport,
    },
    queue: {
      addToQueue: vi.fn(),
      dequeue: vi.fn().mockReturnValue(null),
      pushToHistory: vi.fn(),
      popFromHistory: vi.fn().mockReturnValue(null),
      removeSongIds: vi.fn(),
      reconcileGaplessTransition: vi.fn(),
      peekHead: vi.fn().mockReturnValue(null),
      ...overrides.queue,
    },
    getSeparationStatus: overrides.getSeparationStatus ?? (() => undefined),
    onClockChange: overrides.onClockChange ?? vi.fn(),
    nowMs: overrides.nowMs ?? (() => 1000),
  };
}

describe("shouldEnqueueInsteadOfReplacingCurrentSong", () => {
  test("queues when another song is playing", () => {
    expect(
      shouldEnqueueInsteadOfReplacingCurrentSong(
        snapshot({ song_id: "current", is_playing: true }),
        "next-song",
      ),
    ).toBe(true);
  });

  test("queues when another song is paused", () => {
    expect(
      shouldEnqueueInsteadOfReplacingCurrentSong(
        snapshot({ song_id: "current", is_playing: false }),
        "next-song",
      ),
    ).toBe(true);
  });

  test("does not queue when no song loaded", () => {
    expect(shouldEnqueueInsteadOfReplacingCurrentSong(null, "next-song")).toBe(
      false,
    );
  });

  test("does not queue when replaying current song", () => {
    expect(
      shouldEnqueueInsteadOfReplacingCurrentSong(
        snapshot({ song_id: "current", is_playing: true }),
        "current",
      ),
    ).toBe(false);
  });

  test("does not queue when snapshot has null song_id", () => {
    expect(
      shouldEnqueueInsteadOfReplacingCurrentSong(
        snapshot({ song_id: null }),
        "any-song",
      ),
    ).toBe(false);
  });
});

describe("shouldLoadSeparatedStems", () => {
  test("loads when separation completed and stems not loaded", () => {
    expect(
      shouldLoadSeparatedStems(
        snapshot({ has_stems: false }),
        completedSeparation(),
      ),
    ).toBe(true);
  });

  test("skips when stems already loaded", () => {
    expect(
      shouldLoadSeparatedStems(
        snapshot({ has_stems: true }),
        completedSeparation(),
      ),
    ).toBe(false);
  });

  test("skips while loading", () => {
    expect(
      shouldLoadSeparatedStems(
        snapshot({ state: "loading", has_stems: false }),
        completedSeparation(),
      ),
    ).toBe(false);
  });

  test("skips when separation not completed", () => {
    expect(
      shouldLoadSeparatedStems(
        snapshot({ has_stems: false }),
        completedSeparation({ state: "running" }),
      ),
    ).toBe(false);
  });
});

describe("createPlaybackSession", () => {
  describe("play", () => {
    test("queues when another song is currently loaded", async () => {
      const deps = mockDeps();
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "current" }));

      await session.play("next-song");

      expect(deps.queue.addToQueue).toHaveBeenCalledWith("next-song");
      expect(deps.transport.play).not.toHaveBeenCalled();
    });

    test("pushes current song to history and replays it when same song requested", async () => {
      const deps = mockDeps({
        transport: {
          play: vi
            .fn()
            .mockResolvedValue(
              snapshot({ song_id: "old-song", is_playing: true }),
            ),
        },
      });
      const session = createPlaybackSession(deps);
      session.applySnapshot(
        snapshot({ song_id: "old-song", is_playing: false }),
      );

      await session.play("old-song");

      expect(deps.queue.pushToHistory).toHaveBeenCalledWith("old-song");
      expect(deps.transport.play).toHaveBeenCalledWith("old-song");
      expect(deps.onClockChange).toHaveBeenCalled();
    });

    test("plays directly when no song is loaded", async () => {
      const deps = mockDeps({
        transport: {
          play: vi
            .fn()
            .mockResolvedValue(
              snapshot({ song_id: "first-song", is_playing: true }),
            ),
        },
      });
      const session = createPlaybackSession(deps);

      await session.play("first-song");

      expect(deps.queue.addToQueue).not.toHaveBeenCalled();
      expect(deps.queue.pushToHistory).not.toHaveBeenCalled();
      expect(deps.transport.play).toHaveBeenCalledWith("first-song");
    });
  });

  describe("playNow", () => {
    test("pushes current song to history and plays immediately", async () => {
      const deps = mockDeps({
        transport: {
          play: vi
            .fn()
            .mockResolvedValue(
              snapshot({ song_id: "new-song", is_playing: true }),
            ),
        },
      });
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "current" }));

      await session.playNow("new-song");

      expect(deps.queue.pushToHistory).toHaveBeenCalledWith("current");
      expect(deps.transport.play).toHaveBeenCalledWith("new-song");
      expect(deps.queue.addToQueue).not.toHaveBeenCalled();
    });

    test("plays even when same song (replay)", async () => {
      const deps = mockDeps({
        transport: {
          play: vi
            .fn()
            .mockResolvedValue(
              snapshot({ song_id: "same-song", is_playing: true }),
            ),
        },
      });
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "same-song" }));

      await session.playNow("same-song");

      expect(deps.queue.pushToHistory).toHaveBeenCalledWith("same-song");
      expect(deps.transport.play).toHaveBeenCalledWith("same-song");
    });
  });

  describe("onEnded", () => {
    test("dequeues and plays next song", async () => {
      const deps = mockDeps({
        queue: {
          dequeue: vi.fn().mockReturnValue("next-song"),
        },
        transport: {
          play: vi
            .fn()
            .mockResolvedValue(
              snapshot({ song_id: "next-song", is_playing: true }),
            ),
        },
      });
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "ended-song" }));

      await session.onEnded("ended-song");

      expect(deps.queue.dequeue).toHaveBeenCalled();
      expect(deps.queue.pushToHistory).toHaveBeenCalledWith("ended-song");
      expect(deps.transport.play).toHaveBeenCalledWith("next-song");
    });

    test("ignores when ended song does not match current", async () => {
      const deps = mockDeps();
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "different-song" }));

      await session.onEnded("ended-song");

      expect(deps.queue.dequeue).not.toHaveBeenCalled();
    });

    test("does nothing when queue is empty", async () => {
      const deps = mockDeps();
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "ended-song" }));

      await session.onEnded("ended-song");

      expect(deps.queue.pushToHistory).not.toHaveBeenCalled();
      expect(deps.transport.play).not.toHaveBeenCalled();
    });
  });

  describe("onTrackTransitioned", () => {
    test("reconciles queue and applies authoritative state", () => {
      // #88: The backend emits track-transitioned with the authoritative
      // post-transition snapshot. The handler applies the snapshot and
      // reconciles the queue via the named store action.
      const deps = mockDeps();
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "old-song" }));

      session.onTrackTransitioned(transitionEvent());

      expect(deps.queue.reconcileGaplessTransition).toHaveBeenCalledWith(
        "old-song",
        "new-song",
      );
    });

    test("ignores duplicate transition by serial", () => {
      // #88: Idempotent serial dedup — the same serial must not advance
      // the queue twice.
      const deps = mockDeps();
      const session = createPlaybackSession(deps);

      session.onTrackTransitioned(transitionEvent({ transitionSerial: 5 }));
      session.onTrackTransitioned(transitionEvent({ transitionSerial: 5 }));

      expect(deps.queue.reconcileGaplessTransition).toHaveBeenCalledTimes(1);
    });

    test("ignores stale transition with lower serial", () => {
      const deps = mockDeps();
      const session = createPlaybackSession(deps);

      session.onTrackTransitioned(transitionEvent({ transitionSerial: 5 }));
      session.onTrackTransitioned(transitionEvent({ transitionSerial: 3 }));

      expect(deps.queue.reconcileGaplessTransition).toHaveBeenCalledTimes(1);
      expect(deps.queue.reconcileGaplessTransition).toHaveBeenCalledWith(
        "old-song",
        "new-song",
      );
    });

    test("applies authoritative snapshot from event state", () => {
      // #88: The event's `state` field is the authoritative post-transition
      // snapshot. The handler must apply it so the clock holds `toSongId`.
      const deps = mockDeps();
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "old-song" }));

      session.onTrackTransitioned(
        transitionEvent({
          state: snapshot({
            song_id: "new-song",
            is_playing: true,
            position_ms: 42,
          }),
        }),
      );

      expect(session.getPositionClock().snapshot?.song_id).toBe("new-song");
      expect(session.getPositionClock().positionMs).toBe(42);
    });

    test("never calls play() in response to transition event", () => {
      const deps = mockDeps();
      const session = createPlaybackSession(deps);

      session.onTrackTransitioned(transitionEvent());

      expect(deps.transport.play).not.toHaveBeenCalled();
    });

    test("updates preload candidate from the resulting queue head", () => {
      // #88: After reconciliation, the session must update the preload
      // candidate to the new queue head so the backend can decode the
      // next track for a future gapless/crossfade transition.
      const deps = mockDeps({
        queue: {
          peekHead: vi.fn().mockReturnValue("song-c"),
        },
      });
      const session = createPlaybackSession(deps);

      session.onTrackTransitioned(transitionEvent());

      expect(deps.queue.peekHead).toHaveBeenCalledTimes(1);
      expect(deps.transport.setPreloadCandidate).toHaveBeenCalledWith("song-c");
    });

    test("clears preload candidate when queue is empty after reconciliation", () => {
      // #88: An empty queue after reconciliation clears the preload
      // candidate (null) so the backend does not hold a stale preparation.
      const deps = mockDeps({
        queue: {
          peekHead: vi.fn().mockReturnValue(null),
        },
      });
      const session = createPlaybackSession(deps);

      session.onTrackTransitioned(transitionEvent());

      expect(deps.transport.setPreloadCandidate).toHaveBeenCalledWith(null);
    });

    test("does not update preload candidate on duplicate serial", () => {
      // #88: A duplicate transition must not re-trigger the preload
      // candidate update — the queue head has not changed since the
      // first application.
      const deps = mockDeps({
        queue: {
          peekHead: vi.fn().mockReturnValue("song-c"),
        },
      });
      const session = createPlaybackSession(deps);

      session.onTrackTransitioned(transitionEvent({ transitionSerial: 5 }));
      session.onTrackTransitioned(transitionEvent({ transitionSerial: 5 }));

      expect(deps.transport.setPreloadCandidate).toHaveBeenCalledTimes(1);
    });

    test("skips reconciliation when user manually switched tracks (stale generation)", () => {
      // #89: If the user manually started a different track during the
      // ~33ms window between the backend's gapless swap and the position
      // emitter draining the transition, the clock holds a newer
      // transport_generation. The transition event's snapshot has the
      // old generation, so `tryApplyAuthoritative` rejects it. The
      // handler must NOT reconcile the queue — the user has already
      // moved to an unrelated track and removing `toSongId` / pushing
      // `fromSongId` to history would corrupt the queue.
      const deps = mockDeps();
      const session = createPlaybackSession(deps);
      // Clock holds a manually-started track with generation 10.
      session.applySnapshot(
        snapshot({ song_id: "user-picked-song", transport_generation: 10 }),
      );

      // Transition event has generation 5 (stale — the gapless swap
      // happened before the manual track change).
      session.onTrackTransitioned(
        transitionEvent({
          state: snapshot({
            song_id: "new-song",
            transport_generation: 5,
            is_playing: true,
          }),
        }),
      );

      // The stale snapshot must not be applied.
      expect(session.getPositionClock().snapshot?.song_id).toBe(
        "user-picked-song",
      );
      // Reconciliation must NOT happen — the queue belongs to the
      // user's manually-started track, not the gapless transition.
      expect(deps.queue.reconcileGaplessTransition).not.toHaveBeenCalled();
      // Preload candidate must NOT be updated from the stale transition.
      expect(deps.transport.setPreloadCandidate).not.toHaveBeenCalled();
    });

    test("reconciles when position event arrived first with same generation", () => {
      // #89: A position event with the same transport_generation as the
      // transition may arrive first (the gapless swap does not bump
      // transport_generation). The transition snapshot is not stale, so
      // `tryApplyAuthoritative` accepts it and reconciliation proceeds.
      const deps = mockDeps();
      const session = createPlaybackSession(deps);
      // Clock holds the from-song with generation 5 (same as the
      // transition event's generation).
      session.applySnapshot(
        snapshot({ song_id: "old-song", transport_generation: 5 }),
      );

      session.onTrackTransitioned(
        transitionEvent({
          state: snapshot({
            song_id: "new-song",
            transport_generation: 5,
            is_playing: true,
          }),
        }),
      );

      // The snapshot must be applied (toSongId is now current).
      expect(session.getPositionClock().snapshot?.song_id).toBe("new-song");
      // Reconciliation must happen.
      expect(deps.queue.reconcileGaplessTransition).toHaveBeenCalledWith(
        "old-song",
        "new-song",
      );
    });
  });

  describe("skipForward", () => {
    test("dequeues next and pushes current to history", async () => {
      const deps = mockDeps({
        queue: {
          dequeue: vi.fn().mockReturnValue("next-song"),
        },
        transport: {
          play: vi
            .fn()
            .mockResolvedValue(
              snapshot({ song_id: "next-song", is_playing: true }),
            ),
        },
      });
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "current" }));

      await session.skipForward();

      expect(deps.queue.pushToHistory).toHaveBeenCalledWith("current");
      expect(deps.transport.play).toHaveBeenCalledWith("next-song");
    });

    test("does nothing when queue is empty", async () => {
      const deps = mockDeps();
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "current" }));

      await session.skipForward();

      expect(deps.queue.pushToHistory).not.toHaveBeenCalled();
      expect(deps.transport.play).not.toHaveBeenCalled();
    });
  });

  describe("skipBack", () => {
    test("pops from history and plays previous song", async () => {
      const deps = mockDeps({
        queue: {
          popFromHistory: vi.fn().mockReturnValue("previous-song"),
        },
        transport: {
          play: vi
            .fn()
            .mockResolvedValue(
              snapshot({ song_id: "previous-song", is_playing: true }),
            ),
        },
      });
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "current" }));

      await session.skipBack();

      expect(deps.queue.popFromHistory).toHaveBeenCalled();
      expect(deps.transport.play).toHaveBeenCalledWith("previous-song");
      expect(deps.transport.seek).not.toHaveBeenCalled();
    });

    test("seeks to 0 when history is empty", async () => {
      const deps = mockDeps();
      const session = createPlaybackSession(deps);
      session.applySnapshot(snapshot({ song_id: "current" }));

      await session.skipBack();

      expect(deps.transport.seek).toHaveBeenCalledWith(0);
      expect(deps.onClockChange).toHaveBeenCalled();
      expect(deps.transport.play).not.toHaveBeenCalled();
    });

    test("does nothing when no song loaded", async () => {
      const deps = mockDeps();
      const session = createPlaybackSession(deps);

      await session.skipBack();

      expect(deps.queue.popFromHistory).not.toHaveBeenCalled();
      expect(deps.transport.seek).not.toHaveBeenCalled();
    });
  });

  describe("seek", () => {
    test("returns false when no song is loaded", async () => {
      const deps = mockDeps();
      const session = createPlaybackSession(deps);

      await expect(session.seek(1500)).resolves.toBe(false);
      expect(deps.transport.seek).not.toHaveBeenCalled();
    });

    test("returns true after applying the authoritative target", async () => {
      const deps = mockDeps({
        transport: {
          seek: vi.fn().mockResolvedValue(
            snapshot({
              song_id: "song-1",
              transport_generation: 2,
              position_ms: 15_000,
            }),
          ),
        },
      });
      const session = createPlaybackSession(deps);
      session.applySnapshot(
        snapshot({ song_id: "song-1", transport_generation: 1 }),
      );

      await expect(session.seek(15_000)).resolves.toBe(true);
      expect(session.getPositionClock().positionMs).toBe(15_000);
    });

    test("returns false when a stale seek response is rejected", async () => {
      const deps = mockDeps({
        transport: {
          seek: vi.fn().mockResolvedValue(
            snapshot({
              song_id: "song-1",
              transport_generation: 1,
              position_ms: 15_000,
            }),
          ),
        },
      });
      const session = createPlaybackSession(deps);
      session.applySnapshot(
        snapshot({ song_id: "song-1", transport_generation: 2 }),
      );

      await expect(session.seek(15_000)).resolves.toBe(false);
      expect(session.getPositionClock().positionMs).toBe(0);
    });

    test("does not let a late buffering response overwrite a newer same-generation playing event", async () => {
      let resolveSeek!: (value: PlaybackStateSnapshot) => void;
      const seekResponse = new Promise<PlaybackStateSnapshot>((resolve) => {
        resolveSeek = resolve;
      });
      let now = 1000;
      const deps = mockDeps({
        nowMs: () => now,
        transport: {
          seek: vi.fn().mockReturnValue(seekResponse),
        },
      });
      const session = createPlaybackSession(deps);
      session.applySnapshot(
        snapshot({
          song_id: "song-1",
          transport_generation: 1,
          state: "playing",
          is_playing: true,
          position_ms: 1000,
        }),
      );

      const pending = session.seek(15_000);

      // Real streaming seek: the command first emits a buffering target. The
      // audio thread can then recover and emit playing before the invoke
      // response is delivered to JavaScript.
      now = 1100;
      session.applyPosition({
        ms: 15_000,
        transport_generation: 2,
        snapshot: snapshot({
          song_id: "song-1",
          transport_generation: 2,
          state: "buffering",
          is_playing: true,
          position_ms: 15_000,
        }),
      });
      now = 1200;
      session.applyPosition({
        ms: 15_050,
        transport_generation: 2,
        snapshot: snapshot({
          song_id: "song-1",
          transport_generation: 2,
          state: "playing",
          is_playing: true,
          position_ms: 15_050,
        }),
      });

      // The older command response arrives last with the same generation.
      resolveSeek(
        snapshot({
          song_id: "song-1",
          transport_generation: 2,
          state: "buffering",
          is_playing: true,
          position_ms: 15_000,
        }),
      );

      await expect(pending).resolves.toBe(true);
      expect(session.getPositionClock()).toMatchObject({
        positionMs: 15_050,
        playingSinceMs: 1200,
        snapshot: {
          transport_generation: 2,
          state: "playing",
          position_ms: 15_050,
        },
      });
    });
  });
});

describe("volume updates ignore stale transport generations", () => {
  test("setVolume does not publish when the response is stale", async () => {
    const onClockChange = vi.fn();
    const deps = mockDeps({
      onClockChange,
      transport: {
        play: vi.fn().mockResolvedValue(
          snapshot({
            song_id: "song-1",
            is_playing: true,
            transport_generation: 2,
          }),
        ),
        setVolume: vi.fn().mockResolvedValue(
          snapshot({
            song_id: "song-1",
            volume: 0.5,
            transport_generation: 1,
          }),
        ),
      },
    });
    const session = createPlaybackSession(deps);
    await session.play("song-1");
    onClockChange.mockClear();

    await session.setVolume(0.5);

    expect(deps.transport.setVolume).toHaveBeenCalledWith(0.5);
    expect(onClockChange).not.toHaveBeenCalled();
    expect(session.getPositionClock().snapshot?.volume).toBe(1);
  });

  test("setStemVolume does not publish when the response is stale", async () => {
    const onClockChange = vi.fn();
    const deps = mockDeps({
      onClockChange,
      transport: {
        play: vi.fn().mockResolvedValue(
          snapshot({
            song_id: "song-1",
            is_playing: true,
            transport_generation: 2,
          }),
        ),
        setStemVolume: vi.fn().mockResolvedValue(
          snapshot({
            song_id: "song-1",
            transport_generation: 1,
            stem_volumes: { vocals: 0.2, drums: 1, bass: 1, other: 1 },
          }),
        ),
      },
    });
    const session = createPlaybackSession(deps);
    await session.play("song-1");
    onClockChange.mockClear();

    await session.setStemVolume("vocals", 0.2);

    expect(deps.transport.setStemVolume).toHaveBeenCalledWith("vocals", 0.2);
    expect(onClockChange).not.toHaveBeenCalled();
  });
});

describe("F6: guards against stale loadStems after song change", () => {
  test("skips stems apply when song changed during loadStems()", async () => {
    const onClockChange = vi.fn();
    let sessionRef: ReturnType<typeof createPlaybackSession> | null = null;

    const deps = mockDeps({
      getSeparationStatus: () => completedSeparation(),
      onClockChange,
      transport: {
        play: vi.fn().mockResolvedValue(
          snapshot({
            song_id: "song-1",
            is_playing: true,
            transport_generation: 1,
          }),
        ),
        loadStems: vi.fn().mockImplementation(async () => {
          // Mid-flight song switch (e.g. playNow during stem load).
          sessionRef?.applySnapshot(
            snapshot({
              song_id: "song-2",
              is_playing: true,
              transport_generation: 2,
            }),
          );
          return snapshot({
            song_id: "song-1",
            is_playing: true,
            has_stems: true,
            transport_generation: 1,
          });
        }),
      },
    });

    const session = createPlaybackSession(deps);
    sessionRef = session;
    await session.play("song-1");

    expect(session.getPositionClock().snapshot?.song_id).toBe("song-2");
    expect(session.getPositionClock().snapshot?.has_stems).toBe(false);
  });

  test("applies loadStems snapshot when song has not changed", async () => {
    const onClockChange = vi.fn();
    const deps = mockDeps({
      getSeparationStatus: () => completedSeparation(),
      onClockChange,
      transport: {
        play: vi
          .fn()
          .mockResolvedValue(snapshot({ song_id: "song-1", is_playing: true })),
        loadStems: vi
          .fn()
          .mockResolvedValue(
            snapshot({ song_id: "song-1", is_playing: true, has_stems: true }),
          ),
      },
    });

    const session = createPlaybackSession(deps);
    await session.play("song-1");

    expect(session.getPositionClock().snapshot?.has_stems).toBe(true);
    // play + stems
    expect(onClockChange).toHaveBeenCalledTimes(2);
  });
});
