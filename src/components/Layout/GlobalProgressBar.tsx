import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { useLibraryStore } from "@/stores/library-store";
import { useBootstrapStore } from "@/stores/bootstrap-store";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import { formatBytes } from "@/lib/format";
import * as api from "@/lib/tauri";
import { notifyError } from "@/lib/errors";
import { songDisplayTitle } from "@/lib/song-display";
import type { ModelBootstrapState } from "@/types/ipc";

interface ActiveTask {
  key: string;
  label: string;
  detail?: string;
  percent: number;
  indeterminate?: boolean;
  onCancel?: () => void;
}

interface TaskProgressBarProps extends Omit<ActiveTask, "key"> {
  className?: string;
  compact?: boolean;
  ariaLabel?: string;
}

export function TaskProgressBar({
  label,
  detail,
  percent,
  indeterminate,
  onCancel,
  className,
  compact = false,
  ariaLabel,
}: TaskProgressBarProps) {
  const clampedPercent = Math.min(100, Math.max(0, percent));

  if (compact) {
    return (
      <div
        className={className ?? "w-full"}
        role="progressbar"
        aria-label={ariaLabel || label || undefined}
        aria-valuenow={indeterminate ? undefined : clampedPercent}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div className="h-1 w-full overflow-hidden rounded-full bg-[var(--color-border)]">
          {indeterminate ? (
            <div className="relative h-full w-full overflow-hidden rounded-full bg-[color-mix(in_srgb,var(--color-accent)_22%,transparent)]">
              <div className="model-indeterminate-bar absolute inset-y-0 left-0 rounded-full bg-[var(--color-accent)] will-change-transform" />
            </div>
          ) : (
            <div
              className="motion-surface h-full rounded-full bg-[var(--color-accent)]"
              style={{ width: `${clampedPercent}%` }}
            />
          )}
        </div>
      </div>
    );
  }

  return (
    <div className={className ?? "space-y-1"}>
      <div className="flex items-center justify-between">
        <span className="min-w-0 truncate text-[11px] text-[var(--color-text-dim)]">
          {label}
          {detail && (
            <span className="ml-1 text-[var(--color-text-dimmer)]">
              {detail}
            </span>
          )}
        </span>
        {onCancel && (
          <button
            onClick={onCancel}
            className="motion-icon-button shrink-0 rounded p-0.5 text-[var(--color-text-dim)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
          >
            <X size={12} />
          </button>
        )}
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--color-border)]">
        {indeterminate ? (
          <div className="relative h-full w-full overflow-hidden rounded-full bg-[color-mix(in_srgb,var(--color-accent)_22%,transparent)]">
            <div className="model-indeterminate-bar absolute inset-y-0 left-0 rounded-full bg-[var(--color-accent)] will-change-transform" />
          </div>
        ) : (
          <div
            className="motion-surface h-full rounded-full bg-[var(--color-accent)]"
            style={{ width: `${clampedPercent}%` }}
          />
        )}
      </div>
    </div>
  );
}

function useModelDownloadCompleteFlash(): boolean {
  const bootstrapState = useBootstrapStore((s) => s.status?.state);
  const [flash, setFlash] = useState(false);
  const prevRef = useRef<ModelBootstrapState | undefined>(undefined);

  useEffect(() => {
    const prev = prevRef.current;
    prevRef.current = bootstrapState;

    if (prev !== "downloading" || bootstrapState !== "ready") {
      return;
    }

    let hideTimer: number | null = null;
    const showTimer = window.setTimeout(() => {
      setFlash(true);
      hideTimer = window.setTimeout(() => setFlash(false), 2800);
    }, 0);

    return () => {
      window.clearTimeout(showTimer);
      if (hideTimer != null) {
        window.clearTimeout(hideTimer);
      }
    };
  }, [bootstrapState]);

  return flash;
}

