import { describe, expect, test, vi } from "vitest";
import { handleAppKeyDown } from "./use-keyboard-shortcuts";

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
