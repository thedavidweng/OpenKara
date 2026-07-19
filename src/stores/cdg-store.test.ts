import { beforeEach, describe, expect, test } from "vitest";
import { useCdgStore } from "./cdg-store";

describe("cdg-store", () => {
  beforeEach(() => {
    useCdgStore.setState({
      hasCdg: false,
      songId: null,
      availability: "none",
      errorCode: null,
      frameVersion: 0,
      transportGeneration: 0,
    });
  });

  test("initial state has hasCdg=false and songId=null", () => {
    const state = useCdgStore.getState();
    expect(state.hasCdg).toBe(false);
    expect(state.songId).toBeNull();
    expect(state.availability).toBe("none");
    expect(state.errorCode).toBeNull();
    expect(state.frameVersion).toBe(0);
    expect(state.transportGeneration).toBe(0);
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

  test("setStatus sets availability and errorCode", () => {
    useCdgStore.getState().setStatus("loading", null);
    expect(useCdgStore.getState().availability).toBe("loading");
    expect(useCdgStore.getState().errorCode).toBeNull();

    useCdgStore.getState().setStatus("ready", null);
    expect(useCdgStore.getState().availability).toBe("ready");

    useCdgStore.getState().setStatus("error", "invalid");
    expect(useCdgStore.getState().availability).toBe("error");
    expect(useCdgStore.getState().errorCode).toBe("invalid");
  });

  test("setFrameVersion sets frameVersion and transportGeneration", () => {
    useCdgStore.getState().setFrameVersion(42, 7);
    expect(useCdgStore.getState().frameVersion).toBe(42);
    expect(useCdgStore.getState().transportGeneration).toBe(7);
  });

  test("clear resets to hasCdg=false and songId=null", () => {
    useCdgStore.getState().setSong("song-123", true);
    useCdgStore.getState().setStatus("ready", null);
    useCdgStore.getState().setFrameVersion(10, 3);

    useCdgStore.getState().clear();

    const state = useCdgStore.getState();
    expect(state.hasCdg).toBe(false);
    expect(state.songId).toBeNull();
    expect(state.availability).toBe("none");
    expect(state.errorCode).toBeNull();
    expect(state.frameVersion).toBe(0);
    expect(state.transportGeneration).toBe(0);
  });

  test("clear is idempotent when already cleared", () => {
    useCdgStore.getState().clear();

    const state = useCdgStore.getState();
    expect(state.hasCdg).toBe(false);
    expect(state.songId).toBeNull();
  });
});
