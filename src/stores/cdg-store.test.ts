import { beforeEach, describe, expect, test } from "vitest";
import { useCdgStore } from "./cdg-store";

describe("cdg-store", () => {
  beforeEach(() => {
    useCdgStore.setState({ hasCdg: false, songId: null });
  });

  test("initial state has hasCdg=false and songId=null", () => {
    const state = useCdgStore.getState();
    expect(state.hasCdg).toBe(false);
    expect(state.songId).toBeNull();
  });

  test("setSong sets songId and hasCdg=true", () => {
    useCdgStore.getState().setSong("song-123", true);

    const state = useCdgStore.getState();
    expect(state.songId).toBe("song-123");
    expect(state.hasCdg).toBe(true);
  });

  test("setSong with hasCdg=false sets songId but hasCdg=false", () => {
    useCdgStore.getState().setSong("song-456", false);

    const state = useCdgStore.getState();
    expect(state.songId).toBe("song-456");
    expect(state.hasCdg).toBe(false);
  });

  test("setSong with null songId clears songId", () => {
    useCdgStore.getState().setSong("song-123", true);
    useCdgStore.getState().setSong(null, false);

    const state = useCdgStore.getState();
    expect(state.songId).toBeNull();
    expect(state.hasCdg).toBe(false);
  });

  test("clear resets to hasCdg=false and songId=null", () => {
    useCdgStore.getState().setSong("song-123", true);

    useCdgStore.getState().clear();

    const state = useCdgStore.getState();
    expect(state.hasCdg).toBe(false);
    expect(state.songId).toBeNull();
  });

  test("clear is idempotent when already cleared", () => {
    useCdgStore.getState().clear();

    const state = useCdgStore.getState();
    expect(state.hasCdg).toBe(false);
    expect(state.songId).toBeNull();
  });
});
