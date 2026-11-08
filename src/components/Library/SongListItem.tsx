import { useEffect, useId, useMemo, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { Layers, Loader2, RotateCw, X } from "lucide-react";
import { Tooltip } from "@/components/Overlay/Tooltip";
import { CoverArtThumbnail } from "@/components/Shared/CoverArtThumbnail";
import { useBootstrapStore } from "@/stores/bootstrap-store";
import { useLibraryStore } from "@/stores/library-store";
import { usePlayerStore } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { useSettingsStore } from "@/stores/settings-store";
import { useQueueStore } from "@/stores/queue-store";
import { formatDuration } from "@/lib/format";
import { TaskProgressBar } from "@/components/Layout/GlobalProgressBar";
import { songCanBeSeparated } from "@/lib/song-media";
import { batchSeparationInProgress } from "@/lib/task-progress";
import { useBackend } from "@/lib/backend";
import { notifyError } from "@/lib/errors";
import { showNativeContextMenu } from "@/lib/native-context-menu";
import {
  createSongCommands,
  type SongCommandContext,
} from "@/lib/song-commands";
import { ConfirmationDialog } from "../Settings/ConfirmationDialog";
import { InputDialog } from "../Settings/InputDialog";
import { SongEditDialog } from "./SongEditDialog";
import { SongPropertiesDialog } from "./SongPropertiesDialog";
import { songDisplayTitle } from "@/lib/song-display";
import type { RevealTargets, Song } from "@/types/ipc";

const songActionClassName =
  "inline-flex size-7 items-center justify-center rounded-md border border-[var(--sidebar-control-border)] bg-[var(--sidebar-control-bg)] text-[var(--color-text-dim)] hover:bg-[var(--sidebar-row-selected-bg)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--color-focus-ring)] disabled:cursor-default disabled:opacity-50";

interface SongListItemProps {
  song: Song;
  orderedHashes: string[];
  previewMode?: boolean;
}

export function SongListItem({
  song,
  orderedHashes,
  previewMode = false,
}: SongListItemProps) {
  const backend = useBackend();
  const { t } = useTranslation();
  const isSelected = useLibraryStore((s) => s.selectedSongIds.has(song.hash));
  const selectSong = useLibraryStore((s) => s.selectSong);
  const separationStatus = useLibraryStore(
    (s) => s.separationStatuses[song.hash],
  );
  const uploadStatus = useLibraryStore((s) => s.uploadStatuses[song.hash]);
  const batchActive = useLibraryStore((s) =>
    batchSeparationInProgress(s.batchSeparation),
  );
  const playSong = usePlayerStore((s) => s.playSong);
  const playNow = usePlayerStore((s) => s.playNow);
  const closeSettings = useSettingsStore((s) => s.close);

  const [editDialogOpen, setEditDialogOpen] = useState(false);
  const [propertiesDialogOpen, setPropertiesDialogOpen] = useState(false);
  const [deleteSongIds, setDeleteSongIds] = useState<string[] | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);
  const [playlistDialogOpen, setPlaylistDialogOpen] = useState(false);
  const [revealTargets, setRevealTargets] = useState<RevealTargets | undefined>(
    undefined,
  );

  const songCommands = useMemo(
    () =>
      createSongCommands({
        backend,
        dialogs: {
          editInfo: () => setEditDialogOpen(true),
          properties: () => setPropertiesDialogOpen(true),
          confirmDelete: setDeleteSongIds,
          createPlaylist: () => setPlaylistDialogOpen(true),
        },
      }),
    [backend],
  );
  useEffect(() => {
    let cancelled = false;
    void backend.catalog
      .getRevealTargets(song.hash)
      .then((targets) => {
        if (!cancelled) {
          setRevealTargets(targets);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setRevealTargets(undefined);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [backend.catalog, song.hash]);

  const commandContext: SongCommandContext = {
    song,
    t: (key, options) => String(t(key as never, options as never)),
    revealTargets,
  };

  const isCurrentPlaying = usePlayerStore(
    (s) => s.snapshot?.song_id === song.hash && !!s.snapshot?.is_playing,
  );
  const isCurrentLoading = usePlayerStore(
    (s) => s.snapshot?.song_id === song.hash && s.snapshot?.state === "loading",
  );
  const sepState = separationStatus?.state ?? "idle";
  const uploadRunning = uploadStatus?.state === "running";
  const showBatchSongProgress = batchActive && sepState === "running";
  const canSeparateSong = songCanBeSeparated(song);
  const modelPreparing = useBootstrapStore(
    (s) => s.status?.state === "pending" || s.status?.state === "downloading",
  );
  const preparingNoteId = useId();
  const progressPercent =
    sepState === "running"
      ? (separationStatus?.percent ?? 0)
      : uploadRunning
        ? (uploadStatus?.percent ?? 0)
        : null;
  const mediaGBadgeLabel =
    song.media_g_container === "paired"
      ? "CDG"
      : song.media_g_container === "zip"
        ? "ZIP+G"
        : null;

  const handlePlay = () => {
    if (previewMode) {
      void playNow(song.hash);
      closeSettings();
      return;
    }
    const current = usePlayerStore.getState().snapshot;
    if (current?.song_id && current.song_id !== song.hash) {
      useQueueStore.getState().addToQueue(song.hash);
    } else {
      playSong(song.hash);
    }
    closeSettings();
  };

  const selectSongFromEvent = (event: React.MouseEvent<HTMLButtonElement>) => {
    selectSong(
      song.hash,
      {
        shiftKey: event.shiftKey,
        metaKey: event.metaKey,
        ctrlKey: event.ctrlKey,
      },
      orderedHashes,
    );
  };

  const handleSeparate = (e: React.MouseEvent) => {
    e.stopPropagation();
    backend.separation.separate(song.hash).catch((err) => notifyError(err));
  };

  const handleCancelSeparation = (e: React.MouseEvent) => {
    e.stopPropagation();
    backend.separation
      .cancelSeparation(song.hash)
      .catch((err) => notifyError(err));
  };

  const handleDeleteSongs = async (songIds: string[]) => {
    setIsDeleting(true);
    try {
      const result = await backend.library.deleteSongs(songIds);
      for (const failure of result.failed) {
        notifyError(failure.error);
      }

      if (result.deleted_song_ids.length > 0) {
        useQueueStore.getState().removeSongIds(result.deleted_song_ids);
        useLibraryStore.setState((state) => ({
          selectedSongIds: new Set(
            [...state.selectedSongIds].filter(
              (id) => !result.deleted_song_ids.includes(id),
            ),
          ),
          lastClickedSongId: result.deleted_song_ids.includes(
            state.lastClickedSongId ?? "",
          )
            ? null
            : state.lastClickedSongId,
          separationStatuses: Object.fromEntries(
            Object.entries(state.separationStatuses).filter(
              ([id]) => !result.deleted_song_ids.includes(id),
            ),
          ),
          uploadStatuses: Object.fromEntries(
            Object.entries(state.uploadStatuses).filter(
              ([id]) => !result.deleted_song_ids.includes(id),
            ),
          ),
        }));
        await useLibraryStore.getState().loadLibrary();
        await usePlayerStore.getState().loadState();
        const lyricsStore = useLyricsStore.getState();
        if (
          lyricsStore.songId &&
          result.deleted_song_ids.includes(lyricsStore.songId)
        ) {
          lyricsStore.clear();
        }
      }
    } catch (error) {
      notifyError(error);
    } finally {
      setIsDeleting(false);
      setDeleteSongIds(null);
    }
  };

  const openContextMenu = (clientX: number, clientY: number) => {
    if (!isSelected) {
      selectSong(
        song.hash,
        { shiftKey: false, metaKey: false, ctrlKey: false },
        orderedHashes,
      );
    }
    void showNativeContextMenu(
      songCommands.buildMenu(commandContext),
      clientX,
      clientY,
    );
  };

  const handleContextMenu = (event: React.MouseEvent<HTMLButtonElement>) => {
    event.preventDefault();
    openContextMenu(event.clientX, event.clientY);
  };

  const handleSelectionKeyDown = (
    event: React.KeyboardEvent<HTMLButtonElement>,
  ) => {
    if (event.key === "Enter") {
      event.preventDefault();
      handlePlay();
      return;
    }

    if (
      event.key === "ContextMenu" ||
      (event.shiftKey && event.key === "F10")
    ) {
      event.preventDefault();
      const rect = event.currentTarget.getBoundingClientRect();
      openContextMenu(rect.left + rect.width / 2, rect.top + rect.height / 2);
    }
  };

  return (
    <div
      className={`group relative flex select-none items-center gap-2.5 rounded-[14px] border px-2 py-2.5 transition-colors duration-150 ${
        isSelected
          ? "border-[var(--sidebar-row-selected-border)] bg-[var(--sidebar-row-selected-bg)] text-[var(--color-text)]"
          : "border-transparent text-[var(--color-text)] hover:bg-[var(--sidebar-row-overlay-bg)]"
      }`}
      data-native-overlay-surface="song-row"
      data-song-list-item-variant="unified"
      data-song-hash={song.hash}
      data-selected={isSelected ? "true" : "false"}
      style={
        showBatchSongProgress
          ? undefined
          : ({
              contentVisibility: "auto",
              containIntrinsicSize: "64px",
            } satisfies CSSProperties)
      }
    >
      <button
        type="button"
        onClick={(event) => {
          selectSongFromEvent(event);
          if (previewMode && event.detail < 2) {
            handlePlay();
          }
        }}
        onDoubleClick={previewMode ? undefined : handlePlay}
        onContextMenu={handleContextMenu}
        onKeyDown={handleSelectionKeyDown}
        aria-label={songDisplayTitle(song)}
        aria-pressed={isSelected}
        className="absolute inset-0 z-0 cursor-pointer rounded-[14px] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]"
        data-song-action="select"
        data-preview-song-switch={previewMode ? "true" : undefined}
      />

      <CoverArtThumbnail
        songHash={song.hash}
        coverArt={song.cover_art}
        thumbnailPath={song.artwork_thumb_path}
        alt={t("common.coverArtAlt", { title: songDisplayTitle(song) })}
        className="pointer-events-none h-11 w-11 shrink-0"
      />

      <div className="relative z-10 pointer-events-none flex min-w-0 flex-1 flex-col justify-center">
        <div className="flex min-w-0 items-center gap-1.5">
          {isCurrentPlaying ? (
            <span className="size-1.5 shrink-0 rounded-full bg-[var(--color-accent)]" />
          ) : isCurrentLoading ? (
            <Loader2 size={10} className="shrink-0 animate-spin" />
          ) : null}
          <span className="truncate text-[14px] font-semibold">
            {songDisplayTitle(song)}
          </span>
        </div>

        {showBatchSongProgress && (
          <div className="mt-1.5">
            <TaskProgressBar
              compact
              label=""
              percent={separationStatus?.percent ?? 0}
              ariaLabel={t("progress.separating", {
                title: songDisplayTitle(song),
              })}
            />
          </div>
        )}

        <div className="flex items-center gap-2 text-[12px] text-[var(--color-text-dim)]">
          <span className="min-w-0 flex-1 truncate">
            {song.artist || t("common.unknownArtist")}
          </span>
          {mediaGBadgeLabel && (
            <span className="inline-flex h-[14px] shrink-0 items-center justify-center rounded bg-[var(--sidebar-row-overlay-bg)] px-1.5 text-[9px] font-semibold leading-none tracking-[0.08em]">
              {mediaGBadgeLabel}
            </span>
          )}
          {progressPercent != null ? (
            <span className="flex shrink-0 items-center gap-1 text-[11px] tabular-nums">
              <Loader2 size={10} className="animate-spin" />
              {progressPercent}%
            </span>
          ) : (
            <span className="shrink-0 text-[11px] tabular-nums">
              {formatDuration(song.duration_ms)}
            </span>
          )}
        </div>
      </div>

      <div className="pointer-events-none relative z-10 flex size-7 shrink-0 items-center justify-center">
        {sepState === "idle" && canSeparateSong && (
          <div className="pointer-events-auto">
            <Tooltip
              label={
                modelPreparing
                  ? t("library.modelPreparing")
                  : t("library.separate")
              }
            >
              <button
                type="button"
                onClick={handleSeparate}
                disabled={modelPreparing}
                aria-label={t("library.separate")}
                aria-describedby={modelPreparing ? preparingNoteId : undefined}
                className={songActionClassName}
                data-native-overlay-surface="song-action"
              >
                <Layers size={13} aria-hidden="true" />
              </button>
            </Tooltip>
            {modelPreparing && (
              <span id={preparingNoteId} className="sr-only">
                {t("library.modelPreparing")}
              </span>
            )}
          </div>
        )}
        {sepState === "running" && !batchActive && (
          <div className="pointer-events-auto">
            <Tooltip label={t("library.cancelSeparation")}>
              <button
                type="button"
                onClick={handleCancelSeparation}
                aria-label={t("library.cancelSeparation")}
                className={songActionClassName}
                data-native-overlay-surface="song-action"
              >
                <X size={13} aria-hidden="true" />
              </button>
            </Tooltip>
          </div>
        )}
        {sepState === "completed" && (
          <span
            className={`inline-flex size-5 items-center justify-center rounded bg-[var(--sidebar-row-overlay-bg)] text-[10px] font-semibold leading-none ${
              separationStatus?.drums_path
                ? "text-[var(--color-accent)]"
                : "text-[var(--color-text-dim)]"
            }`}
          >
            {separationStatus?.drums_path ? "4" : "2"}
          </span>
        )}
        {sepState === "failed" && canSeparateSong && (
          <div className="pointer-events-auto">
            <Tooltip label={t("common.retry")}>
              <button
                type="button"
                onClick={handleSeparate}
                aria-label={t("common.retry")}
                className={`${songActionClassName} text-[var(--color-destructive)] hover:text-[var(--color-destructive)]`}
                data-native-overlay-surface="song-action"
              >
                <RotateCw size={13} aria-hidden="true" />
              </button>
            </Tooltip>
          </div>
        )}
      </div>
      {editDialogOpen && (
        <SongEditDialog song={song} onClose={() => setEditDialogOpen(false)} />
      )}

      {propertiesDialogOpen && (
        <SongPropertiesDialog
          song={song}
          onClose={() => setPropertiesDialogOpen(false)}
        />
      )}

      {playlistDialogOpen && (
        <InputDialog
          title={t("playlist.create")}
          placeholder={t("playlist.name")}
          confirmLabel={t("common.save")}
          onConfirm={(name) => {
            setPlaylistDialogOpen(false);
            void songCommands.execute(
              { id: "createPlaylistAndAdd", name },
              commandContext,
            );
          }}
          onCancel={() => setPlaylistDialogOpen(false)}
        />
      )}

      {deleteSongIds && (
        <ConfirmationDialog
          title={t("library.confirmDeleteTitle", {
            count: deleteSongIds.length,
          })}
          message={t("library.confirmDeleteMessage", {
            count: deleteSongIds.length,
          })}
          confirmLabel={
            isDeleting ? t("common.deleting") : t("library.deleteConfirm")
          }
          onCancel={() => {
            if (!isDeleting) {
              setDeleteSongIds(null);
            }
          }}
          onConfirm={() => {
            if (!isDeleting) {
              void handleDeleteSongs(deleteSongIds);
            }
          }}
        />
      )}
    </div>
  );
}
