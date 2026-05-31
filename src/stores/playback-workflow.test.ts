import { describe, expect, test, vi } from "vitest";
import type {
  PlaybackStateSnapshot,
  SeparationStatusSnapshot,
} from "@/types/ipc";
import {
  createPlaybackWorkflow,
  shouldEnqueueInsteadOfReplacingCurrentSong,
  shouldLoadSeparatedStems,
  type PlaybackWorkflowDeps,
} from "./playback-workflow";

function snapshot(
  overrides: Partial<PlaybackStateSnapshot> = {},
): PlaybackStateSnapshot {
  return {
    song_id: null,
    state: "idle",
    is_playing: false,
    position_ms: 0,
    duration_ms: null,
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
  overrides: Partial<PlaybackWorkflowDeps> = {},
): PlaybackWorkflowDeps {
  return {
    getPlayerSnapshot: () => null,
    play: vi
      .fn()
      .mockResolvedValue(snapshot({ song_id: "song-1", is_playing: true })),
    loadStems: vi
      .fn()
      .mockResolvedValue(
        snapshot({ song_id: "song-1", is_playing: true, has_stems: true }),
      ),
    getSeparationStatus: () => undefined,
    applySnapshot: vi.fn(),
    seek: vi.fn().mockResolvedValue(snapshot({ position_ms: 0 })),
    addToQueue: vi.fn(),
    dequeue: vi.fn().mockReturnValue(null),
    pushToHistory: vi.fn(),
    popFromHistory: vi.fn().mockReturnValue(null),
    ...overrides,
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

describe("createPlaybackWorkflow", () => {
  describe("playSong", () => {
    test("queues when another song is currently loaded", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => snapshot({ song_id: "current" }),
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.playSong("next-song");

      expect(deps.addToQueue).toHaveBeenCalledWith("next-song");
      expect(deps.play).not.toHaveBeenCalled();
    });

    test("pushes current song to history and replays it when same song requested", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () =>
          snapshot({ song_id: "old-song", is_playing: false }),
      });
      const workflow = createPlaybackWorkflow(deps);

      // Replaying the same song — skips queue check, pushes to history, plays
      await workflow.playSong("old-song");

      expect(deps.pushToHistory).toHaveBeenCalledWith("old-song");
      expect(deps.play).toHaveBeenCalledWith("old-song");
      expect(deps.applySnapshot).toHaveBeenCalled();
    });

    test("plays directly when no song is loaded", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => null,
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.playSong("first-song");

      expect(deps.addToQueue).not.toHaveBeenCalled();
      expect(deps.pushToHistory).not.toHaveBeenCalled();
      expect(deps.play).toHaveBeenCalledWith("first-song");
    });
  });

  describe("playNow", () => {
    test("pushes current song to history and plays immediately", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => snapshot({ song_id: "current" }),
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.playNow("new-song");

      expect(deps.pushToHistory).toHaveBeenCalledWith("current");
      expect(deps.play).toHaveBeenCalledWith("new-song");
      expect(deps.addToQueue).not.toHaveBeenCalled();
    });

    test("plays even when same song (replay)", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => snapshot({ song_id: "same-song" }),
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.playNow("same-song");

      expect(deps.pushToHistory).toHaveBeenCalledWith("same-song");
      expect(deps.play).toHaveBeenCalledWith("same-song");
    });
  });

  describe("playNextFromQueue", () => {
    test("dequeues and plays next song", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => snapshot({ song_id: "ended-song" }),
        dequeue: vi.fn().mockReturnValue("next-song"),
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.playNextFromQueue("ended-song");

      expect(deps.dequeue).toHaveBeenCalled();
      expect(deps.pushToHistory).toHaveBeenCalledWith("ended-song");
      expect(deps.play).toHaveBeenCalledWith("next-song");
    });

    test("ignores when ended song does not match current", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => snapshot({ song_id: "different-song" }),
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.playNextFromQueue("ended-song");

      expect(deps.dequeue).not.toHaveBeenCalled();
    });

    test("does nothing when queue is empty", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => snapshot({ song_id: "ended-song" }),
        dequeue: vi.fn().mockReturnValue(null),
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.playNextFromQueue("ended-song");

      expect(deps.pushToHistory).not.toHaveBeenCalled();
      expect(deps.play).not.toHaveBeenCalled();
    });
  });

  describe("skipForward", () => {
    test("dequeues next and pushes current to history", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => snapshot({ song_id: "current" }),
        dequeue: vi.fn().mockReturnValue("next-song"),
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.skipForward();

      expect(deps.pushToHistory).toHaveBeenCalledWith("current");
      expect(deps.play).toHaveBeenCalledWith("next-song");
    });

    test("does nothing when queue is empty", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => snapshot({ song_id: "current" }),
        dequeue: vi.fn().mockReturnValue(null),
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.skipForward();

      expect(deps.pushToHistory).not.toHaveBeenCalled();
      expect(deps.play).not.toHaveBeenCalled();
    });
  });

  describe("skipBack", () => {
    test("pops from history and plays previous song", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => snapshot({ song_id: "current" }),
        popFromHistory: vi.fn().mockReturnValue("previous-song"),
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.skipBack();

      expect(deps.popFromHistory).toHaveBeenCalled();
      expect(deps.play).toHaveBeenCalledWith("previous-song");
      expect(deps.seek).not.toHaveBeenCalled();
    });

    test("seeks to 0 when history is empty", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => snapshot({ song_id: "current" }),
        popFromHistory: vi.fn().mockReturnValue(null),
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.skipBack();

      expect(deps.seek).toHaveBeenCalledWith(0);
      expect(deps.applySnapshot).toHaveBeenCalled();
      expect(deps.play).not.toHaveBeenCalled();
    });

    test("does nothing when no song loaded", async () => {
      const deps = mockDeps({
        getPlayerSnapshot: () => null,
      });
      const workflow = createPlaybackWorkflow(deps);

      await workflow.skipBack();

      expect(deps.popFromHistory).not.toHaveBeenCalled();
      expect(deps.seek).not.toHaveBeenCalled();
    });
  });
});
