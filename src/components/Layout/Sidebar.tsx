import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowLeft,
  Folder,
  CheckCircle2,
  Layers,
  ListMusic,
} from "lucide-react";
import { ConfirmationDialog } from "@/components/Settings/ConfirmationDialog";
import { InputDialog } from "@/components/Settings/InputDialog";
import { SearchBox } from "@/components/Library/SearchBox";
import { SongList } from "@/components/Library/SongList";
import { SortModeSelector } from "@/components/Library/SortModeSelector";
import { songCanBeSeparated } from "@/lib/song-media";
import {
  batchSeparationInProgress,
  batchSeparationLabelArgs,
} from "@/lib/task-progress";
import { useLibraryStore } from "@/stores/library-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import { useSettingsStore } from "@/stores/settings-store";
import { useBackend } from "@/lib/backend";
import { notifyError } from "@/lib/errors";

interface SidebarProps {
  header?: ReactNode;
  previewMode?: boolean;
}

const batchActionClassName =
  "rounded-[12px] border border-[var(--sidebar-control-border)] bg-[var(--sidebar-control-bg)] px-3 py-2 text-[13px] hover:border-[var(--sidebar-control-border)] hover:bg-[var(--sidebar-row-overlay-bg)]";

export function Sidebar({ header, previewMode = false }: SidebarProps = {}) {
  const { maintenance } = useBackend();
  const { t } = useTranslation();
  const songs = useLibraryStore((s) => s.songs);
  const filter = useLibraryStore((s) => s.filter);
  const setFilter = useLibraryStore((s) => s.setFilter);
  const separationStatuses = useLibraryStore((s) => s.separationStatuses);
  const batchSeparation = useLibraryStore((s) => s.batchSeparation);

  const hideBatchSeparate = useSettingsStore((s) => s.hideBatchSeparate);
  const hideUpgradeAll = useSettingsStore((s) => s.hideUpgradeAll);
  const stemMode = useSettingsStore((s) => s.stemMode);
  const [showUpgradeConfirm, setShowUpgradeConfirm] = useState(false);
  const [showCreatePlaylist, setShowCreatePlaylist] = useState(false);

  const playlists = usePlaylistStore((s) => s.playlists);
  const activePlaylistId = usePlaylistStore((s) => s.activePlaylistId);
  const activePlaylist = playlists.find((p) => p.id === activePlaylistId);
  const loadPlaylists = usePlaylistStore((s) => s.loadPlaylists);
  const createPlaylist = usePlaylistStore((s) => s.createPlaylist);
  const setActivePlaylist = usePlaylistStore((s) => s.setActivePlaylist);

  useEffect(() => {
    if (previewMode) {
      return;
    }
    loadPlaylists();
  }, [loadPlaylists, previewMode]);

  const handleCreatePlaylist = async (name: string) => {
    try {
      await createPlaylist(name.trim());
    } catch {
      notifyError(new Error(t("errors.somethingWentWrong")));
    }
  };
  const separableSongs = songs.filter(songCanBeSeparated);

  const separatedCount = songs.filter(
    (s) => separationStatuses[s.hash]?.state === "completed",
  ).length;

  const allSeparated =
    separableSongs.length > 0 &&
    separableSongs.every((s) => {
      const status = separationStatuses[s.hash];
      return status?.state === "completed" || status?.state === "running";
    });

  const allMatchCurrentMode =
    allSeparated &&
    separableSongs.every((s) => {
      const status = separationStatuses[s.hash];
      if (!status) return false;
      if (status.state === "running") return true;
      if (status.state !== "completed") return false;
      if (stemMode === "four_stem") return !!status.drums_path;
      return true;
    });

  const needsUpgrade =
    allSeparated && !allMatchCurrentMode && stemMode === "four_stem";

  const showUpgradeButton = needsUpgrade && !hideUpgradeAll;

  const shouldHideButton =
    hideBatchSeparate ||
    separableSongs.length === 0 ||
    (allSeparated && allMatchCurrentMode) ||
    (needsUpgrade && hideUpgradeAll);

  const handleSeparateAll = () => {
    if (previewMode) {
      return;
    }
    maintenance.batchSeparate([]).catch(notifyError);
  };

  const isBatchRunning = batchSeparationInProgress(batchSeparation);

  return (
    <div
      className="flex h-full w-[var(--window-shell-sidebar-width)] shrink-0 flex-col bg-[var(--color-sidebar)]"
      data-window-shell-section="sidebar"
      data-sidebar-visual-variant="unified"
    >
      {header ? <div className="shrink-0 px-3 pb-2 pt-3">{header}</div> : null}

      <div className={`shrink-0 px-3 pb-3 ${header ? "pt-1" : "pt-3"}`}>
        <SearchBox />
      </div>

      {activePlaylistId && (
        <div className="shrink-0 flex items-center gap-2 border-b border-[color-mix(in_srgb,var(--color-border)_86%,transparent)] px-4 py-2">
          <button
            onClick={() => setActivePlaylist(null)}
            className="shrink-0 text-[var(--color-text-dim)] hover:text-[var(--color-control-primary)] transition-colors"
            aria-label={t("setup.back")}
            data-preview-playlist-switch={previewMode ? "true" : undefined}
          >
            <ArrowLeft size={14} />
          </button>
          <span className="text-[13px] font-medium truncate text-[var(--color-control-primary)]">
            {activePlaylist?.name}
          </span>
        </div>
      )}
      {!activePlaylistId && (
        <div className="shrink-0 space-y-0.5 px-2">
          <div className="px-2 pb-1 text-[11px] font-semibold tracking-wide text-[var(--color-text-dim)]">
            {t("sidebar.library")}
          </div>
          <button
            type="button"
            aria-label={t("sidebar.allTracks")}
            onClick={() => {
              setFilter("all");
              setActivePlaylist(null);
            }}
            className={`sidebar-source-list-row motion-surface flex w-full items-center justify-between px-2 py-1.5 text-[13px] ${
              filter === "all" && !activePlaylistId
                ? "border border-[var(--sidebar-row-selected-border)] bg-[var(--sidebar-row-selected-bg)] text-[var(--color-text)] shadow-[var(--shadow-control-selected)]"
                : "border border-transparent text-[var(--color-text)] hover:bg-[var(--sidebar-row-overlay-bg)]"
            }`}
          >
            <span className="flex items-center gap-2">
              <Folder
                size={14}
                className="text-[var(--color-accent)]"
                fill="currentColor"
                fillOpacity={0.2}
              />
              <span>{t("sidebar.allTracks")}</span>
            </span>
            <span className="text-[11px] text-[var(--color-text-dim)]">
              {songs.length}
            </span>
          </button>
          <button
            type="button"
            aria-label={t("sidebar.separated")}
            onClick={() => {
              setFilter("separated");
              setActivePlaylist(null);
            }}
            className={`sidebar-source-list-row motion-surface flex w-full items-center justify-between px-2 py-1.5 text-[13px] ${
              filter === "separated" && !activePlaylistId
                ? "border border-[var(--sidebar-row-selected-border)] bg-[var(--sidebar-row-selected-bg)] text-[var(--color-text)] shadow-[var(--shadow-control-selected)]"
                : "border border-transparent text-[var(--color-text)] hover:bg-[var(--sidebar-row-overlay-bg)]"
            }`}
          >
            <span className="flex items-center gap-2">
              <CheckCircle2 size={14} className="text-[var(--color-accent)]" />
              <span>{t("sidebar.separated")}</span>
            </span>
            <span className="text-[11px] text-[var(--color-text-dim)]">
              {separatedCount}
            </span>
          </button>
        </div>
      )}

      <div
        className={`shrink-0 space-y-0.5 px-2 ${previewMode ? "mt-3" : "mt-4"}`}
      >
        <div className="flex items-center justify-between px-2 pb-1">
          <span className="text-[11px] font-semibold tracking-wide text-[var(--color-text-dim)]">
            {t("playlist.section")}
          </span>
          <button
            onClick={() => setShowCreatePlaylist(true)}
            className="text-[11px] text-[var(--color-accent)] hover:text-[var(--color-accent-hover)] transition-colors"
            title={t("playlist.create")}
          >
            + {t("playlist.create")}
          </button>
        </div>
        {playlists.length === 0 ? (
          <div className="px-2 py-1 text-[11px] text-[var(--color-text-dim)]">
            {t("playlist.empty")}
          </div>
        ) : (
          <div className="space-y-0.5">
            {playlists.map((playlist) => (
              <button
                key={playlist.id}
                onClick={() => setActivePlaylist(playlist.id)}
                data-preview-playlist-switch={previewMode ? "true" : undefined}
                aria-current={
                  activePlaylistId === playlist.id ? "page" : undefined
                }
                className={`sidebar-source-list-row motion-surface flex w-full items-center justify-between rounded-lg px-2 py-1.5 text-[13px] ${
                  activePlaylistId === playlist.id
                    ? "border border-[var(--sidebar-row-selected-border)] bg-[var(--sidebar-row-selected-bg)] text-[var(--color-text)] shadow-[var(--shadow-control-selected)]"
                    : "border border-transparent text-[var(--color-text)] hover:bg-[var(--sidebar-row-overlay-bg)]"
                }`}
              >
                <span className="flex items-center gap-2 truncate">
                  <ListMusic
                    size={14}
                    className="text-[var(--color-text-dim)] shrink-0"
                  />
                  <span className="truncate">{playlist.name}</span>
                </span>
                <span className="text-[11px] text-[var(--color-text-dim)] shrink-0 ml-2">
                  {playlist.song_count}
                </span>
              </button>
            ))}
          </div>
        )}
      </div>

      {!activePlaylistId && (
        <div className="mt-4 flex flex-1 flex-col overflow-hidden px-2">
          <div className="flex items-center justify-between px-2 pb-1">
            <span className="text-[11px] font-semibold tracking-wide text-[var(--color-text-dim)]">
              {t("sidebar.localMusic")}
            </span>
            <SortModeSelector />
          </div>
          <SongList previewMode={previewMode} />
        </div>
      )}
      {activePlaylistId && (
        <div className="mt-4 flex flex-1 flex-col overflow-hidden px-2">
          <SongList previewMode={previewMode} />
        </div>
      )}

      {!(shouldHideButton && !isBatchRunning && batchSeparation == null) && (
        <div className="shrink-0 border-t border-[var(--color-border)] px-3 py-3">
          {batchSeparation != null ? (
            <div className="text-center text-[11px] text-[var(--color-text-dim)]">
              {isBatchRunning ? (
                t(
                  "sidebar.separating",
                  batchSeparationLabelArgs(batchSeparation),
                )
              ) : (
                <>
                  {t("sidebar.separationComplete", {
                    done: batchSeparation.completed,
                  })}
                  {batchSeparation.skipped > 0 &&
                    `, ${t("sidebar.skipped", { count: batchSeparation.skipped })}`}
                  {batchSeparation.failed > 0 &&
                    `, ${t("sidebar.failed", { count: batchSeparation.failed })}`}
                </>
              )}
            </div>
          ) : showUpgradeButton ? (
            <button
              onClick={() => setShowUpgradeConfirm(true)}
              className={`motion-surface flex w-full items-center justify-center gap-2 ${batchActionClassName} text-[var(--color-text)] hover:text-[var(--color-text)]`}
            >
              <Layers size={12} />
              {t("sidebar.upgradeAll")}
            </button>
          ) : (
            <button
              onClick={handleSeparateAll}
              disabled={separableSongs.length === 0}
              className={`motion-surface flex w-full items-center justify-center gap-2 ${batchActionClassName} text-[var(--color-text)] hover:text-[var(--color-text)] disabled:opacity-40`}
            >
              <Layers size={12} />
              {t("sidebar.separateAll")}
              <span className="text-[10px] text-[var(--color-text-dimmer)]">
                (
                {stemMode === "four_stem"
                  ? t("sidebar.fourStem")
                  : t("sidebar.twoStem")}
                )
              </span>
            </button>
          )}
        </div>
      )}

      {!previewMode && showCreatePlaylist && (
        <InputDialog
          title={t("playlist.create")}
          placeholder={t("playlist.name")}
          confirmLabel={t("common.save")}
          onConfirm={(name) => {
            setShowCreatePlaylist(false);
            handleCreatePlaylist(name);
          }}
          onCancel={() => setShowCreatePlaylist(false)}
        />
      )}

      {!previewMode && showUpgradeConfirm && (
        <ConfirmationDialog
          id="upgrade-confirmation"
          title={t("sidebar.confirmUpgrade.title")}
          message={t("sidebar.confirmUpgrade.message")}
          confirmLabel={t("sidebar.confirmUpgrade.confirm")}
          confirmButtonId="upgrade-confirm"
          onConfirm={() => {
            setShowUpgradeConfirm(false);
            if (!previewMode) {
              maintenance.batchSeparate([]).catch(notifyError);
            }
          }}
          onCancel={() => setShowUpgradeConfirm(false)}
        />
      )}
    </div>
  );
}
