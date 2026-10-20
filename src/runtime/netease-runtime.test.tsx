// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const { listeners, notifyError } = vi.hoisted(() => ({
  listeners: new Map<string, Array<(event: { payload: unknown }) => void>>(),
  notifyError: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (
    name: string,
    handler: (event: { payload: unknown }) => void,
  ) => {
    listeners.set(name, [...(listeners.get(name) ?? []), handler]);
    return () => {
      listeners.set(
        name,
        (listeners.get(name) ?? []).filter((h) => h !== handler),
      );
    };
  },
}));
vi.mock("@/lib/errors", () => ({ notifyError }));

import { useCatalogStore } from "@/stores/catalog-store";
import { useLibraryStore } from "@/stores/library-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import { useSettingsStore } from "@/stores/settings-store";
import { startNeteaseRuntime, useNeteaseRuntime } from "./netease-runtime";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("netease runtime", () => {
  beforeEach(() => {
    listeners.clear();
    notifyError.mockReset();
    useCatalogStore.setState({
      signInError: null,
      session: null,
      loadSession: vi.fn(),
    });
    useSettingsStore.setState({ neteaseSourceEnabled: true });
  });
  afterEach(() => {
    useSettingsStore.setState({ neteaseSourceEnabled: false });
  });

  test("registers each listener once even when started repeatedly", async () => {
    const stopA = startNeteaseRuntime();
    const stopB = startNeteaseRuntime();
    await flush();
    expect(listeners.get("netease-sign-in-error")).toHaveLength(1);
    expect(listeners.get("netease-sign-in-complete")).toHaveLength(1);
    stopA();
    stopB();
    expect(listeners.get("netease-sign-in-error")).toHaveLength(0);
  });

  test("stores the latest sign-in error and accepts completed sessions", async () => {
    const stop = startNeteaseRuntime();
    await flush();
    const error = { code: "internal", message: "boom", retryable: false };
    listeners.get("netease-sign-in-error")?.[0]({ payload: error });
    listeners.get("netease-sign-in-error")?.[0]({ payload: error });
    expect(useCatalogStore.getState().signInError).toEqual(error);
    expect(notifyError).not.toHaveBeenCalled();

    listeners.get("netease-sign-in-complete")?.[0]({
      payload: {
        source_id: "netease",
        signed_in: true,
        display_name: "Ada",
        expired: false,
      },
    });
    expect(useCatalogStore.getState().session?.display_name).toBe("Ada");
    expect(useCatalogStore.getState().signInError).toBeNull();
    stop();
  });

  test("hook loads the session and subscribes only when the source is enabled", async () => {
    const loadSession = vi.fn();
    useCatalogStore.setState({ loadSession });
    useSettingsStore.setState({ neteaseSourceEnabled: false });
    const off = renderHook(() => useNeteaseRuntime(true));
    await flush();
    expect(loadSession).not.toHaveBeenCalled();
    off.unmount();

    useSettingsStore.setState({ neteaseSourceEnabled: true });
    const on = renderHook(() => useNeteaseRuntime(true));
    await flush();
    expect(loadSession).toHaveBeenCalledOnce();
    expect(listeners.get("netease-sign-in-error")).toHaveLength(1);
    on.unmount();
    expect(listeners.get("netease-sign-in-error")).toHaveLength(0);
  });

  test("reloads the library and playlists once after imported events", async () => {
    vi.useFakeTimers();
    const loadLibrary = vi.fn();
    const loadPlaylists = vi.fn();
    useLibraryStore.setState({ loadLibrary });
    usePlaylistStore.setState({ loadPlaylists });
    const stop = startNeteaseRuntime();
    await vi.advanceTimersByTimeAsync(0);
    const emit = (kind: string) =>
      listeners.get("streaming-import-progress")?.[0]({
        payload: {
          job_id: "j",
          remote_track_id: "1",
          title: "t",
          artist: "a",
          total: 3,
          completed: 1,
          state: { kind },
        },
      });
    emit("downloading");
    await vi.advanceTimersByTimeAsync(1000);
    expect(loadLibrary).not.toHaveBeenCalled();

    emit("imported");
    emit("imported");
    emit("imported");
    await vi.advanceTimersByTimeAsync(1000);
    expect(loadLibrary).toHaveBeenCalledTimes(1);
    expect(loadPlaylists).toHaveBeenCalledTimes(1);
    stop();
    vi.useRealTimers();
  });
});
