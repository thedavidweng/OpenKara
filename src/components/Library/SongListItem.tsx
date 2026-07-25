import { useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { CoverArtThumbnail } from "@/components/Shared/CoverArtThumbnail";
import { useBootstrapStore } from "@/stores/bootstrap-store";
import { useLibraryStore } from "@/stores/library-store";
import { usePlayerStore } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { useSettingsStore } from "@/stores/settings-store";
import { useQueueStore } from "@/stores/queue-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import { formatDuration } from "@/lib/format";
import { TaskProgressBar } from "@/components/Layout/GlobalProgressBar";
import { songCanBeSeparated } from "@/lib/song-media";
import * as api from "@/lib/tauri";
import { notifyError, notifySuccess } from "@/lib/errors";
import { showNativeContextMenu } from "@/lib/native-context-menu";
import { ConfirmationDialog } from "../Settings/ConfirmationDialog";
import { InputDialog } from "../Settings/InputDialog";
import { SongEditDialog } from "./SongEditDialog";
import { SongPropertiesDialog } from "./SongPropertiesDialog";
import {
  buildSongListContextMenuForSong,
  getSongListContextSongIds,
} from "./song-list-item-context-menu-build";
import type { Song } from "@/types/ipc";

function getSongDisplayName(song: Song): string {
  return song.title ?? song.file_path?.split("/").pop() ?? song.hash;
}

interface SongListItemProps {
  song: Song;
  orderedHashes: string[];
}

export function SongListItem({ song, orderedHashes }: SongListItemProps) {
  const { t } = useTranslation();
  const isSelected = useLibraryStore((s) => s.selectedSongIds.has(song.hash));
  const selectSong = useLibraryStore((s) => s.selectSong);
  const separationStatus = useLibraryStore(
    (s) => s.separationStatuses[song.hash],
  );
  const uploadStatus = useLibraryStore((s) => s.uploadStatuses[song.hash]);
  const createPlaylist = usePlaylistStore((s) => s.createPlaylist);
  const addSongsToPlaylist = usePlaylistStore((s) => s.addSongsToPlaylist);
  const playSong = usePlayerStore((s) => s.playSong);
  const closeSettings = useSettingsStore((s) => s.close);

  const [editDialogOpen, setEditDialogOpen] = useState(false);
  const [propertiesDialogOpen, setPropertiesDialogOpen] = useState(false);
  const [deleteSongIds, setDeleteSongIds] = useState<string[] | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);
  const [playlistDialogOpen, setPlaylistDialogOpen] = useState(false);

  const isCurrentPlaying = usePlayerStore(
    (s) => s.snapshot?.song_id === song.hash && !!s.snapshot?.is_playing,
  );
  const isCurrentLoading = usePlayerStore(
    (s) => s.snapshot?.song_id === song.hash && s.snapshot?.state === "loading",
  );
  const sepState = separationStatus?.state ?? "idle";
  const isMediaG = song.media_g_container != null;
  const canSeparateSong = songCanBeSeparated(song);
  // While the separation model is still being fetched (first run), the
  // Separate button waits instead of failing with a raw backend error.
  const modelPreparing = useBootstrapStore(
    (s) => s.status?.state === "pending" || s.status?.state === "downloading",
  );
  const mediaGBadgeLabel =
    song.media_g_container === "paired"
      ? "CDG"
      : song.media_g_container === "zip"
        ? "ZIP+G"
        : null;

  const handleDoubleClick = () => {
    const current = usePlayerStore.getState().snapshot;
    if (current?.song_id && current.song_id !== song.hash) {
      useQueueStore.getState().addToQueue(song.hash);
    } else {
      playSong(song.hash);
    }
    closeSettings();
  };

  const handleSeparate = (e: React.MouseEvent) => {
    e.stopPropagation();
    api.separate(song.hash).catch((err) => notifyError(err));
  };

  const handleDeleteSongs = async (songIds: string[]) => {
    setIsDeleting(true);
    try {
      const result = await api.deleteSongs(songIds);
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

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    if (!isSelected) {
      selectSong(
        song.hash,
        { shiftKey: false, metaKey: false, ctrlKey: false },
        orderedHashes,
      );
    }
    const items = buildSongListContextMenuForSong(
      song,
      (key, options) => String(t(key as never, options as never)),
      {
        setEditDialogOpen,
        setPropertiesDialogOpen,
        setDeleteSongIds,
        setPlaylistDialogOpen,
      },
    );
    void showNativeContextMenu(items, e.clientX, e.clientY);
  };

  return (
    <div
      onClick={(e) =>
        selectSong(
          song.hash,
          { shiftKey: e.shiftKey, metaKey: e.metaKey, ctrlKey: e.ctrlKey },
          orderedHashes,
        )
      }
      onDoubleClick={handleDoubleClick}
      onContextMenu={handleContextMenu}
      className={`group relative flex select-none items-center gap-2.5 rounded-[14px] border px-3 py-2.5 transition-colors duration-150 ${
        isSelected
          ? "border-[var(--sidebar-row-selected-border)] bg-[var(--sidebar-row-selected-bg)] text-[var(--color-text)]"
          : "border-transparent text-[var(--color-text)] hover:bg-[var(--sidebar-row-overlay-bg)]"
      }`}
      data-native-overlay-surface="song-row"
      data-song-list-item-variant="unified"
      data-song-hash={song.hash}
      style={
        {
          contentVisibility: "auto",
          containIntrinsicSize: "64px",
        } satisfies CSSProperties
      }
    >
      <CoverArtThumbnail
        songHash={song.hash}
        coverArt={song.cover_art}
        alt={`${getSongDisplayName(song)} cover art`}
        className="h-11 w-11 shrink-0"
      />

      <div className="flex min-w-0 flex-1 flex-col justify-center">
        <div className="flex items-center justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2 overflow-hidden">
            {isCurrentPlaying ? (
              <div className="flex w-3 shrink-0 justify-center">
                <span className="inline-flex h-2 w-2 rounded-full bg-[var(--color-accent)]" />
              </div>
            ) : isCurrentLoading ? (
              <div className="flex w-3 shrink-0 justify-center">
                <Loader2 size={10} className="animate-spin" />
              </div>
            ) : (
              <div className="w-3 shrink-0" />
            )}
            <span className="truncate text-[15px] font-semibold">
              {getSongDisplayName(song)}
            </span>
          </div>

          <div className="flex shrink-0 items-center gap-2">
            {mediaGBadgeLabel && (
              <span className="inline-flex h-[14px] items-center justify-center rounded bg-[var(--sidebar-row-overlay-bg)] px-1.5 text-[9px] font-semibold leading-none tracking-[0.08em] text-[var(--color-text-dim)]">
                {mediaGBadgeLabel}
              </span>
            )}
            {sepState === "idle" && canSeparateSong && (
              <button
                onClick={handleSeparate}
                disabled={modelPreparing}
                title={modelPreparing ? t("library.modelPreparing") : undefined}
                className={`rounded border px-1.5 py-0.5 text-[10px] disabled:cursor-default disabled:opacity-50 ${
                  isSelected
                    ? "border-[var(--sidebar-control-border)] bg-[var(--sidebar-control-bg)] text-[var(--color-text)] hover:bg-[var(--sidebar-row-overlay-bg)]"
                    : "border-[var(--sidebar-control-border)] bg-[var(--sidebar-control-bg)] text-[var(--color-text-dim)] hover:bg-[var(--sidebar-row-overlay-bg)]"
                }`}
                data-native-overlay-surface="song-action"
              >
                {t("library.separate")}
              </button>
            )}
            {sepState === "running" && (
              <div className="flex items-center gap-1 text-[11px] text-[var(--color-text-dim)]">
                <Loader2 size={10} className="animate-spin" />
                <span>{separationStatus?.percent ?? 0}%</span>
              </div>
            )}
            {sepState === "completed" && (
              <span className="flex items-center gap-1.5 text-[11px] text-[var(--color-text-dim)]">
                <span
                  className={`inline-flex h-[14px] min-w-[14px] items-center justify-center rounded text-[9px] font-semibold leading-none ${
                    separationStatus?.drums_path
                      ? "bg-[var(--sidebar-row-overlay-bg)] text-[var(--color-accent)]"
                      : "bg-[var(--sidebar-row-overlay-bg)] text-[var(--color-text-dim)]"
                  }`}
                >
                  {separationStatus?.drums_path ? "4" : "2"}
                </span>
                {formatDuration(song.duration_ms)}
              </span>
            )}
            {sepState === "failed" && canSeparateSong && (
              <button
                onClick={handleSeparate}
                className="text-[10px] text-[var(--color-destructive)]"
              >
                {t("common.retry")}
              </button>
            )}
            {(isMediaG ||
              !canSeparateSong ||
              (sepState !== "idle" &&
                sepState !== "running" &&
                sepState !== "completed" &&
                sepState !== "failed")) && (
              <span className="text-[11px] text-[var(--color-text-dim)]">
                {formatDuration(song.duration_ms)}
              </span>
            )}
          </div>
        </div>

        {(separationStatus?.state === "running" ||
          uploadStatus?.state === "running") && (
          <div className="mt-1 space-y-1">
            {separationStatus?.state === "running" && (
              <TaskProgressBar
                label={t("progress.separating", {
                  title: getSongDisplayName(song),
                  defaultValue: `Separating: ${getSongDisplayName(song)}`,
                })}
                percent={separationStatus.percent}
              />
            )}
            {uploadStatus?.state === "running" && (
              <TaskProgressBar
                label={t("progress.uploadingToRemote", {
                  title: getSongDisplayName(song),
                  defaultValue: `Publishing to remote repository: ${getSongDisplayName(
                    song,
                  )}`,
                })}
                percent={uploadStatus.percent}
              />
            )}
          </div>
        )}

        <div className="flex pl-5">
          <span className="truncate text-[12px] text-[var(--color-text-dim)]">
            {song.artist || t("common.unknownArtist")}
          </span>
        </div>
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
          onConfirm={async (name) => {
            setPlaylistDialogOpen(false);
            const contextSongIds = getSongListContextSongIds(song);
            try {
              const playlist = await createPlaylist(name.trim());
              await addSongsToPlaylist(playlist.id, contextSongIds);
              notifySuccess(
                t("playlist.createdAndAddedToast", {
                  count: contextSongIds.length,
                }),
              );
            } catch (error) {
              notifyError(error);
            }
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
