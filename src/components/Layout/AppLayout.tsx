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

const PREVIEW_WINDOW_SHELL_STATE: WindowShellState = {
  chromeVariant: "mac",
  tier: "mac",
  toolbarHeight: 48,
  trafficLightInsetLeading: 24,
  sidebarHeaderHeight: 0,
  sidebarWidth: 260,
};

function isPreviewPlaylistTarget(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    target.closest("[data-preview-playlist-switch='true']") != null
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
    if (isPreviewPlaylistTarget(event.target)) {
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
