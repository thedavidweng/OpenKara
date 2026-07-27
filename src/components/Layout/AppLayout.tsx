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
  MAC_WINDOW_SHELL_STATE,
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

const PREVIEW_WINDOW_SHELL_STATE: WindowShellState = MAC_WINDOW_SHELL_STATE;

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

  const blockPreviewInteraction = useCallback((event: SyntheticEvent) => {
    const nativeEvent = event.nativeEvent;
    if (nativeEvent instanceof KeyboardEvent) {
      if (
        (nativeEvent.metaKey || nativeEvent.ctrlKey) &&
        !nativeEvent.altKey &&
        (nativeEvent.code === "Equal" ||
          nativeEvent.code === "NumpadAdd" ||
          nativeEvent.code === "Minus" ||
          nativeEvent.code === "NumpadSubtract" ||
          nativeEvent.code === "Digit0" ||
          nativeEvent.code === "Numpad0")
      ) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
    }
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
