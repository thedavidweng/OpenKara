// @vitest-environment jsdom

import { beforeEach, describe, expect, test, vi } from "vitest";
import { renderHook as renderHookRaw, act } from "@testing-library/react";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { withBackend } from "@/test-utils/backend";
import { useKeyboardShortcuts } from "./use-keyboard-shortcuts";

const mockBatchSeparate = vi.fn().mockResolvedValue(undefined);
const backend = createMockBackend({
  overrides: { maintenance: { batchSeparate: mockBatchSeparate } },
});

function renderHook(hook: () => void) {
  return renderHookRaw(hook, { wrapper: withBackend(backend) });
}

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: {
    getByLabel: vi.fn(),
  },
}));

vi.mock("@/lib/fullscreen-player", () => ({
  openFullscreenPlayer: vi.fn(),
  closeFullscreenPlayer: vi.fn(),
}));

vi.mock("@/runtime/menu-runtime", () => ({
  promptImportFiles: vi.fn(),
}));

vi.mock("@/lib/song-media", () => ({
  songCanBeSeparated: vi.fn(() => true),
}));

vi.mock("@/stores/player-store", () => {
  const state = {
    snapshot: null as never,
    resume: vi.fn(),
    pause: vi.fn(),
    seek: vi.fn(),
    setVolume: vi.fn(),
  };
  return {
    usePlayerStore: {
      getState: () => state,
      setState: (patch: Partial<typeof state>) => Object.assign(state, patch),
    },
  };
});

vi.mock("@/stores/library-store", () => {
  const state = {
    songs: [] as never[],
    importFiles: vi.fn(),
  };
  return {
    useLibraryStore: {
      getState: () => state,
      setState: (patch: Partial<typeof state>) => Object.assign(state, patch),
    },
  };
});

vi.mock("@/stores/settings-store", () => {
  const state = {
    toggle: vi.fn(),
  };
  return {
    useSettingsStore: {
      getState: () => state,
      setState: (patch: Partial<typeof state>) => Object.assign(state, patch),
    },
  };
});

vi.mock("@/stores/layout-store", () => {
  const state = {
    toggleSidebar: vi.fn(),
  };
  return {
    useLayoutStore: {
      getState: () => state,
      setState: (patch: Partial<typeof state>) => Object.assign(state, patch),
    },
  };
});

vi.mock("@/stores/queue-store", () => {
  const state = {
    togglePanel: vi.fn(),
  };
  return {
    useQueueStore: {
      getState: () => state,
      setState: (patch: Partial<typeof state>) => Object.assign(state, patch),
    },
  };
});

import { handleAppKeyDown } from "./use-keyboard-shortcuts";
import { usePlayerStore } from "@/stores/player-store";
import { useLibraryStore } from "@/stores/library-store";
import { useSettingsStore } from "@/stores/settings-store";
import { useLayoutStore } from "@/stores/layout-store";
import { useQueueStore } from "@/stores/queue-store";

function createKeyboardTarget(
  tagName: string,
  isContentEditable = false,
): EventTarget & { tagName: string; isContentEditable: boolean } {
  return { tagName, isContentEditable } as EventTarget & {
    tagName: string;
    isContentEditable: boolean;
  };
}

function createKeyboardEvent(
  overrides: Partial<KeyboardEvent> & Pick<KeyboardEvent, "code" | "key">,
): KeyboardEvent {
  return {
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    preventDefault: vi.fn(),
    target: createKeyboardTarget("DIV"),
    ...overrides,
  } as KeyboardEvent;
}

function baseDeps(
  overrides: Partial<Parameters<typeof handleAppKeyDown>[1]> = {},
): Parameters<typeof handleAppKeyDown>[1] {
  const { player: playerOverride, ...rest } = overrides;
  return {
    openImportDialog: vi.fn(),
    toggleSettings: vi.fn(),
    toggleSidebar: vi.fn(),
    toggleQueue: vi.fn(),
    toggleMute: vi.fn(),
    toggleFullscreen: vi.fn(),
    stopPlayback: vi.fn(),
    separateCurrent: vi.fn(),
    ...rest,
    player: {
      snapshot: null,
      pause: vi.fn(),
      resume: vi.fn(),
      seek: vi.fn(),
      setVolume: vi.fn(),
      ...playerOverride,
    },
  };
}

