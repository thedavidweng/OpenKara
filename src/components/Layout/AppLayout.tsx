import { useCallback, useEffect, type SyntheticEvent } from "react";
import { Sidebar } from "./Sidebar";
import { SidebarRail } from "./SidebarRail";
import { WindowChrome } from "./WindowChrome";
import { ToastContainer } from "./ToastContainer";
import { MainContentView } from "./MainContentView";
import { ImportCdgChoiceDialog } from "@/components/Library/ImportCdgChoiceDialog";
import { getShortcutPlatform } from "@/lib/app-shortcuts";
import {
  createWindowShellStyle,
  getNativeWindowShellState,
  type WindowShellState,
  useWindowShellState,
} from "@/lib/window-shell";
import { promptImportFiles } from "@/runtime/menu-runtime";
import { useLayoutStore } from "@/stores/layout-store";
import { useLibraryStore } from "@/stores/library-store";
import { useSettingsStore } from "@/stores/settings-store";
import { useRotationStore } from "@/stores/rotation-store";

interface AppLayoutProps {
  initialWindowShellState?: WindowShellState;
  previewMode?: boolean;
}

// Keep the landing preview on the same macOS fallback profile as the packaged
// app. This lets the shared toolbar retain its native traffic-light spacing as
// the desktop shell evolves, while the browser preview supplies only a visual
// stand-in for the OS-owned controls.
const PREVIEW_WINDOW_SHELL_STATE: WindowShellState =
  getNativeWindowShellState();

// Preview-mode interaction whitelist. The landing-page mock blocks all
// interactions except: (1) playlist switches in the sidebar (including the
// back button that exits a playlist), (2) lyrics scrolling + the Follow
// button inside the lyrics panel, (3) the toolbar sidebar toggle (keyboard
// already works via window shortcuts), (4) the play/pause toggle so the
// mock stays consistent with the spacebar shortcut, and (5) song list
// scrolling so visitors can browse the mock library. Import and other
// mutating actions stay blocked.
function isPreviewAllowedTarget(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    (target.closest("[data-preview-playlist-switch='true']") != null ||
      target.closest("[data-preview-lyrics-interactive='true']") != null ||
      target.closest("[data-preview-sidebar-toggle='true']") != null ||
      target.closest("[data-preview-play-toggle='true']") != null ||
      target.closest("[data-preview-song-list='true']") != null)
  );
}

export function AppLayout({
  initialWindowShellState,
  previewMode = false,
}: AppLayoutProps) {
  const sidebarVisible = useLayoutStore((s) => s.sidebarVisible);
  const sidebarWidth = useLayoutStore((s) => s.sidebarWidth);
  const setSidebarWidth = useLayoutStore((s) => s.setSidebarWidth);
  const toggleSidebar = useLayoutStore((s) => s.toggleSidebar);
  const settingsOpen = useSettingsStore((s) => s.isOpen);
  const openSettings = useSettingsStore((s) => s.open);
  const toggleSettings = useSettingsStore((s) => s.toggle);
  const importFiles = useLibraryStore((s) => s.importFiles);
  const platform = getShortcutPlatform();
  const hydratedWindowShellState = useWindowShellState(
    initialWindowShellState,
    previewMode ? "windows" : platform,
  );
  const windowShellState = previewMode
    ? PREVIEW_WINDOW_SHELL_STATE
    : hydratedWindowShellState;

  const handleImportMenuAction = useCallback(() => {
    if (previewMode) {
      return;
    }
    return promptImportFiles({ importFiles });
  }, [importFiles, previewMode]);

  const loadRotation = useRotationStore((s) => s.loadRotation);

  // The landing page borrows this shell as a composed product scene, not a
  // second app. Keep its state deterministic while preserving playlist changes
  // as the one meaningful way to inspect the mock library.
  const blockPreviewInteraction = useCallback((event: SyntheticEvent) => {
    if (isPreviewAllowedTarget(event.target)) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
  }, []);

  useEffect(() => {
    if (previewMode) {
      return;
    }
    void loadRotation();
  }, [loadRotation, previewMode]);

  return (
    <div
      className={`flex w-full flex-col overflow-hidden bg-[var(--color-sidebar)] font-sans ${previewMode ? "h-full" : "h-screen"}`}
      onContextMenu={(e) => e.preventDefault()}
      onClickCapture={previewMode ? blockPreviewInteraction : undefined}
      onDoubleClickCapture={previewMode ? blockPreviewInteraction : undefined}
      onKeyDownCapture={previewMode ? blockPreviewInteraction : undefined}
      onPointerDownCapture={previewMode ? blockPreviewInteraction : undefined}
      onContextMenuCapture={
        previewMode
          ? (event) => {
              event.preventDefault();
              event.stopPropagation();
            }
          : undefined
      }
      data-window-chrome-platform={windowShellState.chromeVariant}
      data-window-shell-tier={windowShellState.tier}
      data-preview-interaction-mode={previewMode ? "playlist-only" : undefined}
      style={createWindowShellStyle({
        ...windowShellState,
        sidebarWidth,
      })}
    >
      <WindowChrome
        onImportMenuAction={handleImportMenuAction}
        onOpenSettingsMenuAction={openSettings}
        onToggleSidebar={toggleSidebar}
        onToggleSettings={previewMode ? () => {} : toggleSettings}
        previewMode={previewMode}
        shellState={windowShellState}
        settingsOpen={settingsOpen}
        sidebarVisible={sidebarVisible}
      />

      <div className="flex min-h-0 flex-1 overflow-hidden">
        <SidebarRail
          visible={sidebarVisible}
          width={sidebarWidth}
          onResize={setSidebarWidth}
          resizable={!previewMode}
        >
          <Sidebar previewMode={previewMode} />
        </SidebarRail>

        <MainContentView previewMode={previewMode} />
      </div>

      {!previewMode && <ToastContainer />}
      {!previewMode && <ImportCdgChoiceDialog />}
    </div>
  );
}
