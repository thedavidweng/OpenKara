import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragOverEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { ChevronDown, ChevronUp, GripVertical, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  useCallback,
  useMemo,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { Tooltip } from "@/components/Overlay/Tooltip";
import { useLibraryStore } from "@/stores/library-store";
import { useQueueStore } from "@/stores/queue-store";
import { useRotationStore } from "@/stores/rotation-store";
import { RotationControls } from "./RotationControls";
import { SingerPickerDialog } from "./SingerPickerDialog";
import {
  getDropAnnouncementPosition,
  getDropIndicatorPosition,
  getVerticalTransform,
  type DropIndicatorPosition,
} from "./queue-dnd";

interface QueueItemCardProps {
  title: string;
  artist: string;
  singer?: string | null;
  singerLabel?: string;
  onSingerClick?: () => void;
  handle: ReactNode;
  controls?: ReactNode;
  removeAction?: ReactNode;
  dropIndicator?: DropIndicatorPosition | null;
  isOverlay?: boolean;
  isDraggingSource?: boolean;
  className?: string;
}

interface SortableQueueItemProps {
  songId: string;
  index: number;
  queueLength: number;
  title: string;
  artist: string;
  singer?: string | null;
  singerLabel?: string;
  onSingerClick?: () => void;
  moveUpLabel: string;
  moveDownLabel: string;
  dragLabel: string;
  removeLabel: string;
  dropIndicator: DropIndicatorPosition | null;
  onMoveUp: () => void;
  onMoveDown: () => void;
  onRemove: () => void;
}

function QueueItemCard({
  title,
  artist,
  singer,
  singerLabel,
  onSingerClick,
  handle,
  controls,
  removeAction,
  dropIndicator,
  isOverlay = false,
  isDraggingSource = false,
  className = "",
}: QueueItemCardProps) {
  const stateClassName = isOverlay
    ? "motion-safe:scale-[1.01] bg-[color-mix(in_srgb,var(--color-hover)_86%,transparent)] shadow-[0_20px_42px_rgba(0,0,0,0.34)] ring-1 ring-[color-mix(in_srgb,var(--color-accent)_65%,white)]"
    : isDraggingSource
      ? "bg-[color-mix(in_srgb,var(--color-hover)_80%,transparent)] opacity-25"
      : dropIndicator
        ? "bg-[color-mix(in_srgb,var(--color-hover)_80%,transparent)] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.04)]"
        : "hover:bg-[color-mix(in_srgb,var(--color-hover)_76%,transparent)]";

  return (
    <div
      className={`group motion-surface relative flex items-center gap-1.5 rounded-md border border-transparent px-3 py-1.5 ${stateClassName} ${className}`}
    >
      {dropIndicator && (
        <span
          className={`pointer-events-none absolute left-3 right-3 h-0.5 rounded-full bg-[var(--color-accent)] shadow-[0_0_12px_rgba(255,255,255,0.12)] ${
            dropIndicator === "above" ? "top-0" : "bottom-0"
          }`}
        />
      )}

      {handle}

      {controls}

      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[12px] font-medium text-[var(--color-control-primary)]">
          {title}
        </span>
        <span className="truncate text-[10px] text-[var(--color-text-dimmer)]">
          {artist}
        </span>
      </div>

      {singer !== undefined && (
        <button
          type="button"
          onClick={onSingerClick}
          className="shrink-0 rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] text-[var(--color-text-dim)] transition-colors hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
        >
          {singer || singerLabel}
        </button>
      )}

      {removeAction}
    </div>
  );
}

function SortableQueueItem({
  songId,
  index,
  queueLength,
  title,
  artist,
  singer,
  singerLabel,
  onSingerClick,
  moveUpLabel,
  moveDownLabel,
  dragLabel,
  removeLabel,
  dropIndicator,
  onMoveUp,
  onMoveDown,
  onRemove,
}: SortableQueueItemProps) {
  const {
    attributes,
    listeners,
    setActivatorNodeRef,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: songId });

  const verticalTransform = getVerticalTransform(transform);

  const style: CSSProperties = {
    transform: CSS.Transform.toString(verticalTransform),
    transition,
  };

  return (
    <div ref={setNodeRef} style={style} className="will-change-transform">
      <QueueItemCard
        title={title}
        artist={artist}
        singer={singer}
        singerLabel={singerLabel}
        onSingerClick={onSingerClick}
        dropIndicator={dropIndicator}
        isDraggingSource={isDragging}
        handle={
          <Tooltip label={dragLabel}>
            <button
              type="button"
              ref={setActivatorNodeRef}
              {...attributes}
              {...listeners}
              className={`motion-icon-button -m-1 shrink-0 rounded-md p-1 ${
                isDragging
                  ? "cursor-grabbing bg-[var(--color-ghost-hover)] text-[var(--color-control-primary)] shadow-[0_8px_18px_rgba(0,0,0,0.2)]"
                  : "cursor-grab text-[var(--color-text-dimmer)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-control-primary)] motion-safe:active:scale-95 active:cursor-grabbing focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-accent)]"
              }`}
              aria-label={dragLabel}
            >
              <GripVertical size={12} />
            </button>
          </Tooltip>
        }
        controls={
          <div className="-my-0.5 flex shrink-0 flex-col">
            <Tooltip label={moveUpLabel}>
              <button
                type="button"
                onClick={onMoveUp}
                disabled={index === 0}
                className="motion-icon-button rounded text-[var(--color-text-dimmer)] hover:text-[var(--color-control-primary)] disabled:opacity-20"
                aria-label={moveUpLabel}
              >
                <ChevronUp size={10} />
              </button>
            </Tooltip>
            <Tooltip label={moveDownLabel}>
              <button
                type="button"
                onClick={onMoveDown}
                disabled={index === queueLength - 1}
                className="motion-icon-button rounded text-[var(--color-text-dimmer)] hover:text-[var(--color-control-primary)] disabled:opacity-20"
                aria-label={moveDownLabel}
              >
                <ChevronDown size={10} />
              </button>
            </Tooltip>
          </div>
        }
        removeAction={
          <Tooltip label={removeLabel}>
            <button
              type="button"
              onClick={onRemove}
              className="contextual-reveal-horizontal motion-icon-button shrink-0 rounded-md text-[var(--color-text-dimmer)] hover:text-[var(--color-control-primary)] focus-visible:translate-x-0 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-accent)]"
              aria-label={removeLabel}
            >
              <X size={12} />
            </button>
          </Tooltip>
        }
      />
    </div>
  );
}