describe("handleAppKeyDown", () => {
  test("toggles the sidebar with the shared primary shortcut", () => {
    const toggleSidebar = vi.fn();
    const event = createKeyboardEvent({
      code: "KeyB",
      key: "b",
      metaKey: true,
    });

    const handled = handleAppKeyDown(event, baseDeps({ toggleSidebar }));

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(toggleSidebar).toHaveBeenCalledOnce();
  });

  test("ignores the sidebar shortcut while typing in an input", () => {
    const toggleSidebar = vi.fn();
    const event = createKeyboardEvent({
      code: "KeyB",
      key: "b",
      ctrlKey: true,
      target: createKeyboardTarget("INPUT"),
    });

    const handled = handleAppKeyDown(event, baseDeps({ toggleSidebar }));

    expect(handled).toBe(false);
    expect(toggleSidebar).not.toHaveBeenCalled();
  });

  test("does not handle lyrics font shortcuts", () => {
    const event = createKeyboardEvent({
      code: "Equal",
      key: "+",
      metaKey: true,
      shiftKey: true,
    });

    const handled = handleAppKeyDown(event, baseDeps());

    expect(handled).toBe(false);
  });

  test("does not handle plain-text lyrics page shortcuts", () => {
    const event = createKeyboardEvent({
      code: "PageUp",
      key: "PageUp",
    });

    const handled = handleAppKeyDown(event, baseDeps());

    expect(handled).toBe(false);
  });

  test("opens the import dialog with the shared primary shortcut", () => {
    const openImportDialog = vi.fn();
    const event = createKeyboardEvent({
      code: "KeyO",
      key: "o",
      metaKey: true,
    });

    const handled = handleAppKeyDown(event, baseDeps({ openImportDialog }));

    expect(handled).toBe(true);
    expect(openImportDialog).toHaveBeenCalledOnce();
  });

  test("ignores the import shortcut while typing in an input", () => {
    const openImportDialog = vi.fn();
    const event = createKeyboardEvent({
      code: "KeyO",
      key: "o",
      ctrlKey: true,
      target: createKeyboardTarget("INPUT"),
    });

    const handled = handleAppKeyDown(event, baseDeps({ openImportDialog }));

    expect(handled).toBe(false);
    expect(openImportDialog).not.toHaveBeenCalled();
  });

  test("toggles play/pause with Space", () => {
    const pause = vi.fn();
    const event = createKeyboardEvent({
      code: "Space",
      key: " ",
    });

    const handled = handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: { is_playing: true, song_id: "abc", volume: 1 } as never,
          pause,
          resume: vi.fn(),
          setVolume: vi.fn(),
          seek: vi.fn(),
        },
      }),
    );

    expect(handled).toBe(true);
    expect(pause).toHaveBeenCalledOnce();
  });

  test("resumes with Space when paused and a song is loaded", () => {
    const resume = vi.fn();
    const event = createKeyboardEvent({
      code: "Space",
      key: " ",
    });

    const handled = handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: { is_playing: false, song_id: "abc", volume: 1 } as never,
          pause: vi.fn(),
          resume,
          setVolume: vi.fn(),
          seek: vi.fn(),
        },
      }),
    );

    expect(handled).toBe(true);
    expect(resume).toHaveBeenCalledOnce();
  });

  test("does not dispatch transport with Space while a track is loading", () => {
    const pause = vi.fn();
    const resume = vi.fn();
    const event = createKeyboardEvent({
      code: "Space",
      key: " ",
    });

    const handled = handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: {
            is_playing: false,
            song_id: "abc",
            state: "loading",
            volume: 1,
          } as never,
          pause,
          resume,
          setVolume: vi.fn(),
          seek: vi.fn(),
        },
      }),
    );

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(pause).not.toHaveBeenCalled();
    expect(resume).not.toHaveBeenCalled();
  });

  test("does not seek with plain ArrowLeft or ArrowRight", () => {
    const left = createKeyboardEvent({
      code: "ArrowLeft",
      key: "ArrowLeft",
    });
    const right = createKeyboardEvent({
      code: "ArrowRight",
      key: "ArrowRight",
    });

    expect(handleAppKeyDown(left, baseDeps())).toBe(false);
    expect(handleAppKeyDown(right, baseDeps())).toBe(false);
    expect(left.preventDefault).not.toHaveBeenCalled();
    expect(right.preventDefault).not.toHaveBeenCalled();
  });

  test("seeks backward with Ctrl+ArrowLeft", () => {
    const seek = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowLeft",
      key: "ArrowLeft",
      ctrlKey: true,
    });

    const handled = handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: {
            song_id: "abc",
            position_ms: 12_000,
            duration_ms: 120_000,
          } as never,
          pause: vi.fn(),
          resume: vi.fn(),
          seek,
          setVolume: vi.fn(),
        },
      }),
    );

    expect(handled).toBe(true);
    expect(seek).toHaveBeenCalledWith(7_000);
  });

  test("seeks forward with Ctrl+ArrowRight", () => {
    const seek = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowRight",
      key: "ArrowRight",
      ctrlKey: true,
    });

    const handled = handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: {
            song_id: "abc",
            position_ms: 12_000,
            duration_ms: 120_000,
          } as never,
          pause: vi.fn(),
          resume: vi.fn(),
          seek,
          setVolume: vi.fn(),
        },
      }),
    );

    expect(handled).toBe(true);
    expect(seek).toHaveBeenCalledWith(17_000);
  });

  test("increases master volume by 0.05 with ArrowUp, capped at 1", () => {
    const setVolume = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowUp",
      key: "ArrowUp",
    });

    const handled = handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: { is_playing: true, song_id: "abc", volume: 0.97 } as never,
          pause: vi.fn(),
          resume: vi.fn(),
          setVolume,
          seek: vi.fn(),
        },
      }),
    );

    expect(handled).toBe(true);
    expect(setVolume).toHaveBeenCalledWith(1);
  });

  test("defaults volume to 1 when snapshot has no volume field for ArrowUp", () => {
    const setVolume = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowUp",
      key: "ArrowUp",
    });

    handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: { is_playing: true, song_id: "abc" } as never,
          pause: vi.fn(),
          resume: vi.fn(),
          setVolume,
          seek: vi.fn(),
        },
      }),
    );

    expect(setVolume).toHaveBeenCalledWith(1);
  });

  test("decreases master volume by 0.05 with ArrowDown, floored at 0", () => {
    const setVolume = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowDown",
      key: "ArrowDown",
    });

    const handled = handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: { is_playing: true, song_id: "abc", volume: 0.02 } as never,
          pause: vi.fn(),
          resume: vi.fn(),
          setVolume,
          seek: vi.fn(),
        },
      }),
    );

    expect(handled).toBe(true);
    expect(setVolume).toHaveBeenCalledWith(0);
  });

  test("defaults volume to 1 when snapshot has no volume field for ArrowDown", () => {
    const setVolume = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowDown",
      key: "ArrowDown",
    });

    handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: { is_playing: true, song_id: "abc" } as never,
          pause: vi.fn(),
          resume: vi.fn(),
          setVolume,
          seek: vi.fn(),
        },
      }),
    );

    expect(setVolume).toHaveBeenCalledWith(0.95);
  });

  test("does not intercept arrows inside a dialog", () => {
    const setVolume = vi.fn();
    const dialog = {
      tagName: "DIV",
      isContentEditable: false,
      closest: (selector: string) => (selector.includes("dialog") ? {} : null),
    };
    const event = createKeyboardEvent({
      code: "ArrowUp",
      key: "ArrowUp",
      target: dialog as unknown as EventTarget,
    });

    const handled = handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: { is_playing: true, song_id: "abc", volume: 0.5 } as never,
          pause: vi.fn(),
          resume: vi.fn(),
          setVolume,
          seek: vi.fn(),
        },
      }),
    );

    expect(handled).toBe(false);
    expect(setVolume).not.toHaveBeenCalled();
  });

  test("does not intercept Space from a focused button", () => {
    const pause = vi.fn();
    const button = {
      tagName: "BUTTON",
      isContentEditable: false,
      closest: (selector: string) => (selector.includes("button") ? {} : null),
    };
    const event = createKeyboardEvent({
      code: "Space",
      key: " ",
      target: button as unknown as EventTarget,
    });

    const handled = handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: { is_playing: true, song_id: "abc", volume: 1 } as never,
          pause,
          resume: vi.fn(),
          setVolume: vi.fn(),
          seek: vi.fn(),
        },
      }),
    );

    expect(handled).toBe(false);
    expect(pause).not.toHaveBeenCalled();
  });

  test("does not intercept slider arrows as global volume changes", () => {
    const setVolume = vi.fn();
    const slider = {
      tagName: "DIV",
      isContentEditable: false,
      closest: (selector: string) =>
        selector.includes('[role="slider"]') ? {} : null,
    };
    const event = createKeyboardEvent({
      code: "ArrowUp",
      key: "ArrowUp",
      target: slider as unknown as EventTarget,
    });

    const handled = handleAppKeyDown(
      event,
      baseDeps({
        player: {
          snapshot: { is_playing: true, song_id: "abc", volume: 0.5 } as never,
          pause: vi.fn(),
          resume: vi.fn(),
          setVolume,
          seek: vi.fn(),
        },
      }),
    );

    expect(handled).toBe(false);
    expect(setVolume).not.toHaveBeenCalled();
  });

  test("toggles the queue panel with Q", () => {
    const toggleQueue = vi.fn();
    const event = createKeyboardEvent({ code: "KeyQ", key: "q" });

    const handled = handleAppKeyDown(event, baseDeps({ toggleQueue }));

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(toggleQueue).toHaveBeenCalledOnce();
  });

  test("toggles mute with M", () => {
    const toggleMute = vi.fn();
    const event = createKeyboardEvent({ code: "KeyM", key: "m" });

    const handled = handleAppKeyDown(event, baseDeps({ toggleMute }));

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(toggleMute).toHaveBeenCalledOnce();
  });

  test("toggles fullscreen with F", () => {
    const toggleFullscreen = vi.fn();
    const event = createKeyboardEvent({ code: "KeyF", key: "f" });

    const handled = handleAppKeyDown(event, baseDeps({ toggleFullscreen }));

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(toggleFullscreen).toHaveBeenCalledOnce();
  });

  test("stops playback with Ctrl+Period", () => {
    const stopPlayback = vi.fn();
    const event = createKeyboardEvent({
      code: "Period",
      key: ".",
      ctrlKey: true,
    });

    const handled = handleAppKeyDown(event, baseDeps({ stopPlayback }));

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(stopPlayback).toHaveBeenCalledOnce();
  });

  test("triggers current-song separation with Ctrl+Shift+S", () => {
    const separateCurrent = vi.fn();
    const event = createKeyboardEvent({
      code: "KeyS",
      key: "s",
      ctrlKey: true,
      shiftKey: true,
    });

    const handled = handleAppKeyDown(event, baseDeps({ separateCurrent }));

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(separateCurrent).toHaveBeenCalledOnce();
  });

  test("does not toggle queue while typing in an input", () => {
    const toggleQueue = vi.fn();
    const event = createKeyboardEvent({
      code: "KeyQ",
      key: "q",
      target: createKeyboardTarget("INPUT"),
    });

    const handled = handleAppKeyDown(event, baseDeps({ toggleQueue }));

    expect(handled).toBe(false);
    expect(toggleQueue).not.toHaveBeenCalled();
  });
});