function useActiveTasks(modelDownloadCompleteFlash: boolean): ActiveTask[] {
  const { t } = useTranslation();
  const bootstrapStatus = useBootstrapStore((s) => s.status);
  const runtimeStatus = useRuntimeBootstrapStore((s) => s.status);
  const separationStatuses = useLibraryStore((s) => s.separationStatuses);
  const uploadStatuses = useLibraryStore((s) => s.uploadStatuses);
  const batchSeparation = useLibraryStore((s) => s.batchSeparation);
  const songs = useLibraryStore((s) => s.songs);

  const tasks: ActiveTask[] = [];

  if (modelDownloadCompleteFlash && bootstrapStatus?.state === "ready") {
    tasks.push({
      key: "model-download-complete",
      label: t("progress.modelDownloadComplete"),
      percent: 100,
    });
  }

  if (
    runtimeStatus?.state === "downloading" ||
    runtimeStatus?.state === "downloading_candidate"
  ) {
    const total = runtimeStatus.total_bytes;
    const down = runtimeStatus.downloaded_bytes;
    const hasTotal = total != null && total > 0;
    const hasDown = down != null && down >= 0;
    const percent =
      hasTotal && hasDown
        ? Math.min(100, Math.max(0, (down / total) * 100))
        : 0;
    const indeterminate = !hasTotal;
    tasks.push({
      key: "runtime-download",
      label: t("bootstrap.downloadingRuntime"),
      detail:
        down != null
          ? formatBytes(down) + (hasTotal ? ` / ${formatBytes(total!)}` : "")
          : undefined,
      percent,
      indeterminate,
    });
  }

  if (bootstrapStatus?.state === "downloading") {
    const total = bootstrapStatus.total_bytes;
    const down = bootstrapStatus.downloaded_bytes;
    const hasTotal = total != null && total > 0;
    const hasDown = down != null && down >= 0;
    const percent =
      hasTotal && hasDown
        ? Math.min(100, Math.max(0, (down / total) * 100))
        : 0;
    const indeterminate = !hasTotal;
    tasks.push({
      key: "model-download",
      label: t("bootstrap.downloadingModel"),
      detail:
        down != null
          ? formatBytes(down) + (hasTotal ? ` / ${formatBytes(total!)}` : "")
          : undefined,
      percent,
      indeterminate,
    });
  }

  if (batchSeparation != null) {
    const done = batchSeparation.completed + batchSeparation.failed;
    if (done < batchSeparation.total) {
      const percent =
        ((batchSeparation.completed +
          (batchSeparation.current_percent ?? 0) / 100) /
          batchSeparation.total) *
        100;
      tasks.push({
        key: "batch-separation",
        label: t("sidebar.separating", {
          current: Math.min(
            batchSeparation.completed + 1,
            batchSeparation.total,
          ),
          total: batchSeparation.total,
        }),
        percent,
        onCancel: () => api.cancelBatchSeparation().catch(notifyError),
      });
    }
  }

  if (batchSeparation == null) {
    const runningSep = Object.values(separationStatuses).find(
      (s) => s.state === "running",
    );
    if (runningSep) {
      const song = songs.find((s) => s.hash === runningSep.song_id);
      const title = songDisplayTitle(song);
      tasks.push({
        key: `sep-${runningSep.song_id}`,
        label: t("progress.separating", { title }),
        percent: runningSep.percent,
        onCancel: () =>
          api.cancelSeparation(runningSep.song_id).catch(notifyError),
      });
    }
  }

  const runningUploads = Object.values(uploadStatuses).filter(
    (status) => status.state === "running",
  );

  for (const upload of runningUploads) {
    const song = songs.find((candidate) => candidate.hash === upload.song_id);
    const title = songDisplayTitle(song);
    tasks.push({
      key: `upload-${upload.song_id}`,
      label: t("progress.uploadingToRemote", {
        title,
      }),
      percent: upload.percent,
    });
  }

  return tasks;
}

export function GlobalProgressBar() {
  const modelDownloadCompleteFlash = useModelDownloadCompleteFlash();
  const tasks = useActiveTasks(modelDownloadCompleteFlash);

  if (tasks.length === 0) return null;

  return (
    <div className="z-10 space-y-2 bg-[var(--color-sidebar)] px-3 py-2">
      {tasks.map(({ key, ...task }) => (
        <TaskProgressBar key={key} {...task} />
      ))}
    </div>
  );
}
