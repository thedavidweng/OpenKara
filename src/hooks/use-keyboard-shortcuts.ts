import { useEffect, useRef } from "react";
import { usePlayerStore } from "@/stores/player-store";
import { useLayoutStore } from "@/stores/layout-store";
import { useLibraryStore } from "@/stores/library-store";
import { useQueueStore } from "@/stores/queue-store";
import { useSettingsStore } from "@/stores/settings-store";
import { promptImportFiles } from "@/runtime/menu-runtime";
import { songCanBeSeparated } from "@/lib/song-media";
import { batchSeparate } from "@/lib/tauri/maintenance";
import {
  closeFullscreenPlayer,
  openFullscreenPlayer,
} from "@/lib/fullscreen-player";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  APP_SHORTCUTS,
  isEditableShortcutTarget,
  matchesShortcut,
} from "@/lib/app-shortcuts";

const SEEK_STEP_MS = 5_000;

interface KeyboardShortcutPlayerState {
  snapshot: ReturnType<typeof usePlayerStore.getState>["snapshot"];
  resume: () => Promise<void>;
  pause: () => Promise<void>;
  seek: (ms: number) => Promise<void>;
  setVolume: (level: number) => Promise<void>;
}

interface KeyboardShortcutDeps {
  openImportDialog: () => void;
  toggleSettings: () => void;
  toggleSidebar: () => void;
  toggleQueue: () => void;
  toggleMute: () => void;
  toggleFullscreen: () => void;
  stopPlayback: () => void;
  separateCurrent: () => void;
  player: KeyboardShortcutPlayerState;
}

export function handleAppKeyDown(
  e: KeyboardEvent,
  {
    openImportDialog,
    toggleSettings,
    toggleSidebar,
    toggleQueue,
    toggleMute,
    toggleFullscreen,
    stopPlayback,
    separateCurrent,
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

  if (matchesShortcut(APP_SHORTCUTS.stopPlayback, e)) {
    e.preventDefault();
    stopPlayback();
    return true;
  }

  if (matchesShortcut(APP_SHORTCUTS.separateCurrent, e)) {
    e.preventDefault();
    separateCurrent();
    return true;
  }

  if (matchesShortcut(APP_SHORTCUTS.seekBackward, e)) {
    e.preventDefault();
    const { snapshot, seek } = player;
    if (snapshot?.song_id) {
      void seek(Math.max(0, (snapshot.position_ms ?? 0) - SEEK_STEP_MS));
    }
    return true;
  }

  if (matchesShortcut(APP_SHORTCUTS.seekForward, e)) {
    e.preventDefault();
    const { snapshot, seek } = player;
    if (snapshot?.song_id) {
      const duration_ms = snapshot.duration_ms ?? 0;
      void seek(
        Math.min(duration_ms, (snapshot.position_ms ?? 0) + SEEK_STEP_MS),
      );
    }
    return true;
  }

  if (matchesShortcut(APP_SHORTCUTS.toggleQueue, e)) {
    e.preventDefault();
    toggleQueue();
    return true;
  }

  if (matchesShortcut(APP_SHORTCUTS.toggleMute, e)) {
    e.preventDefault();
    toggleMute();
    return true;
  }

  if (matchesShortcut(APP_SHORTCUTS.toggleFullscreen, e)) {
    e.preventDefault();
    toggleFullscreen();
    return true;
  }

  const { snapshot, resume, pause, setVolume } = player;

  const target = e.target as HTMLElement | null;
  if (
    typeof target?.closest === "function" &&
    target.closest(
      '[role="dialog"], [data-dialog], [role="listbox"], [role="menu"], [role="slider"], button, a[href], summary, [role="button"], [role="switch"], [role="checkbox"], [role="radio"], [role="tab"]',
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
  const lastVolumeRef = useRef(1);

  useEffect(() => {
    if (!enabled) {
      return;
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      const playerState = usePlayerStore.getState();
      const library = useLibraryStore.getState();

      const separateCurrent = () => {
        const songId = playerState.snapshot?.song_id;
        if (!songId) return;
        const song = library.songs.find((s) => s.hash === songId);
        if (!songCanBeSeparated(song)) return;
        void batchSeparate([songId]).catch(() => {});
      };

      const toggleMute = () => {
        const volume = playerState.snapshot?.volume ?? 1;
        if (volume > 0) {
          lastVolumeRef.current = volume;
          void playerState.setVolume(0);
        } else {
          void playerState.setVolume(lastVolumeRef.current || 1);
        }
      };

      const toggleFullscreen = () => {
        void (async () => {
          try {
            const existing =
              await WebviewWindow.getByLabel("fullscreen-player");
            if (existing) {
              await closeFullscreenPlayer();
            } else {
              await openFullscreenPlayer();
            }
          } catch (err) {
            console.error("Failed to toggle fullscreen:", err);
          }
        })();
      };

      const stopPlayback = () => {
        void (async () => {
          await playerState.pause();
          await playerState.seek(0);
        })().catch(() => {});
      };

      handleAppKeyDown(e, {
        openImportDialog: () =>
          void promptImportFiles({
            importFiles: library.importFiles,
          }),
        toggleSettings: () => useSettingsStore.getState().toggle(),
        toggleSidebar: () => useLayoutStore.getState().toggleSidebar(),
        toggleQueue: () => useQueueStore.getState().togglePanel(),
        toggleMute,
        toggleFullscreen,
        stopPlayback,
        separateCurrent,
        player: {
          snapshot: playerState.snapshot,
          resume: playerState.resume,
          pause: playerState.pause,
          seek: playerState.seek,
          setVolume: playerState.setVolume,
        },
      });
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [enabled]);
}
