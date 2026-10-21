import {
  memo,
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { AlertCircle, Check, Download } from "lucide-react";
import { useTranslation } from "react-i18next";
import { StreamingCover } from "@/components/Catalog/StreamingCover";
import { formatDuration } from "@/components/Catalog/streaming-utils";
import {
  playbackIdFor,
  usePlayPreview,
} from "@/components/Catalog/use-play-preview";
import { sourceAllows, useCatalogStore } from "@/stores/catalog-store";
import { usePlayerStore } from "@/stores/player-store";
import type { StreamingImportTrackState, StreamingTrack } from "@/types/ipc";
import { onlinePreviewQueueId } from "@/playback";

const focusRing =
  "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]";

const ROW_HEIGHT_PX = 56;

/** A track can be picked for import when nothing blocks or already covers it. */
function isTrackSelectable(
  track: StreamingTrack,
  state: StreamingImportTrackState | undefined,
): boolean {
  if (track.refusal) return false;
  if (track.library_song_id) return false;
  if (!state) return true;
  return state.kind === "failed" || state.kind === "skipped";
}

function ProgressRing({
  percent,
  label,
  dim,
}: {
  percent: number | null;
  label: string;
  dim?: boolean;
}) {
  const radius = 9;
  const circumference = 2 * Math.PI * radius;
  const known = percent !== null;
  return (
    <span
      role="progressbar"
      aria-label={label}
      aria-valuemin={known ? 0 : undefined}
      aria-valuemax={known ? 100 : undefined}
      aria-valuenow={known ? percent : undefined}
      title={label}
      className={`relative inline-flex size-7 items-center justify-center ${dim ? "opacity-50" : ""}`}
    >
      <svg
        viewBox="0 0 24 24"
        aria-hidden="true"
        className={`size-7 -rotate-90 ${known ? "" : "animate-spin"}`}
      >
        <circle
          cx="12"
          cy="12"
          r={radius}
          fill="none"
          strokeWidth="2.5"
          className="stroke-[var(--color-border-light)]"
        />
        <circle
          cx="12"
          cy="12"
          r={radius}
          fill="none"
          strokeWidth="2.5"
          strokeLinecap="round"
          className="stroke-[var(--color-accent)]"
          strokeDasharray={circumference}
          strokeDashoffset={
            known ? circumference * (1 - percent / 100) : circumference * 0.7
          }
        />
      </svg>
      {known ? (
        <span
          aria-hidden="true"
          className="absolute text-[8px] font-medium tabular-nums text-[var(--color-text-dim)]"
        >
          {percent}
        </span>
      ) : null}
    </span>
  );
}

const actionButton = `inline-flex size-8 items-center justify-center rounded-full text-[var(--color-text-dim)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)] disabled:opacity-40 ${focusRing}`;

function RowAction({
  track,
  state,
  busy,
}: {
  track: StreamingTrack;
  state: StreamingImportTrackState | undefined;
  busy: boolean;
}) {
  const { t } = useTranslation();
  const importTracks = useCatalogStore((s) => s.importTracks);

  if (track.refusal) return null;
  if (state?.kind === "queued") {
    return (
      <ProgressRing
        percent={null}
        dim
        label={t("catalog.streaming.statusQueued")}
      />
    );
  }
  if (state?.kind === "downloading") {
    const percent = state.percent ?? null;
    return (
      <ProgressRing
        percent={percent}
        label={
          percent === null
            ? t("catalog.streaming.statusDownloadingUnknown")
            : t("catalog.streaming.statusDownloading", { percent })
        }
      />
    );
  }
  if (state?.kind === "importing") {
    return (
      <ProgressRing
        percent={null}
        label={t("catalog.streaming.statusImporting")}
      />
    );
  }
  if (state?.kind === "imported" || track.library_song_id) {
    return (
      <span
        role="img"
        aria-label={t("catalog.streaming.statusImported")}
        title={t("catalog.streaming.statusImported")}
        className="inline-flex size-8 items-center justify-center text-[var(--color-success)]"
      >
        <Check size={16} aria-hidden="true" />
      </span>
    );
  }
  const retry = state?.kind === "failed" || state?.kind === "skipped";
  const failedLabel =
    state?.kind === "failed" && state.reason === "cancelled"
      ? t("catalog.streaming.statusCancelled")
      : t("catalog.streaming.statusFailed");
  return (
    <button
      type="button"
      disabled={busy}
      title={retry ? failedLabel : t("catalog.streaming.importTrack")}
      aria-label={
        retry
          ? t("catalog.streaming.retryImportLabel", { title: track.title })
          : t("catalog.streaming.importTrackLabel", { title: track.title })
      }
      onClick={() => void importTracks([track.remote_track_id])}
      className={`${actionButton} ${
        retry
          ? "text-[var(--color-destructive)]"
          : "opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
      }`}
    >
      {retry ? (
        <AlertCircle size={16} aria-hidden="true" />
      ) : (
        <Download size={16} aria-hidden="true" />
      )}
    </button>
  );
}

interface RowProps {
  track: StreamingTrack;
  index: number;
  top: number;
  selecting: boolean;
  checked: boolean;
  onToggle: (index: number, checked: boolean, shiftKey: boolean) => void;
  onPlay: (index: number) => void;
  onFocusRow: (index: number) => void;
  tabbable: boolean;
}

const StreamingTrackRow = memo(function StreamingTrackRow({
  track,
  index,
  top,
  selecting,
  checked,
  onToggle,
  onPlay,
  onFocusRow,
  tabbable,
}: RowProps) {
  const { t } = useTranslation();
  const state = useCatalogStore(
    (s) =>
      s.trackStates[
        onlinePreviewQueueId(track.source_id, track.remote_track_id)
      ],
  );
  const busy = useCatalogStore((s) => s.importRun?.phase === "running");
  const canImport = useCatalogStore((s) =>
    sourceAllows(
      s.sources.find((source) => source.source_id === track.source_id),
      "import",
      s.sourceStates[track.source_id]?.session,
    ),
  );
  const playing = usePlayerStore(
    (s) => s.snapshot?.song_id === playbackIdFor(track),
  );
  const unavailable = !!track.refusal && !track.library_song_id;
  const canSelect = isTrackSelectable(track, state);
  const reason =
    unavailable && track.refusal
      ? track.refusal.reason === "trial_clip"
        ? t("catalog.streaming.statusTrialHint")
        : track.refusal.reason === "no_play_rights"
          ? t("catalog.streaming.statusNoRightsHint")
          : t(`catalog.streaming.refusalReason.${track.refusal.reason}`)
      : null;

  return (
    <div
      role="row"
      aria-selected={selecting ? checked : undefined}
      aria-rowindex={index + 1}
      data-testid="streaming-track-row"
      data-unavailable={unavailable ? "true" : undefined}
      data-playing={playing ? "true" : undefined}
      aria-current={playing ? "true" : undefined}
      tabIndex={tabbable ? 0 : -1}
      data-row-index={index}
      onDoubleClick={() => onPlay(index)}
      onFocus={(event) => {
        if (event.target === event.currentTarget) onFocusRow(index);
      }}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key === "Enter") {
          event.preventDefault();
          onPlay(index);
        } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          const delta = event.key === "ArrowDown" ? 1 : -1;
          const list = event.currentTarget.parentElement;
          list
            ?.querySelector<HTMLElement>(`[data-row-index="${index + delta}"]`)
            ?.focus();
        }
      }}
      className={`group absolute left-0 top-0 flex w-full items-center gap-3.5 rounded-[10px] px-2 hover:bg-[var(--color-ghost-hover)] ${focusRing} ${
        playing ? "bg-[var(--color-ghost-hover)]" : ""
      }`}
      style={{ height: ROW_HEIGHT_PX, transform: `translateY(${top}px)` }}
    >
      {selecting ? (
        <div role="gridcell" className="flex shrink-0 items-center">
          <input
            type="checkbox"
            aria-label={t("catalog.streaming.selectRow", {
              title: track.title,
            })}
            checked={checked}
            disabled={!canSelect}
            onChange={() => {}}
            onClick={(event) =>
              onToggle(index, event.currentTarget.checked, event.shiftKey)
            }
            className={`size-4 shrink-0 accent-[var(--color-accent)] ${focusRing}`}
          />
        </div>
      ) : null}
      <div
        role="gridcell"
        className={`flex min-w-0 flex-1 items-center gap-3.5 ${unavailable ? "opacity-30" : ""}`}
      >
        <StreamingCover
          url={track.cover_url}
          size={80}
          muted={unavailable}
          className="size-10 rounded-md"
        />
        <div className="min-w-0 flex-[1.4]" title={reason ?? undefined}>
          <div
            className={`truncate text-[15px] font-semibold ${
              playing
                ? "text-[var(--color-accent)]"
                : "text-[var(--color-text)]"
            }`}
          >
            {track.title}
          </div>
          <div className="truncate text-[12px] text-[var(--color-text-dim)]">
            {track.artist}
          </div>
          {reason ? <span className="sr-only">{reason}</span> : null}
        </div>
        <div className="hidden min-w-0 flex-1 truncate text-[13px] text-[var(--color-text-dim)] md:block">
          {track.album ?? ""}
        </div>
        <div className="w-12 shrink-0 text-right text-[13px] tabular-nums text-[var(--color-text-dim)]">
          {formatDuration(track.duration_ms)}
        </div>
      </div>
      <div
        role="gridcell"
        className="flex w-16 shrink-0 items-center justify-end"
      >
        {canImport ? (
          <RowAction track={track} state={state} busy={busy} />
        ) : null}
      </div>
    </div>
  );
});

