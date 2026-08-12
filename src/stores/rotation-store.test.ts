import { beforeEach, describe, expect, test, vi } from "vitest";

const { mockSetQueue, mockQueueState } = vi.hoisted(() => ({
  mockSetQueue: vi.fn(),
  mockQueueState: { queue: [] as string[] },
}));

const mockSetRotationState = vi.fn().mockResolvedValue(undefined);
const mockGetRotationState = vi.fn();
const mockAdvanceRotation = vi.fn();

vi.mock("@/stores/queue-store", () => ({
  useQueueStore: {
    getState: () => ({
      get queue() {
        return mockQueueState.queue;
      },
      setQueue: mockSetQueue,
    }),
  },
}));

import { createMockBackend } from "@/lib/backend/mock-backend";
import { createRotationStore } from "./rotation-store";

const useRotationStore = createRotationStore(
  createMockBackend({
    overrides: {
      playlist: {
        setRotationState: mockSetRotationState,
        getRotationState: mockGetRotationState,
        advanceRotation: mockAdvanceRotation,
      },
    },
  }),
);

describe("rotation-store", () => {
  beforeEach(() => {
    mockSetRotationState.mockClear();
    mockAdvanceRotation.mockReset();
    mockSetQueue.mockReset();
    mockQueueState.queue = [];
    useRotationStore.setState({
      active: false,
      singerNames: [],
      currentIndex: 0,
      mode: "round_robin",
      queueSingers: new Map(),
      filterSinger: null,
      isLoading: false,
    });
  });

  describe("getNextSinger", () => {
    test("returns null when no singers", () => {
      expect(useRotationStore.getState().getNextSinger()).toBeNull();
    });

    test("returns singer at current index", () => {
      useRotationStore.setState({
        singerNames: ["Alice", "Bob", "Charlie"],
        currentIndex: 1,
      });
      expect(useRotationStore.getState().getNextSinger()).toBe("Bob");
    });

    test("wraps around using modulo", () => {
      useRotationStore.setState({
        singerNames: ["Alice", "Bob"],
        currentIndex: 2,
      });
      expect(useRotationStore.getState().getNextSinger()).toBe("Alice");
    });
  });

  describe("addSinger", () => {
    test("adds a new singer", async () => {
      await useRotationStore.getState().addSinger("Alice");
      expect(useRotationStore.getState().singerNames).toEqual(["Alice"]);
    });

    test("trims whitespace", async () => {
      await useRotationStore.getState().addSinger("  Alice  ");
      expect(useRotationStore.getState().singerNames).toEqual(["Alice"]);
    });

    test("rejects empty name", async () => {
      await useRotationStore.getState().addSinger("   ");
      expect(useRotationStore.getState().singerNames).toEqual([]);
    });

    test("rejects duplicate name", async () => {
      useRotationStore.setState({ singerNames: ["Alice"] });
      await useRotationStore.getState().addSinger("Alice");
      expect(useRotationStore.getState().singerNames).toEqual(["Alice"]);
    });
  });

  describe("removeSinger", () => {
    test("removes singer and adjusts index when removed before current", async () => {
      useRotationStore.setState({
        singerNames: ["Alice", "Bob", "Charlie"],
        currentIndex: 2,
      });

      await useRotationStore.getState().removeSinger("Alice");

      const state = useRotationStore.getState();
      expect(state.singerNames).toEqual(["Bob", "Charlie"]);
      expect(state.currentIndex).toBe(1);
    });

    test("keeps index when removed after current", async () => {
      useRotationStore.setState({
        singerNames: ["Alice", "Bob", "Charlie"],
        currentIndex: 0,
      });

      await useRotationStore.getState().removeSinger("Charlie");

      const state = useRotationStore.getState();
      expect(state.singerNames).toEqual(["Alice", "Bob"]);
      expect(state.currentIndex).toBe(0);
    });

    test("clamps index when it overshoots", async () => {
      useRotationStore.setState({
        singerNames: ["Alice", "Bob"],
        currentIndex: 1,
      });

      await useRotationStore.getState().removeSinger("Bob");

      const state = useRotationStore.getState();
      expect(state.singerNames).toEqual(["Alice"]);
      expect(state.currentIndex).toBe(0);
    });

    test("handles removing the only singer", async () => {
      useRotationStore.setState({
        singerNames: ["Alice"],
        currentIndex: 0,
      });

      await useRotationStore.getState().removeSinger("Alice");

      const state = useRotationStore.getState();
      expect(state.singerNames).toEqual([]);
      expect(state.currentIndex).toBe(0);
    });
  });

  describe("filterSinger", () => {
    test("defaults to null (show all)", () => {
      expect(useRotationStore.getState().filterSinger).toBeNull();
    });

    test("setFilterSinger updates the filter", () => {
      useRotationStore.setState({
        singerNames: ["Alice", "Bob"],
      });
      useRotationStore.getState().setFilterSinger("Alice");
      expect(useRotationStore.getState().filterSinger).toBe("Alice");
    });

    test("setFilterSinger with null clears the filter", () => {
      useRotationStore.setState({ filterSinger: "Alice" });
      useRotationStore.getState().setFilterSinger(null);
      expect(useRotationStore.getState().filterSinger).toBeNull();
    });

    test("advanceRotation sets filter to the new current singer", async () => {
      useRotationStore.setState({
        singerNames: ["Alice", "Bob"],
        currentIndex: 0,
        active: true,
      });
      mockAdvanceRotation.mockResolvedValue({
        singer_names: ["Alice", "Bob"],
        current_index: 1,
      });
      await useRotationStore.getState().advanceRotation();
      expect(useRotationStore.getState().filterSinger).toBe("Bob");
    });

    test("removeSinger clears filter if removed singer was the filtered one", async () => {
      useRotationStore.setState({
        singerNames: ["Alice", "Bob"],
        currentIndex: 0,
        filterSinger: "Alice",
        active: true,
      });
      await useRotationStore.getState().removeSinger("Alice");
      expect(useRotationStore.getState().filterSinger).toBeNull();
    });

    test("removeSinger keeps filter if removed singer was not the filtered one", async () => {
      useRotationStore.setState({
        singerNames: ["Alice", "Bob"],
        currentIndex: 0,
        filterSinger: "Bob",
        active: true,
      });
      await useRotationStore.getState().removeSinger("Alice");
      expect(useRotationStore.getState().filterSinger).toBe("Bob");
    });
  });

  describe("assignSingerToQueueEntry", () => {
    test("assigns singer to song hash", () => {
      useRotationStore.getState().assignSingerToQueueEntry("song-1", "Alice");
      expect(useRotationStore.getState().queueSingers.get("song-1")).toBe(
        "Alice",
      );
    });

    test("removes assignment when singer is null", () => {
      useRotationStore.setState({
        queueSingers: new Map([["song-1", "Alice"]]),
      });
      useRotationStore.getState().assignSingerToQueueEntry("song-1", null);
      expect(useRotationStore.getState().queueSingers.has("song-1")).toBe(
        false,
      );
    });

    test("does not mutate previous map", () => {
      const store = useRotationStore.getState();
      store.assignSingerToQueueEntry("song-1", "Alice");

      const prevMap = useRotationStore.getState().queueSingers;
      store.assignSingerToQueueEntry("song-2", "Bob");

      expect(prevMap.has("song-2")).toBe(false);
    });
  });

  describe("shuffleQueue", () => {
    test("does nothing when queue has 0 or 1 items", () => {
      mockQueueState.queue = [];
      useRotationStore.getState().shuffleQueue();
      expect(mockSetQueue).not.toHaveBeenCalled();

      mockQueueState.queue = ["song-1"];
      useRotationStore.getState().shuffleQueue();
      expect(mockSetQueue).not.toHaveBeenCalled();
    });

    test("randomly shuffles when no singers are assigned", () => {
      mockQueueState.queue = ["a", "b", "c", "d", "e"];
      useRotationStore.setState({ queueSingers: new Map() });
      mockSetQueue.mockClear();

      useRotationStore.getState().shuffleQueue();

      expect(mockSetQueue).toHaveBeenCalledTimes(1);
      const result = mockSetQueue.mock.calls[0][0];
      expect(result).toHaveLength(5);
      expect(result.sort()).toEqual(["a", "b", "c", "d", "e"]);
    });

    test("interleaves by singer to avoid back-to-back", () => {
      mockQueueState.queue = ["a1", "a2", "a3", "b1", "b2"];
      useRotationStore.setState({
        queueSingers: new Map([
          ["a1", "Alice"],
          ["a2", "Alice"],
          ["a3", "Alice"],
          ["b1", "Bob"],
          ["b2", "Bob"],
        ]),
      });
      mockSetQueue.mockClear();

      // Run multiple times to account for randomness within groups
      let hasBackToBack = false;
      for (let run = 0; run < 20; run++) {
        mockSetQueue.mockClear();
        useRotationStore.getState().shuffleQueue();
        const result = mockSetQueue.mock.calls[0][0];

        for (let i = 1; i < result.length; i++) {
          const prevSinger = useRotationStore
            .getState()
            .queueSingers.get(result[i - 1]);
          const currSinger = useRotationStore
            .getState()
            .queueSingers.get(result[i]);
          if (prevSinger && currSinger && prevSinger === currSinger) {
            hasBackToBack = true;
            break;
          }
        }
        if (!hasBackToBack) break;
      }
      expect(hasBackToBack).toBe(false);
    });

    test("preserves all songs in shuffled result", () => {
      mockQueueState.queue = ["x", "y", "z"];
      useRotationStore.setState({
        queueSingers: new Map([
          ["x", "Alice"],
          ["y", "Bob"],
        ]),
      });
      mockSetQueue.mockClear();

      useRotationStore.getState().shuffleQueue();

      const result = mockSetQueue.mock.calls[0][0];
      expect(result.sort()).toEqual(["x", "y", "z"]);
    });

    test("repeated shuffles produce varied orderings with one song per singer (#145)", () => {
      mockQueueState.queue = ["a1", "b1", "c1", "d1"];
      useRotationStore.setState({
        queueSingers: new Map([
          ["a1", "Alice"],
          ["b1", "Bob"],
          ["c1", "Charlie"],
          ["d1", "Diana"],
        ]),
      });

      const orderings = new Set<string>();
      for (let i = 0; i < 30; i++) {
        mockSetQueue.mockClear();
        useRotationStore.getState().shuffleQueue();
        const result = mockSetQueue.mock.calls[0][0];
        orderings.add(result.join(","));
      }
      // 4! = 24 possible orders; require at least 2 distinct across 30
      // presses. The old code produced exactly 1.
      expect(orderings.size).toBeGreaterThan(1);
    });

    test("repeated shuffles still avoid back-to-back with unequal group sizes", () => {
      mockQueueState.queue = ["a1", "a2", "a3", "b1", "b2"];
      useRotationStore.setState({
        queueSingers: new Map([
          ["a1", "Alice"],
          ["a2", "Alice"],
          ["a3", "Alice"],
          ["b1", "Bob"],
          ["b2", "Bob"],
        ]),
      });

      for (let run = 0; run < 30; run++) {
        mockSetQueue.mockClear();
        useRotationStore.getState().shuffleQueue();
        const result = mockSetQueue.mock.calls[0][0];

        for (let i = 1; i < result.length; i++) {
          const prevSinger = useRotationStore
            .getState()
            .queueSingers.get(result[i - 1]);
          const currSinger = useRotationStore
            .getState()
            .queueSingers.get(result[i]);
          if (prevSinger && currSinger) {
            expect(prevSinger).not.toBe(currSinger);
          }
        }
      }
    });
  });
});
