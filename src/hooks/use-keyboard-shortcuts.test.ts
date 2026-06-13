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

describe("handleAppKeyDown", () => {
  test("toggles the sidebar with the shared primary shortcut", () => {
    const toggleSidebar = vi.fn();
    const event = createKeyboardEvent({
      code: "KeyB",
      key: "b",
      metaKey: true,
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar,
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

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

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar,
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(false);
    expect(toggleSidebar).not.toHaveBeenCalled();
  });

  test("increases lyric font size with the shared primary shortcut", () => {
    const adjustLyricsFont = vi.fn();
    const event = createKeyboardEvent({
      code: "Equal",
      key: "+",
      metaKey: true,
      shiftKey: true,
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont,
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(adjustLyricsFont).toHaveBeenCalledWith(1);
  });

  test("decreases lyric font size with the shared primary shortcut", () => {
    const adjustLyricsFont = vi.fn();
    const event = createKeyboardEvent({
      code: "Minus",
      key: "-",
      ctrlKey: true,
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont,
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(adjustLyricsFont).toHaveBeenCalledWith(-1);
  });

  test("resets lyric font size with the shared primary shortcut", () => {
    const resetLyricsFont = vi.fn();
    const event = createKeyboardEvent({
      code: "Digit0",
      key: "0",
      metaKey: true,
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont,
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(resetLyricsFont).toHaveBeenCalledOnce();
  });

  test("ignores lyric font shortcuts while typing in an input", () => {
    const adjustLyricsFont = vi.fn();
    const event = createKeyboardEvent({
      code: "Equal",
      key: "+",
      ctrlKey: true,
      shiftKey: true,
      target: createKeyboardTarget("TEXTAREA"),
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont,
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(false);
    expect(adjustLyricsFont).not.toHaveBeenCalled();
  });

  test("opens the import dialog with the shared primary shortcut", () => {
    const openImportDialog = vi.fn();
    const event = createKeyboardEvent({
      code: "KeyO",
      key: "o",
      metaKey: true,
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog,
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
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

    const handled = handleAppKeyDown(event, {
      openImportDialog,
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(false);
    expect(openImportDialog).not.toHaveBeenCalled();
  });

  test("steps the remote plain-text lyrics page backward with PageUp", () => {
    const stepPlainTextPage = vi.fn();
    const event = createKeyboardEvent({
      code: "PageUp",
      key: "PageUp",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: true,
      stepPlainTextPage,
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(stepPlainTextPage).toHaveBeenCalledWith("prev");
  });

  test("ignores plain-text remote paging shortcuts while AirPlay page feedback is pending", () => {
    const stepPlainTextPage = vi.fn();
    const event = createKeyboardEvent({
      code: "PageDown",
      key: "PageDown",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage,
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(false);
    expect(stepPlainTextPage).not.toHaveBeenCalled();
  });

  test("ignores PageDown when plain-text remote paging is unavailable", () => {
    const stepPlainTextPage = vi.fn();
    const event = createKeyboardEvent({
      code: "PageDown",
      key: "PageDown",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage,
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(false);
    expect(stepPlainTextPage).not.toHaveBeenCalled();
  });

  test("toggles settings with the toggleSettings shortcut (Cmd+,)", () => {
    const toggleSettings = vi.fn();
    const event = createKeyboardEvent({
      code: "Comma",
      key: ",",
      metaKey: true,
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings,
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(toggleSettings).toHaveBeenCalledOnce();
  });

  test("toggles settings even when focus is in an input", () => {
    const toggleSettings = vi.fn();
    const event = createKeyboardEvent({
      code: "Comma",
      key: ",",
      metaKey: true,
      target: createKeyboardTarget("INPUT"),
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings,
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(toggleSettings).toHaveBeenCalledOnce();
  });

  test("pauses playback when Space is pressed and currently playing", () => {
    const pause = vi.fn();
    const event = createKeyboardEvent({
      code: "Space",
      key: " ",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: { is_playing: true, song_id: "abc", volume: 0.8 } as never,
        positionMs: 10000,
        playingSinceMs: null,
        pause,
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(pause).toHaveBeenCalledOnce();
  });

  test("resumes playback when Space is pressed and paused with a song loaded", () => {
    const resume = vi.fn();
    const event = createKeyboardEvent({
      code: "Space",
      key: " ",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: { is_playing: false, song_id: "abc", volume: 0.8 } as never,
        positionMs: 10000,
        playingSinceMs: null,
        pause: vi.fn(),
        resume,
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(resume).toHaveBeenCalledOnce();
  });

  test("handles Space with no active snapshot without calling pause or resume", () => {
    const pause = vi.fn();
    const resume = vi.fn();
    const event = createKeyboardEvent({
      code: "Space",
      key: " ",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause,
        resume,
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(pause).not.toHaveBeenCalled();
    expect(resume).not.toHaveBeenCalled();
  });

  test("seeks backward by 5000ms with ArrowLeft", () => {
    const seek = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowLeft",
      key: "ArrowLeft",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 15000,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek,
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(seek).toHaveBeenCalledWith(10000);
  });

  test("seeks forward by 5000ms with ArrowRight", () => {
    const seek = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowRight",
      key: "ArrowRight",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 10000,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek,
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(seek).toHaveBeenCalledWith(15000);
  });

  test("F7: arrow key seek uses extrapolated position, not raw snapshot", () => {
    const seek = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowLeft",
      key: "ArrowLeft",
    });

    // Mock performance.now() to return a known value.
    const originalNow = performance.now.bind(performance);
    let mockNow = 5000;
    performance.now = () => mockNow;

    try {
      // positionMs=10000, playingSinceMs=1000, now=5000
      // extrapolated = 10000 + (5000 - 1000) = 14000
      // seek = 14000 - 5000 = 9000
      // If the code used raw positionMs instead: seek = 10000 - 5000 = 5000
      const handled = handleAppKeyDown(event, {
        openImportDialog: vi.fn(),
        toggleSettings: vi.fn(),
        toggleSidebar: vi.fn(),
        adjustLyricsFont: vi.fn(),
        resetLyricsFont: vi.fn(),
        canStepPlainTextPage: false,
        stepPlainTextPage: vi.fn(),
        player: {
          snapshot: { is_playing: true, song_id: "abc" } as never,
          positionMs: 10000,
          playingSinceMs: 1000,
          pause: vi.fn(),
          resume: vi.fn(),
          seek,
          setVolume: vi.fn(),
        },
      });

      expect(handled).toBe(true);
      expect(seek).toHaveBeenCalledWith(9000);
    } finally {
      performance.now = originalNow;
    }
  });

  test("increases volume by 0.05 with ArrowUp, capped at 1", () => {
    const setVolume = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowUp",
      key: "ArrowUp",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: { is_playing: true, song_id: "abc", volume: 0.97 } as never,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume,
      },
    });

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(setVolume).toHaveBeenCalledWith(1);
  });

  test("defaults volume to 1 when snapshot has no volume field for ArrowUp", () => {
    const setVolume = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowUp",
      key: "ArrowUp",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume,
      },
    });

    expect(handled).toBe(true);
    expect(setVolume).toHaveBeenCalledWith(1);
  });

  test("decreases volume by 0.05 with ArrowDown, floored at 0", () => {
    const setVolume = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowDown",
      key: "ArrowDown",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: { is_playing: true, song_id: "abc", volume: 0.02 } as never,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume,
      },
    });

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(setVolume).toHaveBeenCalledWith(0);
  });

  test("defaults volume to 1 when snapshot has no volume field for ArrowDown", () => {
    const setVolume = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowDown",
      key: "ArrowDown",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume,
      },
    });

    expect(handled).toBe(true);
    expect(setVolume).toHaveBeenCalledWith(0.95);
  });

  test("ignores arrow keys when focus is inside a role=dialog element", () => {
    const seek = vi.fn();
    const event = createKeyboardEvent({
      code: "ArrowLeft",
      key: "ArrowLeft",
      target: {
        tagName: "DIV",
        isContentEditable: false,
        closest: vi.fn(() => ({})),
      } as unknown as EventTarget,
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 10000,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek,
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(false);
    expect(seek).not.toHaveBeenCalled();
  });

  test("returns false for unhandled keys", () => {
    const event = createKeyboardEvent({
      code: "KeyZ",
      key: "z",
    });

    const handled = handleAppKeyDown(event, {
      openImportDialog: vi.fn(),
      toggleSettings: vi.fn(),
      toggleSidebar: vi.fn(),
      adjustLyricsFont: vi.fn(),
      resetLyricsFont: vi.fn(),
      canStepPlainTextPage: false,
      stepPlainTextPage: vi.fn(),
      player: {
        snapshot: null,
        positionMs: 0,
        playingSinceMs: null,
        pause: vi.fn(),
        resume: vi.fn(),
        seek: vi.fn(),
        setVolume: vi.fn(),
      },
    });

    expect(handled).toBe(false);
  });
});
