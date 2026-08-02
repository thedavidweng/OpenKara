import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { audioDir } from "@tauri-apps/api/path";
import { confirm, open } from "@tauri-apps/plugin-dialog";
import i18next from "@/lib/i18n";
import { copyDebugInfo } from "@/lib/debug-info";
import { notifyError } from "@/lib/errors";
import * as api from "@/lib/tauri";
import { getShortcutPlatform } from "@/lib/app-shortcuts";
import { useLibraryStore } from "@/stores/library-store";
import { useLayoutStore } from "@/stores/layout-store";
import { useSettingsStore } from "@/stores/settings-store";

const APP_MENU_ACTION_EVENT = "openkara://menu-action";

export type AppMenuAction =
  | "import-files"
  | "open-settings"
  | "switch-library"
  | "toggle-sidebar"
  | "copy-debug-info";

export interface ExpandedImportPaths {
  paths: string[];
  song_count: number;
}

interface PromptImportFilesDependencies {
  importFiles: (paths: string[]) => Promise<void>;
  openDialog?: typeof open;
  getDefaultAudioDir?: typeof audioDir;
  confirmImport?: typeof confirm;
  expandImportPaths?: (paths: string[]) => Promise<ExpandedImportPaths>;
  pickImportPaths?: (defaultPath?: string) => Promise<string[]>;
}

interface MenuActionDependencies {
  toggleSettings: () => void;
  importFromDialog: () => Promise<void>;
  toggleSidebar: () => void;
}

const IMPORT_FILE_EXTENSIONS = [
  "mp3",
  "flac",
  "wav",
  "ogg",
  "m4a",
  "aac",
  "wma",
  "opus",
  "aiff",
  "aif",
  "cdg",
  "zip",
  "lrc",
];

function formatImportConfirmMessage(songCount: number): string {
  return i18next
    .t("library.importPrompt.confirmMessage")
    .replace("{{songCount}}", String(songCount));
}

function countImportableSongs(paths: string[]): number {
  let songCount = 0;

  for (const path of paths) {
    const normalized = path.replace(/\\/g, "/");
    const extension = normalized
      .split("/")
      .pop()
      ?.split(".")
      .pop()
      ?.toLowerCase();
    if (extension && extension !== "cdg" && extension !== "lrc") {
      songCount += 1;
    }
  }

  return songCount;
}

export async function promptImportFiles({
  importFiles,
  openDialog = open,
  getDefaultAudioDir = audioDir,
  confirmImport = confirm,
  expandImportPaths = api.expandImportPaths,
  pickImportPaths = api.pickImportPaths,
}: PromptImportFilesDependencies): Promise<void> {
  try {
    let defaultPath: string | undefined;

    try {
      defaultPath = await getDefaultAudioDir();
    } catch {
      // audioDir may not be available on all platforms; fall through
    }
    const selectedPaths =
      getShortcutPlatform() === "mac"
        ? await pickImportPaths(defaultPath)
        : await (async () => {
            const selected = await openDialog({
              multiple: true,
              defaultPath,
              filters: [
                {
                  name: i18next.t("library.importPrompt.audioLyricsFilter"),
                  extensions: IMPORT_FILE_EXTENSIONS,
                },
              ],
            });

            return Array.isArray(selected)
              ? selected
              : selected
                ? [selected]
                : [];
          })();

    if (selectedPaths.length === 0) {
      return;
    }

    const expandedSelection = await expandImportPaths(selectedPaths);
    const songCount = countImportableSongs(expandedSelection.paths);
    if (songCount <= 0) {
      return;
    }

    const confirmed = await confirmImport(
      formatImportConfirmMessage(songCount),
      {
        title: i18next.t("library.importPrompt.confirmTitle"),
        kind: "warning",
        okLabel: i18next.t("library.importPrompt.confirmOk"),
        cancelLabel: i18next.t("common.cancel"),
      },
    );

    if (confirmed) {
      await importFiles(expandedSelection.paths);
    }
  } catch (error) {
    notifyError(error);
  }
}

export async function handleAppMenuAction(
  action: AppMenuAction,
  { toggleSettings, importFromDialog, toggleSidebar }: MenuActionDependencies,
): Promise<void> {
  switch (action) {
    case "open-settings":
      toggleSettings();
      return;
    case "switch-library":
      toggleSettings();
      return;
    case "toggle-sidebar":
      toggleSidebar();
      return;
    case "import-files":
      await importFromDialog();
      return;
    case "copy-debug-info": {
      try {
        await copyDebugInfo();
      } catch (error) {
        notifyError(error);
      }
      return;
    }
    default:
      return;
  }
}

export function useAppMenuRuntime(enabled: boolean): void {
  const { t } = useTranslation();
  const importFiles = useLibraryStore((s) => s.importFiles);
  const toggleSidebar = useLayoutStore((s) => s.toggleSidebar);
  const toggleSettings = useSettingsStore((s) => s.toggle);

  useEffect(() => {
    void api
      .setNativeAppMenuLabels({
        file: t("windowChrome.file"),
        edit: t("windowChrome.edit"),
        view: t("windowChrome.view"),
        window: t("windowChrome.window"),
        help: t("windowChrome.help"),
        import: t("windowChrome.import"),
        settings: t("windowChrome.settings"),
        switchLibrary: t("windowChrome.switchLibrary"),
        toggleSidebar: t("toolbar.toggleSidebar"),
        copyDebugInfo: t("windowChrome.copyDebugInfo"),
      })
      .catch(() => {});
  }, [t]);

  useEffect(() => {
    if (!enabled) {
      return;
    }

    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    listen<AppMenuAction>(APP_MENU_ACTION_EVENT, (event) => {
      void handleAppMenuAction(event.payload, {
        toggleSettings,
        importFromDialog: () => promptImportFiles({ importFiles }),
        toggleSidebar,
      });
    }).then((dispose) => {
      if (cancelled) {
        dispose();
      } else {
        unlisten = dispose;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [enabled, importFiles, toggleSettings, toggleSidebar]);
}
