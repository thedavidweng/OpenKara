import { beforeEach, describe, expect, test } from "vitest";
import { createWebviewSyncChannel } from "@/runtime/webview-sync";
import { createQueueStore } from "./queue-store";

interface FakeChannel {
  onmessage: ((event: { data: unknown }) => void) | null;
  postMessage: (data: unknown) => void;
  close: () => void;
}

describe("queue-store", () => {
  let store: ReturnType<typeof createQueueStore>;

  beforeEach(() => {
    store = createQueueStore();
    store.store.setState({ queue: [], playHistory: [], isOpen: false });
  });

  // ── addToQueue ────────────────────────────────────────────────────────────

  test("addToQueue appends a song to the end of the queue", () => {
    store.store.getState().addToQueue("song-a");
    store.store.getState().addToQueue("song-b");
    expect(store.store.getState().queue).toEqual(["song-a", "song-b"]);
  });

  test("addToQueue is a no-op when songId is already in the queue", () => {
    store.store.setState({ queue: ["song-a", "song-b"] });
    store.store.getState().addToQueue("song-a");
    expect(store.store.getState().queue).toEqual(["song-a", "song-b"]);
  });

  // ── playNext ──────────────────────────────────────────────────────────────

  test("playNext prepends a new song to the front of the queue", () => {
    store.store.setState({ queue: ["song-a", "song-b"] });
    store.store.getState().playNext("song-c");
    expect(store.store.getState().queue).toEqual([
      "song-c",
      "song-a",
      "song-b",
    ]);
  });

  test("playNext moves an existing song to the front of the queue", () => {
    store.store.setState({ queue: ["song-a", "song-b", "song-c"] });
    store.store.getState().playNext("song-c");
    expect(store.store.getState().queue).toEqual([
      "song-c",
      "song-a",
      "song-b",
    ]);
  });

  test("playNext on an empty queue creates a single-item queue", () => {
    store.store.getState().playNext("song-a");
    expect(store.store.getState().queue).toEqual(["song-a"]);
  });

  // ── removeFromQueue ───────────────────────────────────────────────────────

  test("removeFromQueue removes a song by index", () => {
    store.store.setState({ queue: ["song-a", "song-b", "song-c"] });
    store.store.getState().removeFromQueue(1);
    expect(store.store.getState().queue).toEqual(["song-a", "song-c"]);
  });

  test("removeFromQueue with out-of-range index is a no-op", () => {
    store.store.setState({ queue: ["song-a"] });
    store.store.getState().removeFromQueue(5);
    expect(store.store.getState().queue).toEqual(["song-a"]);
  });

  // ── removeSongIds ─────────────────────────────────────────────────────────

  test("removeSongIds removes multiple songs by id", () => {
    store.store.setState({ queue: ["song-a", "song-b", "song-c", "song-d"] });
    store.store.getState().removeSongIds(["song-b", "song-d"]);
    expect(store.store.getState().queue).toEqual(["song-a", "song-c"]);
  });

  test("removeSongIds is a no-op when no ids match", () => {
    store.store.setState({ queue: ["song-a", "song-b"] });
    store.store.getState().removeSongIds(["song-x"]);
    expect(store.store.getState().queue).toEqual(["song-a", "song-b"]);
  });

  test("removeSongIds handles empty ids array", () => {
    store.store.setState({ queue: ["song-a"] });
    store.store.getState().removeSongIds([]);
    expect(store.store.getState().queue).toEqual(["song-a"]);
  });

  // ── reorder ───────────────────────────────────────────────────────────────

  test("reorder moves a song from one position to another", () => {
    store.store.setState({ queue: ["song-a", "song-b", "song-c"] });
    store.store.getState().reorder(0, 2);
    expect(store.store.getState().queue).toEqual([
      "song-b",
      "song-c",
      "song-a",
    ]);
  });

  test("reorder is a no-op when fromIndex equals toIndex", () => {
    store.store.setState({ queue: ["song-a", "song-b"] });
    store.store.getState().reorder(1, 1);
    expect(store.store.getState().queue).toEqual(["song-a", "song-b"]);
  });

  test("reorder is a no-op when indices are out of range", () => {
    store.store.setState({ queue: ["song-a"] });
    store.store.getState().reorder(-1, 0);
    expect(store.store.getState().queue).toEqual(["song-a"]);
    store.store.getState().reorder(0, 5);
    expect(store.store.getState().queue).toEqual(["song-a"]);
  });

  // ── reorderBySongId ───────────────────────────────────────────────────────

  test("reorderBySongId moves the dragged song before the hovered song", () => {
    store.store.setState({ queue: ["song-a", "song-b", "song-c"] });
    store.store.getState().reorderBySongId("song-c", "song-a");
    expect(store.store.getState().queue).toEqual([
      "song-c",
      "song-a",
      "song-b",
    ]);
  });

  test("reorderBySongId moves the dragged song after the hovered song when dragging downward", () => {
    store.store.setState({ queue: ["song-a", "song-b", "song-c"] });
    store.store.getState().reorderBySongId("song-a", "song-c");
    expect(store.store.getState().queue).toEqual([
      "song-b",
      "song-c",
      "song-a",
    ]);
  });

  test("reorderBySongId is a no-op when one of the songs is missing", () => {
    store.store.setState({ queue: ["song-a", "song-b", "song-c"] });
    store.store.getState().reorderBySongId("song-x", "song-a");
    expect(store.store.getState().queue).toEqual([
      "song-a",
      "song-b",
      "song-c",
    ]);
  });

  test("reorderBySongId is a no-op when both ids are the same", () => {
    store.store.setState({ queue: ["song-a", "song-b"] });
    store.store.getState().reorderBySongId("song-a", "song-a");
    expect(store.store.getState().queue).toEqual(["song-a", "song-b"]);
  });

  // ── clearQueue ────────────────────────────────────────────────────────────

  test("clearQueue empties the queue", () => {
    store.store.setState({ queue: ["song-a", "song-b"] });
    store.store.getState().clearQueue();
    expect(store.store.getState().queue).toEqual([]);
  });

  // ── setQueue ──────────────────────────────────────────────────────────────

  test("setQueue replaces the entire queue", () => {
    store.store.setState({ queue: ["song-a"] });
    store.store.getState().setQueue(["song-x", "song-y", "song-z"]);
    expect(store.store.getState().queue).toEqual([
      "song-x",
      "song-y",
      "song-z",
    ]);
  });

  test("setQueue with empty array empties the queue", () => {
    store.store.setState({ queue: ["song-a"] });
    store.store.getState().setQueue([]);
    expect(store.store.getState().queue).toEqual([]);
  });

  // ── dequeue ───────────────────────────────────────────────────────────────

  test("dequeue returns the first song and removes it from the queue", () => {
    store.store.setState({ queue: ["song-a", "song-b", "song-c"] });
    const first = store.store.getState().dequeue();
    expect(first).toBe("song-a");
    expect(store.store.getState().queue).toEqual(["song-b", "song-c"]);
  });

  test("dequeue returns undefined when the queue is empty", () => {
    const result = store.store.getState().dequeue();
    expect(result).toBeUndefined();
    expect(store.store.getState().queue).toEqual([]);
  });

  test("dequeue returns the only item and leaves the queue empty", () => {
    store.store.setState({ queue: ["song-a"] });
    const result = store.store.getState().dequeue();
    expect(result).toBe("song-a");
    expect(store.store.getState().queue).toEqual([]);
  });

  // ── pushToHistory ─────────────────────────────────────────────────────────

  test("pushToHistory appends a song to the history", () => {
    store.store.getState().pushToHistory("song-1");
    store.store.getState().pushToHistory("song-2");
    expect(store.store.getState().playHistory).toEqual(["song-1", "song-2"]);
  });

  test("pushToHistory deduplicates by moving the song to the end", () => {
    store.store.setState({ playHistory: ["song-1", "song-2", "song-3"] });
    store.store.getState().pushToHistory("song-1");
    expect(store.store.getState().playHistory).toEqual([
      "song-2",
      "song-3",
      "song-1",
    ]);
  });

  test("pushToHistory caps history at 500 entries", () => {
    const history = Array.from({ length: 500 }, (_, i) => `song-${i}`);
    store.store.setState({ playHistory: history });
    store.store.getState().pushToHistory("song-new");
    const result = store.store.getState().playHistory;
    expect(result).toHaveLength(500);
    expect(result[0]).toBe("song-1");
    expect(result[499]).toBe("song-new");
  });

  test("pushToHistory does not affect the queue", () => {
    store.store.setState({ queue: ["song-a"], playHistory: [] });
    store.store.getState().pushToHistory("song-x");
    expect(store.store.getState().queue).toEqual(["song-a"]);
  });

  // ── popFromHistory ────────────────────────────────────────────────────────

  test("popFromHistory returns the last history entry and removes it", () => {
    store.store.setState({ playHistory: ["song-1", "song-2"] });
    const result = store.store.getState().popFromHistory();
    expect(result).toBe("song-2");
    expect(store.store.getState().playHistory).toEqual(["song-1"]);
  });

  test("popFromHistory returns undefined when history is empty", () => {
    const result = store.store.getState().popFromHistory();
    expect(result).toBeUndefined();
  });

  test("popFromHistory returns the only item and leaves history empty", () => {
    store.store.setState({ playHistory: ["song-1"] });
    const result = store.store.getState().popFromHistory();
    expect(result).toBe("song-1");
    expect(store.store.getState().playHistory).toEqual([]);
  });

  // ── clearHistory ──────────────────────────────────────────────────────────

  test("clearHistory empties the play history", () => {
    store.store.setState({ playHistory: ["song-1", "song-2"] });
    store.store.getState().clearHistory();
    expect(store.store.getState().playHistory).toEqual([]);
  });

  test("clearHistory does not affect the queue", () => {
    store.store.setState({ queue: ["song-a"], playHistory: ["song-b"] });
    store.store.getState().clearHistory();
    expect(store.store.getState().queue).toEqual(["song-a"]);
    expect(store.store.getState().playHistory).toEqual([]);
  });

  // ── togglePanel ───────────────────────────────────────────────────────────

  test("togglePanel flips isOpen from false to true", () => {
    store.store.setState({ isOpen: false });
    store.store.getState().togglePanel();
    expect(store.store.getState().isOpen).toBe(true);
  });

  test("togglePanel flips isOpen from true to false", () => {
    store.store.setState({ isOpen: true });
    store.store.getState().togglePanel();
    expect(store.store.getState().isOpen).toBe(false);
  });

  // ── dispose ───────────────────────────────────────────────────────────────

  test("dispose closes the sync channel", () => {
    const closed = { value: false };
    const channel = {
      onmessage: null as ((event: { data: unknown }) => void) | null,
      postMessage: () => {},
      close: () => {
        closed.value = true;
      },
    };
    const instance = createQueueStore(
      createWebviewSyncChannel("test-dispose", {
        channelFactory: () => channel,
        originId: "test",
      }),
    );
    instance.dispose();
    expect(closed.value).toBe(true);
  });

  // ── reconcileGaplessTransition (#88) ──────────────────────────────────────

  test("reconcileGaplessTransition removes first toSongId and pushes fromSongId to history", () => {
    store.store.setState({
      queue: ["song-b", "song-c", "song-b"],
      playHistory: ["song-x"],
    });

    store.store.getState().reconcileGaplessTransition("song-a", "song-b");

    // Only the first "song-b" is removed; the duplicate remains.
    expect(store.store.getState().queue).toEqual(["song-c", "song-b"]);
    expect(store.store.getState().playHistory).toEqual(["song-x", "song-a"]);
  });

  test("reconcileGaplessTransition pushes fromSongId to history exactly once", () => {
    store.store.setState({
      queue: ["song-b"],
      playHistory: ["song-a", "song-y"],
    });

    store.store.getState().reconcileGaplessTransition("song-a", "song-b");

    // "song-a" is deduped from history then appended once.
    expect(store.store.getState().playHistory).toEqual(["song-y", "song-a"]);
  });

  test("reconcileGaplessTransition preserves unrelated queue entries", () => {
    store.store.setState({
      queue: ["song-b", "song-d", "song-e"],
      playHistory: [],
    });

    store.store.getState().reconcileGaplessTransition("song-a", "song-b");

    expect(store.store.getState().queue).toEqual(["song-d", "song-e"]);
  });

  test("reconcileGaplessTransition with absent toSongId still pushes history", () => {
    store.store.setState({
      queue: ["song-c", "song-d"],
      playHistory: [],
    });

    store.store.getState().reconcileGaplessTransition("song-a", "song-b");

    // toSongId not in queue — queue unchanged, fromSongId still goes to history.
    expect(store.store.getState().queue).toEqual(["song-c", "song-d"]);
    expect(store.store.getState().playHistory).toEqual(["song-a"]);
  });
});

