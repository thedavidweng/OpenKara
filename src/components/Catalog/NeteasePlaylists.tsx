import { useEffect, useState, type RefObject } from "react";
import { ChevronLeft } from "lucide-react";
import { useTranslation } from "react-i18next";
import { NeteaseCover } from "@/components/Catalog/NeteaseCover";
import { NeteaseTrackList } from "@/components/Catalog/NeteaseTrackList";
import { formatUpdated } from "@/components/Catalog/netease-utils";
import { useCatalogStore } from "@/stores/catalog-store";
import type { StreamingPlaylistSummary } from "@/types/ipc";

const focusRing =
  "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]";

export function NeteasePlaylistGrid({
  playlists,
}: {
  playlists: StreamingPlaylistSummary[];
}) {
  const { t } = useTranslation();
  const openPlaylist = useCatalogStore((s) => s.openPlaylist);
  return (
    <ul className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-6 px-6 pb-6 pt-2">
      {playlists.map((playlist) => (
        <li
          key={playlist.remote_id}
          className="min-w-0 [contain-intrinsic-size:auto_220px] [content-visibility:auto]"
        >
          <button
            type="button"
            onClick={() => void openPlaylist(playlist.remote_id)}
            className={`group flex w-full flex-col gap-2 rounded-xl text-left ${focusRing}`}
          >
            <NeteaseCover
              url={playlist.cover_url}
              size={200}
              className="w-full rounded-xl transition-transform duration-150 group-hover:-translate-y-0.5"
            />
            <span className="min-w-0">
              <span className="line-clamp-2 text-[14px] font-semibold leading-snug text-[var(--color-text)]">
                {playlist.name}
              </span>
              <span className="mt-0.5 block text-[12px] text-[var(--color-text-dim)]">
                {t("catalog.netease.songCount", {
                  count: playlist.track_count,
                })}
              </span>
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}

export function NeteasePlaylistSkeleton() {
  return (
    <div
      aria-hidden="true"
      data-testid="netease-skeleton"
      className="flex animate-pulse flex-col gap-6 px-6 pt-4"
    >
      <div className="flex gap-7">
        <div className="size-56 shrink-0 rounded-xl bg-[var(--color-surface-muted)]" />
        <div className="flex-1 space-y-3 pt-4">
          <div className="h-8 w-2/3 rounded bg-[var(--color-surface-muted)]" />
          <div className="h-4 w-1/3 rounded bg-[var(--color-surface-muted)]" />
        </div>
      </div>
      {Array.from({ length: 6 }, (_, index) => (
        <div
          key={index}
          className="h-10 rounded-md bg-[var(--color-surface-muted)]"
        />
      ))}
    </div>
  );
}

export function NeteasePlaylistDetail({
  fallbackName,
  scrollRef,
}: {
  fallbackName: string;
  scrollRef: RefObject<HTMLElement | null>;
}) {
  const { t, i18n } = useTranslation();
  const detail = useCatalogStore((s) => s.playlistDetail);
  const status = useCatalogStore((s) => s.playlistDetailStatus);
  const selectedId = useCatalogStore((s) => s.selectedPlaylistId);
  const busy = useCatalogStore((s) => s.importRun?.phase === "running");
  const closePlaylist = useCatalogStore((s) => s.closePlaylist);
  const importTracks = useCatalogStore((s) => s.importTracks);
  const openPlaylist = useCatalogStore((s) => s.openPlaylist);

  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const detailId = detail?.remote_id;
  useEffect(() => {
    setSelected(new Set());
    setSelecting(false);
  }, [detailId]);

  const importable = detail?.tracks.filter((track) => !track.refusal) ?? [];
  const metaParts: string[] = [];
  if (detail?.creator) {
    metaParts.push(t("catalog.netease.byCreator", { creator: detail.creator }));
  }
  if (detail) {
    metaParts.push(
      t("catalog.netease.songCount", { count: detail.track_count }),
    );
    if (detail.updated_at_ms) {
      metaParts.push(
        t("catalog.netease.updatedOn", {
          date: formatUpdated(detail.updated_at_ms, i18n.language),
        }),
      );
    }
  }

  const primaryLabel = selecting
    ? t("catalog.netease.importSelected", { selected: selected.size })
    : t("catalog.netease.importPlaylist");
  const primaryDisabled =
    busy || (selecting ? selected.size === 0 : importable.length === 0);

  return (
    <div>
      <div className="px-6 pt-3">
        <button
          type="button"
          aria-label={t("catalog.netease.back")}
          title={t("catalog.netease.back")}
          onClick={closePlaylist}
          className={`inline-flex size-8 items-center justify-center rounded-full text-[var(--color-text-dim)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)] ${focusRing}`}
        >
          <ChevronLeft size={20} aria-hidden="true" />
        </button>
      </div>

      {status === "loading" && !detail ? <NeteasePlaylistSkeleton /> : null}

      {status === "error" ? (
        <div role="alert" className="flex items-center gap-3 p-6 text-[13px]">
          <span className="text-[var(--color-text-dim)]">
            {t("catalog.netease.loadFailed")}
          </span>
          <button
            type="button"
            className={`rounded-md border border-[var(--color-border-light)] px-3 py-1 ${focusRing}`}
            onClick={() => selectedId && void openPlaylist(selectedId)}
          >
            {t("common.retry")}
          </button>
        </div>
      ) : null}

      {detail ? (
        <>
          <header className="flex gap-7 px-6 pb-6 pt-3">
            <NeteaseCover
              url={detail.cover_url}
              size={512}
              eager
              glow
              className="size-56 rounded-xl max-[700px]:size-36"
            />
            <div className="flex min-w-0 flex-1 flex-col">
              <h2 className="line-clamp-2 text-[32px] font-bold leading-tight text-[var(--color-text)]">
                {detail.name || fallbackName}
              </h2>
              <p className="mt-2 text-[14px] text-[var(--color-text-dim)]">
                {metaParts.join(" · ")}
              </p>
              {detail.description ? (
                <p className="mt-2 line-clamp-3 text-[13px] text-[var(--color-text-dim)]">
                  {detail.description}
                </p>
              ) : null}
              <div className="mt-auto flex flex-wrap items-center gap-2 pt-4">
                <button
                  type="button"
                  disabled={primaryDisabled}
                  onClick={() => {
                    if (selecting) {
                      const ids = detail.tracks
                        .map((track) => track.remote_track_id)
                        .filter((id) => selected.has(id));
                      setSelected(new Set());
                      setSelecting(false);
                      void importTracks(ids, detail.remote_id, detail.name);
                    } else {
                      void importTracks(
                        importable.map((track) => track.remote_track_id),
                        detail.remote_id,
                        detail.name,
                      );
                    }
                  }}
                  className={`min-h-9 rounded-full bg-[var(--color-control-primary)] px-5 text-[13px] font-semibold text-[var(--color-control-primary-foreground)] hover:opacity-90 disabled:opacity-50 ${focusRing}`}
                >
                  {primaryLabel}
                </button>
                <button
                  type="button"
                  aria-pressed={selecting}
                  onClick={() => {
                    setSelecting((value) => !value);
                    setSelected(new Set());
                  }}
                  className={`min-h-9 rounded-full px-4 text-[13px] font-semibold text-[var(--color-text)] hover:bg-[var(--color-ghost-hover)] ${focusRing}`}
                >
                  {selecting
                    ? t("catalog.netease.selectDone")
                    : t("catalog.netease.select")}
                </button>
                <span className="text-[12px] text-[var(--color-text-dim)]">
                  {t("catalog.netease.importableCount", {
                    importable: importable.length,
                  })}
                </span>
              </div>
            </div>
          </header>
          {detail.tracks.length === 0 ? (
            <p className="px-6 py-6 text-[13px] text-[var(--color-text-dim)]">
              {t("catalog.netease.playlistEmpty")}
            </p>
          ) : (
            <NeteaseTrackList
              tracks={detail.tracks}
              scrollRef={scrollRef}
              loadingMore={detail.partial}
              selection={
                selecting ? { selected, onChange: setSelected } : undefined
              }
            />
          )}
        </>
      ) : null}
    </div>
  );
}
