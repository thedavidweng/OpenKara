export type ShortcutPlatform = "mac" | "windows" | "linux";

export interface ShortcutEventLike {
  code: string;
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

export interface ShortcutDefinition {
  id: string;
  code?: string;
  key?: string;
  displayKey: string;
  acceptedCodes?: string[];
  acceptedKeys?: string[];
  allowShift?: boolean;
  requiresPrimaryModifier?: boolean;
}

export const APP_SHORTCUTS = {
  toggleSidebar: {
    id: "sidebar.toggle",
    code: "KeyB",
    key: "b",
    displayKey: "B",
  },
  importFiles: {
    id: "library.import",
    code: "KeyO",
    key: "o",
    displayKey: "O",
  },
  toggleSettings: {
    id: "settings.toggle",
    code: "Comma",
    key: ",",
    displayKey: ",",
  },
  increaseLyricsFont: {
    id: "lyrics.font.increase",
    displayKey: "+",
    acceptedCodes: ["Equal", "NumpadAdd"],
    acceptedKeys: ["+", "="],
    allowShift: true,
  },
  decreaseLyricsFont: {
    id: "lyrics.font.decrease",
    code: "Minus",
    key: "-",
    displayKey: "-",
  },
  resetLyricsFont: {
    id: "lyrics.font.reset",
    code: "Digit0",
    key: "0",
    displayKey: "0",
  },
  lyricsPagePrev: {
    id: "lyrics.page.prev",
    code: "PageUp",
    key: "PageUp",
    displayKey: "PageUp",
    requiresPrimaryModifier: false,
  },
  lyricsPageNext: {
    id: "lyrics.page.next",
    code: "PageDown",
    key: "PageDown",
    displayKey: "PageDown",
    requiresPrimaryModifier: false,
  },
  toggleQueue: {
    id: "queue.toggle",
    code: "KeyQ",
    key: "q",
    displayKey: "Q",
    requiresPrimaryModifier: false,
  },
  toggleMute: {
    id: "player.mute",
    code: "KeyM",
    key: "m",
    displayKey: "M",
    requiresPrimaryModifier: false,
  },
  toggleFullscreen: {
    id: "player.fullscreen",
    code: "KeyF",
    key: "f",
    displayKey: "F",
    requiresPrimaryModifier: false,
  },
  stopPlayback: {
    id: "player.stop",
    code: "Period",
    key: ".",
    displayKey: ".",
    requiresPrimaryModifier: true,
  },
  separateCurrent: {
    id: "player.separate",
    code: "KeyS",
    key: "s",
    displayKey: "S",
    requiresPrimaryModifier: true,
    allowShift: true,
  },
  seekBackward: {
    id: "player.seekBackward",
    displayKey: "Left",
    acceptedCodes: ["ArrowLeft"],
    acceptedKeys: ["ArrowLeft"],
    requiresPrimaryModifier: true,
  },
  seekForward: {
    id: "player.seekForward",
    displayKey: "Right",
    acceptedCodes: ["ArrowRight"],
    acceptedKeys: ["ArrowRight"],
    requiresPrimaryModifier: true,
  },
} satisfies Record<string, ShortcutDefinition>;

export function getShortcutPlatform(): ShortcutPlatform {
  const platform =
    typeof navigator !== "undefined"
      ? (navigator as Navigator & { userAgentData?: { platform?: string } })
          .userAgentData?.platform || navigator.platform
      : "";

  if (/mac|darwin/i.test(platform)) return "mac";
  if (/win/i.test(platform)) return "windows";
  return "linux";
}

export function getShortcutDisplay(
  shortcut: ShortcutDefinition,
  platform: ShortcutPlatform = getShortcutPlatform(),
): string {
  if (shortcut.requiresPrimaryModifier === false) {
    return shortcut.displayKey;
  }

  const modifier = platform === "mac" ? "⌘" : "Ctrl+";
  return `${modifier}${shortcut.displayKey}`;
}

export function matchesShortcut(
  shortcut: ShortcutDefinition,
  event: ShortcutEventLike,
): boolean {
  const acceptedCodes =
    shortcut.acceptedCodes ?? (shortcut.code ? [shortcut.code] : []);
  const acceptedKeys =
    shortcut.acceptedKeys ?? (shortcut.key ? [shortcut.key] : []);
  const hasPrimaryModifier = event.metaKey || event.ctrlKey;
  const requiresPrimaryModifier = shortcut.requiresPrimaryModifier !== false;

  return (
    (requiresPrimaryModifier ? hasPrimaryModifier : !hasPrimaryModifier) &&
    !event.altKey &&
    (shortcut.allowShift || !event.shiftKey) &&
    acceptedCodes.includes(event.code) &&
    acceptedKeys.some((key) => event.key.toLowerCase() === key.toLowerCase())
  );
}

export function isEditableShortcutTarget(target: EventTarget | null): boolean {
  const element = target as {
    tagName?: string;
    isContentEditable?: boolean;
  } | null;

  if (!element) {
    return false;
  }

  return (
    element.tagName === "INPUT" ||
    element.tagName === "TEXTAREA" ||
    element.tagName === "SELECT" ||
    element.isContentEditable === true
  );
}