describe("queue-store webview sync", () => {
  test("syncs queue and playHistory across webview contexts", () => {
    const channelsByName = new Map<string, Set<FakeChannel>>();
    const channelFactory = (name: string) => {
      const peers = channelsByName.get(name) ?? new Set<FakeChannel>();
      channelsByName.set(name, peers);

      const channel: FakeChannel = {
        onmessage: null as ((event: { data: unknown }) => void) | null,
        postMessage(data: unknown) {
          for (const peer of peers) {
            if (peer === channel) {
              continue;
            }

            peer.onmessage?.({ data });
          }
        },
        close() {
          peers.delete(channel);
        },
      };

      peers.add(channel);
      return channel;
    };

    const primary = createQueueStore(
      createWebviewSyncChannel("queue", {
        channelFactory,
        originId: "primary",
      }),
    );
    const secondary = createQueueStore(
      createWebviewSyncChannel("queue", {
        channelFactory,
        originId: "secondary",
      }),
    );

    primary.store.getState().addToQueue("song-a");
    primary.store.getState().addToQueue("song-b");
    primary.store.getState().pushToHistory("song-0");

    expect(secondary.store.getState().queue).toEqual(["song-a", "song-b"]);
    expect(secondary.store.getState().playHistory).toEqual(["song-0"]);

    primary.dispose();
    secondary.dispose();
  });
});
