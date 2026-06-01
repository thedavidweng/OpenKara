import { beforeEach, describe, expect, test, vi } from "vitest";

const { mockSetRotationState, mockGetRotationState, mockAdvanceRotation } =
  vi.hoisted(() => ({
    mockSetRotationState: vi.fn().mockResolvedValue(undefined),
    mockGetRotationState: vi.fn(),
    mockAdvanceRotation: vi.fn(),
  }));

vi.mock("@/lib/tauri/playlist", () => ({
  setRotationState: mockSetRotationState,
  getRotationState: mockGetRotationState,
  advanceRotation: mockAdvanceRotation,
}));

import { useRotationStore } from "./rotation-store";

describe("rotation-store", () => {
  beforeEach(() => {
    mockSetRotationState.mockClear();
    useRotationStore.setState({
      active: false,
      singerNames: [],
      currentIndex: 0,
      mode: "round_robin",
      queueSingers: new Map(),
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
        currentIndex: 2, // pointing at Charlie
      });

      await useRotationStore.getState().removeSinger("Alice");

      const state = useRotationStore.getState();
      expect(state.singerNames).toEqual(["Bob", "Charlie"]);
      expect(state.currentIndex).toBe(1); // decremented
    });

    test("keeps index when removed after current", async () => {
      useRotationStore.setState({
        singerNames: ["Alice", "Bob", "Charlie"],
        currentIndex: 0, // pointing at Alice
      });

      await useRotationStore.getState().removeSinger("Charlie");

      const state = useRotationStore.getState();
      expect(state.singerNames).toEqual(["Alice", "Bob"]);
      expect(state.currentIndex).toBe(0);
    });

    test("clamps index when it overshoots", async () => {
      useRotationStore.setState({
        singerNames: ["Alice", "Bob"],
        currentIndex: 1, // pointing at Bob
      });

      await useRotationStore.getState().removeSinger("Bob");

      const state = useRotationStore.getState();
      expect(state.singerNames).toEqual(["Alice"]);
      expect(state.currentIndex).toBe(0); // clamped to last valid
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
});
