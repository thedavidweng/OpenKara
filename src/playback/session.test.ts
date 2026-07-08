import { describe, expect, test, vi } from "vitest";
import type {
  PlaybackStateSnapshot,
  SeparationStatusSnapshot,
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
      ...overrides.transport,
    },
    queue: {
      addToQueue: vi.fn(),
      dequeue: vi.fn().mockReturnValue(null),
      pushToHistory: vi.fn(),
      popFromHistory: vi.fn().mockReturnValue(null),
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