export interface TrackSelection {
  selected: ReadonlySet<string>;
  onChange: (next: Set<string>) => void;
}

interface StreamingTrackListProps {
  tracks: StreamingTrack[];
  /** The element that scrolls this list; the list virtualizes against it. */
  scrollRef: RefObject<HTMLElement | null>;
  /** When present, rows show checkboxes (selection mode). */
  selection?: TrackSelection;
  /** More tracks are still loading; the list grows as pages arrive. */
  loadingMore?: boolean;
}

export function StreamingTrackList({
  tracks,
  scrollRef,
  selection,
  loadingMore,
}: StreamingTrackListProps) {
  const { t } = useTranslation();
  const hideUnavailable = useCatalogStore((s) => s.hideUnavailable);
  const setHideUnavailable = useCatalogStore((s) => s.setHideUnavailable);
  const listRef = useRef<HTMLDivElement>(null);
  const [scrollMargin, setScrollMargin] = useState(0);
  // The scroller is an ancestor whose ref attaches after this component's
  // first layout effects; one extra render lets the virtualizer find it.
  const [, setMounted] = useState(false);
  useLayoutEffect(() => setMounted(true), []);
  const lastClicked = useRef<number | null>(null);
  const playPreview = usePlayPreview();
  const [focusIndex, setFocusIndex] = useState(0);
  const headerBox = useRef<HTMLInputElement>(null);

  const unavailableCount = tracks.reduce(
    (total, track) => total + (track.refusal && !track.library_song_id ? 1 : 0),
    0,
  );
  const visible = hideUnavailable
    ? tracks.filter((track) => !track.refusal || track.library_song_id)
    : tracks;

  const virtualizer = useVirtualizer({
    count: visible.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT_PX,
    overscan: 10,
    scrollMargin,
    getItemKey: (index) => {
      const track = visible[index];
      return track
        ? onlinePreviewQueueId(track.source_id, track.remote_track_id)
        : index;
    },
  });

  // The list sits below other content in the same scroller; keep its offset
  // in sync so virtual rows line up with the scroll position.
  const toolbarShown = unavailableCount > 0 || !!selection;
  useLayoutEffect(() => {
    const list = listRef.current;
    const scroller = scrollRef.current;
    if (!list || !scroller) return;
    const offset = Math.round(
      list.getBoundingClientRect().top -
        scroller.getBoundingClientRect().top +
        scroller.scrollTop,
    );
    setScrollMargin((current) => (current === offset ? current : offset));
  }, [scrollRef, toolbarShown, visible.length, hideUnavailable]);

  const trackStates = useCatalogStore.getState().trackStates;
  const selectableIds = selection
    ? visible
        .filter((track) =>
          isTrackSelectable(
            track,
            trackStates[
              onlinePreviewQueueId(track.source_id, track.remote_track_id)
            ],
          ),
        )
        .map((track) => track.remote_track_id)
    : [];
  const selectedCount = selection
    ? selectableIds.filter((id) => selection.selected.has(id)).length
    : 0;
  const allSelected =
    selectableIds.length > 0 && selectedCount === selectableIds.length;

  useLayoutEffect(() => {
    if (headerBox.current) {
      headerBox.current.indeterminate = selectedCount > 0 && !allSelected;
    }
  }, [selectedCount, allSelected]);

  const latest = useRef({ visible, selection });
  latest.current = { visible, selection };

  const onPlay = useCallback(
    (index: number) => {
      void playPreview(latest.current.visible, index);
    },
    [playPreview],
  );

  const onToggle = useCallback(
    (index: number, checked: boolean, shiftKey: boolean) => {
      const { visible: rows, selection: current } = latest.current;
      if (!current) return;
      const states = useCatalogStore.getState().trackStates;
      const next = new Set(current.selected);
      const anchor = lastClicked.current;
      const range =
        shiftKey && anchor != null
          ? rows.slice(Math.min(anchor, index), Math.max(anchor, index) + 1)
          : [rows[index]];
      for (const track of range) {
        if (
          !track ||
          !isTrackSelectable(
            track,
            states[
              onlinePreviewQueueId(track.source_id, track.remote_track_id)
            ],
          )
        ) {
          continue;
        }
        if (checked) next.add(track.remote_track_id);
        else next.delete(track.remote_track_id);
      }
      lastClicked.current = index;
      current.onChange(next);
    },
    [],
  );

  return (
    <div>
      {toolbarShown ? (
        <div className="flex min-h-9 items-center gap-4 px-6 py-1 text-[12px] text-[var(--color-text-dim)]">
          {selection ? (
            <label className="flex items-center gap-2 pl-2">
              <input
                ref={headerBox}
                type="checkbox"
                checked={allSelected}
                disabled={selectableIds.length === 0}
                onChange={(event) =>
                  selection.onChange(
                    event.target.checked ? new Set(selectableIds) : new Set(),
                  )
                }
                className={`size-4 accent-[var(--color-accent)] ${focusRing}`}
              />
              {t("catalog.streaming.selectAll")}
            </label>
          ) : null}
          {unavailableCount > 0 ? (
            <div className="ml-auto flex items-center gap-2">
              {hideUnavailable ? (
                <span id="streaming-hidden-count">
                  {t("catalog.streaming.unavailableHidden", {
                    hidden: unavailableCount,
                  })}
                </span>
              ) : null}
              <button
                type="button"
                role="switch"
                aria-checked={hideUnavailable}
                aria-describedby={
                  hideUnavailable ? "streaming-hidden-count" : undefined
                }
                onClick={() => setHideUnavailable(!hideUnavailable)}
                className={`inline-flex min-h-7 items-center gap-2 rounded-full px-2 text-[var(--color-text)] hover:bg-[var(--color-ghost-hover)] ${focusRing}`}
              >
                <span
                  aria-hidden="true"
                  className={`relative inline-block h-4 w-7 rounded-full transition-colors ${
                    hideUnavailable
                      ? "bg-[var(--color-accent)]"
                      : "bg-[var(--color-border-light)]"
                  }`}
                >
                  <span
                    className={`absolute top-0.5 size-3 rounded-full bg-white transition-[left] ${
                      hideUnavailable ? "left-3.5" : "left-0.5"
                    }`}
                  />
                </span>
                {t("catalog.streaming.hideUnavailable")}
              </button>
            </div>
          ) : null}
        </div>
      ) : null}
      <div
        ref={listRef}
        role="grid"
        aria-label={t("catalog.streaming.trackListLabel")}
        aria-multiselectable={selection ? true : undefined}
        aria-rowcount={visible.length}
        aria-colcount={selection ? 3 : 2}
        className="relative mx-4"
        style={{ height: `${virtualizer.getTotalSize()}px` }}
      >
        {virtualizer.getVirtualItems().map((item) => {
          const track = visible[item.index];
          if (!track) return null;
          return (
            <StreamingTrackRow
              key={onlinePreviewQueueId(track.source_id, track.remote_track_id)}
              track={track}
              index={item.index}
              top={item.start - scrollMargin}
              selecting={!!selection}
              checked={!!selection?.selected.has(track.remote_track_id)}
              onToggle={onToggle}
              onPlay={onPlay}
              onFocusRow={setFocusIndex}
              tabbable={item.index === Math.min(focusIndex, visible.length - 1)}
            />
          );
        })}
      </div>
      {loadingMore ? (
        <p
          role="status"
          className="mx-4 px-2 py-3 text-[12px] text-[var(--color-text-dim)]"
        >
          {t("catalog.streaming.loadingMore")}
        </p>
      ) : null}
    </div>
  );
}