test("toggles settings with the primary shortcut", () => {
  const toggleSettings = vi.fn();
  const event = createKeyboardEvent({
    code: "Comma",
    key: ",",
    metaKey: true,
  });

  const handled = handleAppKeyDown(event, baseDeps({ toggleSettings }));

  expect(handled).toBe(true);
  expect(event.preventDefault).toHaveBeenCalledOnce();
  expect(toggleSettings).toHaveBeenCalledOnce();
});

describe("useKeyboardShortcuts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    usePlayerStore.setState({
      snapshot: null as never,
      resume: vi.fn(),
      pause: vi.fn(),
      seek: vi.fn(),
      setVolume: vi.fn(),
    });
    useLibraryStore.setState({
      songs: [] as never[],
      importFiles: vi.fn(),
    });
    useSettingsStore.setState({ toggle: vi.fn() });
    useLayoutStore.setState({ toggleSidebar: vi.fn() });
    useQueueStore.setState({ togglePanel: vi.fn() });
  });

  function dispatchKey(options: KeyboardEventInit) {
    return act(() => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, ...options }),
      );
    });
  }

  test("registers and removes the keydown listener", () => {
    const addSpy = vi.spyOn(window, "addEventListener");
    const removeSpy = vi.spyOn(window, "removeEventListener");

    const { unmount } = renderHook(() => useKeyboardShortcuts(true));

    expect(addSpy).toHaveBeenCalledWith("keydown", expect.any(Function));

    unmount();

    expect(removeSpy).toHaveBeenCalledWith("keydown", expect.any(Function));

    addSpy.mockRestore();
    removeSpy.mockRestore();
  });

  test("does not register listener when disabled", () => {
    const addSpy = vi.spyOn(window, "addEventListener");
    renderHook(() => useKeyboardShortcuts(false));
    expect(addSpy).not.toHaveBeenCalledWith("keydown", expect.any(Function));
    addSpy.mockRestore();
  });

  test("exposes all wired callbacks via the keyboard hook", async () => {
    const { unmount } = renderHook(() => useKeyboardShortcuts(true));

    usePlayerStore.setState({
      snapshot: {
        song_id: "abc",
        is_playing: true,
        volume: 0.5,
      } as never,
    });
    useLibraryStore.setState({
      songs: [{ hash: "abc" }] as never[],
      importFiles: vi.fn(),
    });

    await dispatchKey({ code: "Comma", key: ",", metaKey: true });
    expect(useSettingsStore.getState().toggle).toHaveBeenCalledOnce();

    await dispatchKey({ code: "KeyB", key: "b", metaKey: true });
    expect(useLayoutStore.getState().toggleSidebar).toHaveBeenCalledOnce();

    await dispatchKey({ code: "KeyO", key: "o", metaKey: true });
    // promptImportFiles receives an object containing library.importFiles
    const promptImportFiles = (await import("@/runtime/menu-runtime"))
      .promptImportFiles as ReturnType<typeof vi.fn>;
    expect(promptImportFiles).toHaveBeenCalledOnce();

    await dispatchKey({ code: "KeyQ", key: "q" });
    expect(useQueueStore.getState().togglePanel).toHaveBeenCalledOnce();

    await dispatchKey({ code: "Space", key: " " });
    expect(usePlayerStore.getState().pause).toHaveBeenCalledOnce();

    usePlayerStore.setState({
      snapshot: {
        song_id: "abc",
        is_playing: false,
        volume: 0.5,
      } as never,
    });
    await dispatchKey({ code: "Space", key: " " });
    expect(usePlayerStore.getState().resume).toHaveBeenCalledOnce();

    usePlayerStore.setState({
      snapshot: { song_id: "abc", is_playing: true, volume: 0.97 } as never,
    });
    await dispatchKey({ code: "ArrowUp", key: "ArrowUp" });
    expect(usePlayerStore.getState().setVolume).toHaveBeenLastCalledWith(1);

    usePlayerStore.setState({
      snapshot: { song_id: "abc", is_playing: true, volume: 0.02 } as never,
    });
    await dispatchKey({ code: "ArrowDown", key: "ArrowDown" });
    expect(usePlayerStore.getState().setVolume).toHaveBeenLastCalledWith(0);

    await dispatchKey({ code: "Period", key: ".", ctrlKey: true });
    expect(usePlayerStore.getState().pause).toHaveBeenCalledTimes(2);
    expect(usePlayerStore.getState().seek).toHaveBeenLastCalledWith(0);

    await dispatchKey({
      code: "KeyS",
      key: "s",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(mockBatchSeparate).toHaveBeenCalledWith(["abc"]);

    usePlayerStore.setState({
      snapshot: {
        song_id: "abc",
        is_playing: true,
        volume: 0.5,
      } as never,
    });
    await dispatchKey({ code: "KeyM", key: "m" });
    expect(usePlayerStore.getState().setVolume).toHaveBeenLastCalledWith(0);

    usePlayerStore.setState({
      snapshot: {
        song_id: "abc",
        is_playing: true,
        volume: 0,
      } as never,
    });
    await dispatchKey({ code: "KeyM", key: "m" });
    expect(usePlayerStore.getState().setVolume).toHaveBeenLastCalledWith(0.5);

    const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    const openFullscreenPlayer = (await import("@/lib/fullscreen-player"))
      .openFullscreenPlayer as ReturnType<typeof vi.fn>;
    const closeFullscreenPlayer = (await import("@/lib/fullscreen-player"))
      .closeFullscreenPlayer as ReturnType<typeof vi.fn>;

    vi.mocked(WebviewWindow.getByLabel).mockResolvedValueOnce(null);
    await dispatchKey({ code: "KeyF", key: "f" });
    await vi.waitFor(() => {
      expect(openFullscreenPlayer).toHaveBeenCalledOnce();
    });

    vi.mocked(WebviewWindow.getByLabel).mockResolvedValueOnce({} as never);
    await dispatchKey({ code: "KeyF", key: "f" });
    await vi.waitFor(() => {
      expect(closeFullscreenPlayer).toHaveBeenCalledOnce();
    });

    unmount();
  });
});