function DragOverlayQueueItem({
  title,
  artist,
  dragLabel,
}: Pick<SortableQueueItemProps, "title" | "artist" | "dragLabel">) {
  return (
    <QueueItemCard
      title={title}
      artist={artist}
      isOverlay
      className="w-[272px]"
      handle={
        <div
          className="-m-1 shrink-0 rounded-md bg-[var(--color-ghost-hover)] p-1 text-[var(--color-control-primary)] shadow-[0_8px_18px_rgba(0,0,0,0.2)]"
          aria-label={dragLabel}
        >
          <GripVertical size={12} />
        </div>
      }
    />
  );
}

export function QueuePanel() {
  const { t } = useTranslation();
  const queue = useQueueStore((s) => s.queue);
  const removeFromQueue = useQueueStore((s) => s.removeFromQueue);
  const reorder = useQueueStore((s) => s.reorder);
  const reorderBySongId = useQueueStore((s) => s.reorderBySongId);
  const clearQueue = useQueueStore((s) => s.clearQueue);
  const songs = useLibraryStore((s) => s.songs);
  const active = useRotationStore((s) => s.active);
  const singerNames = useRotationStore((s) => s.singerNames);
  const queueSingers = useRotationStore((s) => s.queueSingers);
  const filterSinger = useRotationStore((s) => s.filterSinger);
  const assignSingerToQueueEntry = useRotationStore(
    (s) => s.assignSingerToQueueEntry,
  );
  const assignSingerLabel = t("rotation.assignSinger");
  const [activeSongId, setActiveSongId] = useState<string | null>(null);
  const [overSongId, setOverSongId] = useState<string | null>(null);
  const [singerDialog, setSingerDialog] = useState<{
    open: boolean;
    songId: string | null;
    currentSinger: string;
  }>({ open: false, songId: null, currentSinger: "" });

  const handleSingerClick = useCallback(
    (songId: string) => {
      const current = queueSingers.get(songId) ?? "";
      setSingerDialog({ open: true, songId, currentSinger: current });
    },
    [queueSingers],
  );

  const handleAssignSinger = useCallback(
    (singer: string) => {
      if (singerDialog.songId !== null) {
        assignSingerToQueueEntry(singerDialog.songId, singer);
      }
      setSingerDialog({ open: false, songId: null, currentSinger: "" });
    },
    [singerDialog.songId, assignSingerToQueueEntry],
  );

  const handleRemoveSinger = useCallback(() => {
    if (singerDialog.songId !== null) {
      assignSingerToQueueEntry(singerDialog.songId, null);
    }
    setSingerDialog({ open: false, songId: null, currentSinger: "" });
  }, [singerDialog.songId, assignSingerToQueueEntry]);

  const handleCancelAssignSinger = useCallback(() => {
    setSingerDialog({ open: false, songId: null, currentSinger: "" });
  }, []);

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 6,
      },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const getSong = useCallback(
    (hash: string) => songs.find((song) => song.hash === hash),
    [songs],
  );

  const getSongLabel = useCallback(
    (songId: string) => getSong(songId)?.title || songId.slice(0, 8),
    [getSong],
  );

  const clearDragState = useCallback(() => {
    setActiveSongId(null);
    setOverSongId(null);
  }, []);

  const getAnnouncementMessage = useCallback(
    (phase: "over" | "end", activeId: string, overId: string) => {
      const position = getDropAnnouncementPosition(
        queue.indexOf(activeId),
        queue.indexOf(overId),
      );

      if (position === "after") {
        return String(
          t(phase === "over" ? "queue.dragOverAfter" : "queue.dragEndAfter", {
            title: getSongLabel(activeId),
            overTitle: getSongLabel(overId),
          }),
        );
      }

      return String(
        t(phase === "over" ? "queue.dragOverBefore" : "queue.dragEndBefore", {
          title: getSongLabel(activeId),
          overTitle: getSongLabel(overId),
        }),
      );
    },
    [getSongLabel, queue, t],
  );

  const accessibility = useMemo(
    () => ({
      screenReaderInstructions: {
        draggable: String(t("queue.dragInstructions")),
      },
      announcements: {
        onDragStart({ active }: { active: { id: string | number } }) {
          return String(
            t("queue.dragStart", {
              title: getSongLabel(String(active.id)),
            }),
          );
        },
        onDragOver({
          active,
          over,
        }: {
          active: { id: string | number };
          over: { id: string | number } | null;
        }) {
          if (!over) {
            return String(t("queue.dragCancel"));
          }

          return getAnnouncementMessage(
            "over",
            String(active.id),
            String(over.id),
          );
        },
        onDragEnd({
          active,
          over,
        }: {
          active: { id: string | number };
          over: { id: string | number } | null;
        }) {
          if (!over) {
            return String(t("queue.dragCancel"));
          }

          return getAnnouncementMessage(
            "end",
            String(active.id),
            String(over.id),
          );
        },
        onDragCancel() {
          return String(t("queue.dragCancel"));
        },
      },
    }),
    [getAnnouncementMessage, getSongLabel, t],
  );

  const handleDragStart = useCallback((event: DragStartEvent) => {
    const songId = String(event.active.id);
    setActiveSongId(songId);
    setOverSongId(songId);
  }, []);

  const handleDragOver = useCallback((event: DragOverEvent) => {
    setOverSongId(event.over ? String(event.over.id) : null);
  }, []);

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const activeId = String(event.active.id);
      const overId = event.over ? String(event.over.id) : null;

      if (overId && activeId !== overId) {
        reorderBySongId(activeId, overId);
      }

      clearDragState();
    },
    [clearDragState, reorderBySongId],
  );

  const activeIndex = activeSongId ? queue.indexOf(activeSongId) : null;
  const overIndex = overSongId ? queue.indexOf(overSongId) : null;
  const activeSong = activeSongId ? getSong(activeSongId) : undefined;

  const filteredQueue = useMemo(() => {
    if (!filterSinger) return queue;
    return queue.filter((songId) => queueSingers.get(songId) === filterSinger);
  }, [queue, filterSinger, queueSingers]);

  return (
    <div className="app-panel-surface flex h-full w-[280px] shrink-0 flex-col border-l border-[color-mix(in_srgb,var(--color-border)_86%,transparent)] bg-[color-mix(in_srgb,var(--color-toolbar)_94%,transparent)] shadow-[-1px_0_0_rgba(255,255,255,0.02)]">
      <div className="flex items-center justify-between border-b border-[color-mix(in_srgb,var(--color-border)_86%,transparent)] px-4 py-2">
        <span className="text-[13px] font-medium text-[var(--color-control-primary)]">
          {t("queue.upNext")}
          {queue.length > 0 && (
            <span className="ml-2 text-[var(--color-text-dimmer)]">
              (
              {filterSinger
                ? `${filteredQueue.length}/${queue.length}`
                : queue.length}
              )
            </span>
          )}
        </span>
        {queue.length > 0 && (
          <button
            type="button"
            onClick={clearQueue}
            className="motion-icon-button rounded px-1.5 py-1 text-[11px] text-[var(--color-text-dimmer)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-control-primary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-accent)]/30"
          >
            {t("queue.clearAll")}
          </button>
        )}
      </div>

      <RotationControls />

      <div className="custom-scrollbar flex-1 overflow-y-auto px-1.5 py-1">
        {filteredQueue.length === 0 ? (
          <div className="flex items-center justify-center py-8">
            <span className="text-[13px] text-[var(--color-text-dimmer)]">
              {t("queue.empty")}
            </span>
          </div>
        ) : (
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            accessibility={accessibility}
            onDragStart={handleDragStart}
            onDragOver={handleDragOver}
            onDragEnd={handleDragEnd}
            onDragCancel={clearDragState}
          >
            <SortableContext
              items={filteredQueue}
              strategy={verticalListSortingStrategy}
            >
              <div className="space-y-1">
                {filteredQueue.map((songId, index) => {
                  const song = getSong(songId);
                  const title = song?.title || songId.slice(0, 8);
                  const dropIndicator =
                    songId === overSongId
                      ? getDropIndicatorPosition(activeIndex, overIndex)
                      : null;

                  return (
                    <SortableQueueItem
                      key={songId}
                      songId={songId}
                      index={index}
                      queueLength={filteredQueue.length}
                      title={title}
                      artist={song?.artist || t("common.unknownArtist")}
                      singer={
                        active ? (queueSingers.get(songId) ?? null) : undefined
                      }
                      singerLabel={assignSingerLabel}
                      onSingerClick={
                        active ? () => handleSingerClick(songId) : undefined
                      }
                      moveUpLabel={t("queue.moveUp")}
                      moveDownLabel={t("queue.moveDown")}
                      dragLabel={String(t("queue.reorder", { title }))}
                      removeLabel={String(t("queue.remove", { title }))}
                      dropIndicator={dropIndicator}
                      onMoveUp={() => reorder(index, index - 1)}
                      onMoveDown={() => reorder(index, index + 1)}
                      onRemove={() => removeFromQueue(index)}
                    />
                  );
                })}
              </div>
            </SortableContext>

            <DragOverlay dropAnimation={null}>
              {activeSongId ? (
                <DragOverlayQueueItem
                  title={activeSong?.title || activeSongId.slice(0, 8)}
                  artist={activeSong?.artist || t("common.unknownArtist")}
                  dragLabel={String(
                    t("queue.reorder", {
                      title: activeSong?.title || activeSongId.slice(0, 8),
                    }),
                  )}
                />
              ) : null}
            </DragOverlay>
          </DndContext>
        )}
      </div>

      {singerDialog.open && (
        <SingerPickerDialog
          singerNames={singerNames}
          currentSinger={singerDialog.currentSinger || null}
          onSelect={handleAssignSinger}
          onRemove={handleRemoveSinger}
          onCancel={handleCancelAssignSinger}
        />
      )}
    </div>
  );
}
