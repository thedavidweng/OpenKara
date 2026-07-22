// @vitest-environment jsdom

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { RemoteReconnectIndicator } from "./RemoteReconnectIndicator";
import { useRemotePlaybackStore } from "@/stores/remote-playback-store";
import { usePlayerStore } from "@/stores/player-store";
import type { PlaybackStateSnapshot } from "@/types/ipc";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (
        _key: string,
        opts?: { defaultValue?: string } & Record<string, unknown>,
      ) => {
        let result = opts?.defaultValue ?? "";
        if (opts) {
          for (const [k, v] of Object.entries(opts)) {
            if (k === "defaultValue") continue;
            result = result.replace(new RegExp(`{{${k}}}`, "g"), String(v));
          }
        }
        return result;
      },
    }),
  };
});

function setPlayerSongId(songId: string | null) {
  usePlayerStore.setState({
    snapshot: songId
      ? ({ song_id: songId } as unknown as PlaybackStateSnapshot)
      : null,
  });
}

describe("RemoteReconnectIndicator", () => {
  beforeEach(() => {
    useRemotePlaybackStore.getState().reset();
    setPlayerSongId(null);
  });

  afterEach(() => {
    cleanup();
    useRemotePlaybackStore.getState().reset();
  });

  test("renders nothing when state is idle", () => {
    const { container } = render(<RemoteReconnectIndicator />);
    expect(container.firstChild).toBeNull();
  });

  test("renders reconnecting badge with attempt count", () => {
    setPlayerSongId("song-1");
    useRemotePlaybackStore.getState().applyReconnectEvent({
      song_id: "song-1",
      request_id: 1,
      attempt: 2,
      max_attempts: 3,
      reason: "503",
    });

    render(<RemoteReconnectIndicator />);
    const indicator = screen.getByTestId("remote-reconnect-indicator");
    expect(indicator.getAttribute("data-reconnect-state")).toBe("reconnecting");
    expect(indicator.textContent).toContain("2/3");
  });

  test("renders resync badge with delta", () => {
    setPlayerSongId("song-1");
    useRemotePlaybackStore.getState().applyResyncEvent({
      song_id: "song-1",
      requested_position_ms: 10_000,
      actual_position_ms: 9_000,
    });

    render(<RemoteReconnectIndicator />);
    const indicator = screen.getByTestId("remote-reconnect-indicator");
    expect(indicator.getAttribute("data-reconnect-state")).toBe("resync");
    expect(indicator.textContent).toContain("1000");
  });

  test("renders failed badge", () => {
    setPlayerSongId("song-1");
    useRemotePlaybackStore.getState().applyFailedEvent({
      song_id: "song-1",
      request_id: 1,
      reason: "permanent",
    });

    render(<RemoteReconnectIndicator />);
    const indicator = screen.getByTestId("remote-reconnect-indicator");
    expect(indicator.getAttribute("data-reconnect-state")).toBe("failed");
  });

  test("resets when the active song changes", () => {
    useRemotePlaybackStore.getState().applyReconnectEvent({
      song_id: "song-1",
      request_id: 1,
      attempt: 1,
      max_attempts: 3,
      reason: "503",
    });
    setPlayerSongId("song-1");

    render(<RemoteReconnectIndicator />);
    expect(screen.getByTestId("remote-reconnect-indicator")).toBeTruthy();

    act(() => {
      setPlayerSongId("song-2");
    });

    expect(useRemotePlaybackStore.getState().reconnectState).toBe("idle");
  });

  test("resets when playback stops (song becomes null)", () => {
    useRemotePlaybackStore.getState().applyReconnectEvent({
      song_id: "song-1",
      request_id: 1,
      attempt: 1,
      max_attempts: 3,
      reason: "503",
    });
    setPlayerSongId("song-1");

    render(<RemoteReconnectIndicator />);

    act(() => {
      setPlayerSongId(null);
    });

    expect(useRemotePlaybackStore.getState().reconnectState).toBe("idle");
  });
});
