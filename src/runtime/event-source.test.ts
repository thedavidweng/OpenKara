import { describe, expect, it } from "vitest";
import {
  createRecordingRuntimeEventSource,
  eventSubscription,
} from "@/runtime/event-source";

describe("runtime event source", () => {
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
