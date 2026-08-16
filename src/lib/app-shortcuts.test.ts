import { describe, expect, test } from "vitest";
import {
  APP_SHORTCUTS,
  getShortcutDisplay,
  matchesShortcut,
  shouldYieldSpaceToTarget,
} from "./app-shortcuts";

describe("app-shortcuts", () => {
  test("formats primary shortcuts for macOS", () => {
    expect(getShortcutDisplay(APP_SHORTCUTS.toggleSidebar, "mac")).toBe("⌘B");
    expect(getShortcutDisplay(APP_SHORTCUTS.importFiles, "mac")).toBe("⌘O");
    expect(getShortcutDisplay(APP_SHORTCUTS.toggleSettings, "mac")).toBe("⌘,");
    expect(getShortcutDisplay(APP_SHORTCUTS.increaseLyricsFont, "mac")).toBe(
      "⌘+",
    );
    expect(getShortcutDisplay(APP_SHORTCUTS.decreaseLyricsFont, "mac")).toBe(
      "⌘-",
    );
    expect(getShortcutDisplay(APP_SHORTCUTS.resetLyricsFont, "mac")).toBe("⌘0");
    expect(getShortcutDisplay(APP_SHORTCUTS.lyricsPagePrev, "mac")).toBe(
      "PageUp",
    );
    expect(getShortcutDisplay(APP_SHORTCUTS.lyricsPageNext, "mac")).toBe(
      "PageDown",
    );
  });

  test("formats primary shortcuts for non-mac platforms", () => {
    expect(getShortcutDisplay(APP_SHORTCUTS.toggleSidebar, "windows")).toBe(
      "Ctrl+B",
    );
    expect(getShortcutDisplay(APP_SHORTCUTS.importFiles, "windows")).toBe(
      "Ctrl+O",
    );
    expect(getShortcutDisplay(APP_SHORTCUTS.toggleSettings, "linux")).toBe(
      "Ctrl+,",
    );
    expect(
      getShortcutDisplay(APP_SHORTCUTS.increaseLyricsFont, "windows"),
    ).toBe("Ctrl++");
    expect(getShortcutDisplay(APP_SHORTCUTS.lyricsPagePrev, "windows")).toBe(
      "PageUp",
    );
  });

  test("matches shortcuts through the primary modifier", () => {
    expect(
      matchesShortcut(APP_SHORTCUTS.toggleSidebar, {
        code: "KeyB",
        key: "b",
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe(true);

    expect(
      matchesShortcut(APP_SHORTCUTS.toggleSidebar, {
        code: "KeyB",
        key: "b",
        metaKey: false,
        ctrlKey: true,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe(true);

    expect(
      matchesShortcut(APP_SHORTCUTS.importFiles, {
        code: "KeyO",
        key: "o",
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe(true);

    expect(
      matchesShortcut(APP_SHORTCUTS.toggleSettings, {
        code: "Comma",
        key: ",",
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe(false);
  });

  test("matches lyric font shortcuts with their accepted key shapes", () => {
    expect(
      matchesShortcut(APP_SHORTCUTS.increaseLyricsFont, {
        code: "Equal",
        key: "+",
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: true,
      }),
    ).toBe(true);

    expect(
      matchesShortcut(APP_SHORTCUTS.increaseLyricsFont, {
        code: "NumpadAdd",
        key: "+",
        metaKey: false,
        ctrlKey: true,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe(true);

    expect(
      matchesShortcut(APP_SHORTCUTS.decreaseLyricsFont, {
        code: "Minus",
        key: "-",
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe(true);

    expect(
      matchesShortcut(APP_SHORTCUTS.resetLyricsFont, {
        code: "Digit0",
        key: "0",
        metaKey: false,
        ctrlKey: true,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe(true);
  });

  test("matches plain-text paging shortcuts without the primary modifier", () => {
    expect(
      matchesShortcut(APP_SHORTCUTS.lyricsPagePrev, {
        code: "PageUp",
        key: "PageUp",
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe(true);

    expect(
      matchesShortcut(APP_SHORTCUTS.lyricsPageNext, {
        code: "PageDown",
        key: "PageDown",
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe(true);

    expect(
      matchesShortcut(APP_SHORTCUTS.lyricsPagePrev, {
        code: "PageUp",
        key: "PageUp",
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe(false);
  });
});

describe("shouldYieldSpaceToTarget", () => {
  function mockControl(options: {
    selectorHint: string;
    focusVisible: boolean;
  }) {
    const element = {
      matches: (selector: string) =>
        selector === ":focus-visible" ? options.focusVisible : false,
      closest: (selector: string) =>
        selector.includes(options.selectorHint) ? element : null,
    };
    return element as unknown as Element;
  }

  test("yields Space to a keyboard-focused button", () => {
    expect(
      shouldYieldSpaceToTarget(
        mockControl({ selectorHint: "button", focusVisible: true }),
      ),
    ).toBe(true);
  });

  test("keeps Space for play/pause after a pointer-focused button", () => {
    expect(
      shouldYieldSpaceToTarget(
        mockControl({ selectorHint: "button", focusVisible: false }),
      ),
    ).toBe(false);
  });

  test("always yields Space inside a dialog", () => {
    expect(
      shouldYieldSpaceToTarget(
        mockControl({ selectorHint: "dialog", focusVisible: false }),
      ),
    ).toBe(true);
  });
});
