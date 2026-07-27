import { useEffect } from "react";
import { usePlayerStore } from "@/stores/player-store";
import { useLayoutStore } from "@/stores/layout-store";
import { useLibraryStore } from "@/stores/library-store";
import { useSettingsStore } from "@/stores/settings-store";
import { promptImportFiles } from "@/runtime/menu-runtime";
import {
  APP_SHORTCUTS,
  isEditableShortcutTarget,
  matchesShortcut,
} from "@/lib/app-shortcuts";

interface KeyboardShortcutPlayerState {
  snapshot: ReturnType<typeof usePlayerStore.getState>["snapshot"];
  resume: () => Promise<void>;
  pause: () => Promise<void>;
  setVolume: (level: number) => Promise<void>;
}

interface KeyboardShortcutDeps {
  openImportDialog: () => void;
  toggleSettings: () => void;
  toggleSidebar: () => void;
  player: KeyboardShortcutPlayerState;
}

export function handleAppKeyDown(
  e: KeyboardEvent,
  {
    openImportDialog,
    toggleSettings,
    toggleSidebar,
    player,
  }: KeyboardShortcutDeps,
): boolean {
  if (matchesShortcut(APP_SHORTCUTS.toggleSettings, e)) {
    e.preventDefault();
    toggleSettings();
    return true;
  }

  if (isEditableShortcutTarget(e.target)) {
    return false;
  }

  if (matchesShortcut(APP_SHORTCUTS.toggleSidebar, e)) {
    e.preventDefault();
    toggleSidebar();
    return true;
  }

  if (matchesShortcut(APP_SHORTCUTS.importFiles, e)) {
    e.preventDefault();
    openImportDialog();
    return true;
  }

  const { snapshot, resume, pause, setVolume } = player;

  const target = e.target as HTMLElement | null;
  if (
    typeof target?.closest === "function" &&
    target.closest(
      '[role="dialog"], [data-dialog], [role="listbox"], [role="menu"]',
    )
  ) {
    return false;
  }

  switch (e.code) {
    case "Space": {
      e.preventDefault();
      if (snapshot?.state === "loading") {
        return true;
      }
      if (snapshot?.is_playing) {
        pause();
      } else if (snapshot?.song_id) {
        resume();
      }
      return true;
    }
    case "ArrowUp": {
      e.preventDefault();
      const volume = snapshot?.volume ?? 1;
      setVolume(Math.min(1, volume + 0.05));
      return true;
    }
    case "ArrowDown": {
      e.preventDefault();
      const volume = snapshot?.volume ?? 1;
      setVolume(Math.max(0, volume - 0.05));
      return true;
    }
    default:
      return false;
  }
}

export function useKeyboardShortcuts(enabled = true): void {
  useEffect(() => {
    if (!enabled) {
      return;
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      handleAppKeyDown(e, {
        openImportDialog: () =>
          void promptImportFiles({
            importFiles: useLibraryStore.getState().importFiles,
          }),
        toggleSettings: () => useSettingsStore.getState().toggle(),
        toggleSidebar: () => useLayoutStore.getState().toggleSidebar(),
        player: {
          snapshot: usePlayerStore.getState().snapshot,
          resume: usePlayerStore.getState().resume,
          pause: usePlayerStore.getState().pause,
          setVolume: usePlayerStore.getState().setVolume,
        },
      });
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [enabled]);
}
