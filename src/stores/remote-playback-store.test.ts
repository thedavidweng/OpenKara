import { beforeEach, describe, expect, test } from "vitest";
import { useRemotePlaybackStore } from "./remote-playback-store";

describe("remote-playback-store", () => {
  beforeEach(() => {
    useRemotePlaybackStore.getState().reset();
  });

  test("applyReconnectEvent sets reconnecting state with attempt info", () => {
    useRemotePlaybackStore.getState().applyReconnectEvent({
      song_id: "song-1",
      request_id: 1,
      attempt: 2,
      max_attempts: 3,
      reason: "transient network error",
    });

    const state = useRemotePlaybackStore.getState();
    expect(state.reconnectState).toBe("reconnecting");
    expect(state.songId).toBe("song-1");
    expect(state.attempt).toBe(2);
    expect(state.maxAttempts).toBe(3);
    expect(state.reason).toBe("transient network error");
    expect(state.resyncDeltaMs).toBeNull();
  });

  test("applyResyncEvent sets resync state with delta", () => {
    useRemotePlaybackStore.getState().applyResyncEvent({
      song_id: "song-1",
      requested_position_ms: 10_000,
      actual_position_ms: 9_500,
    });

    const state = useRemotePlaybackStore.getState();
    expect(state.reconnectState).toBe("resync");
    expect(state.songId).toBe("song-1");
    expect(state.resyncDeltaMs).toBe(500);
  });

  test("applyFailedEvent sets failed state with reason", () => {
    useRemotePlaybackStore.getState().applyFailedEvent({
      song_id: "song-1",
      request_id: 1,
      reason: "permanent error",
    });

    const state = useRemotePlaybackStore.getState();
    expect(state.reconnectState).toBe("failed");
    expect(state.songId).toBe("song-1");
    expect(state.reason).toBe("permanent error");
  });

  test("reset clears all state to idle", () => {
    useRemotePlaybackStore.getState().applyReconnectEvent({
      song_id: "song-1",
      request_id: 1,
      attempt: 1,
      max_attempts: 3,
      reason: "error",
    });
    useRemotePlaybackStore.getState().reset();

    const state = useRemotePlaybackStore.getState();
    expect(state.reconnectState).toBe("idle");
    expect(state.songId).toBeNull();
    expect(state.attempt).toBe(0);
    expect(state.maxAttempts).toBe(0);
    expect(state.reason).toBeNull();
    expect(state.resyncDeltaMs).toBeNull();
  });
});
