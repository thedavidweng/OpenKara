import { describe, expect, it, vi } from "vitest";
import {
  createRecordingRuntimeEventSource,
  eventSubscription,
  tauriRuntimeEventSource,
} from "@/runtime/event-source";

const { mockListen } = vi.hoisted(() => ({
  mockListen: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: mockListen,
}));

describe("runtime event source", () => {
  it("unwraps Tauri event payloads and returns its unlistener", async () => {
    const unlisten = vi.fn();
    const handler = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const returnedUnlisten = await tauriRuntimeEventSource.listen(
      "playback-ended",
      handler,
    );
    const callback = mockListen.mock.calls[0]?.[1] as
      | ((event: { payload: { song_id: string } }) => void)
      | undefined;
    callback?.({ payload: { song_id: "song-1" } });

    expect(mockListen).toHaveBeenCalledWith(
      "playback-ended",
      expect.any(Function),
    );
    expect(handler).toHaveBeenCalledWith({ song_id: "song-1" });
    expect(returnedUnlisten).toBe(unlisten);
  });

  it("removes a handler without affecting later listeners", async () => {
    const source = createRecordingRuntimeEventSource();
    const received: string[] = [];

    const firstStop = await source.listen("playback-ended", ({ song_id }) => {
      received.push(`first:${song_id}`);
    });
    await source.listen("playback-ended", ({ song_id }) => {
      received.push(`second:${song_id}`);
    });

    source.emit("playback-ended", { song_id: "song-1" });
    firstStop();
    source.emit("playback-ended", { song_id: "song-2" });

    expect(received).toEqual([
      "first:song-1",
      "second:song-1",
      "second:song-2",
    ]);
  });

  it("does not invoke a handler after a subscription is stopped", async () => {
    const source = createRecordingRuntimeEventSource();
    const received: string[] = [];
    const subscription = eventSubscription(
      "playback-ended",
      ({ song_id }) => received.push(song_id),
      source,
    );

    const stop = await subscription.subscribe();
    stop();
    source.emit("playback-ended", { song_id: "song-1" });

    expect(received).toEqual([]);
  });
});
