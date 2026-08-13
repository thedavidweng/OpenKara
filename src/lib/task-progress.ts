import type { ParseKeys } from "i18next";
import type { Backend } from "@/lib/backend";
import { notifyError } from "@/lib/errors";
import { formatBytes } from "@/lib/format";
import { songDisplayTitle } from "@/lib/song-display";
import type {
  BatchSeparationProgress,
  ModelBootstrapState,
  ModelBootstrapStatusSnapshot,
  RuntimeBootstrapStatusSnapshot,
  SeparationStatusSnapshot,
  Song,
  UploadStatusSnapshot,
} from "@/types/ipc";

export const MODEL_DOWNLOAD_FLASH_MS = 2_800;

type TranslateFn = (
  key: ParseKeys,
  options?: Record<string, string | number>,
) => string;

export interface ActiveTask {
  key: string;
  label: string;
  detail?: string;
  percent: number;
  indeterminate?: boolean;
  onCancel?: () => void;
}

export interface TaskProgressInputs {
  t: TranslateFn;
  backend: Backend;
  modelBootstrap: ModelBootstrapStatusSnapshot | null;
  runtimeBootstrap: RuntimeBootstrapStatusSnapshot | null;
  modelDownloadCompleteFlash: boolean;
  separationStatuses: Record<string, SeparationStatusSnapshot>;
  uploadStatuses: Record<string, UploadStatusSnapshot>;
  batchSeparation: BatchSeparationProgress | null;
  songs: Song[];
}

const RUNTIME_POST_DOWNLOAD_LABELS = {
  installing: "bootstrap.installingRuntime",
  probing: "bootstrap.checkingRuntimeCompatibility",
  activating: "bootstrap.activatingRuntime",
} as const;

function downloadTask(
  key: string,
  label: string,
  downloaded: number | null,
  totalBytes: number | null,
): ActiveTask {
  const total = totalBytes != null && totalBytes > 0 ? totalBytes : null;

  return {
    key,
    label,
    detail:
      downloaded == null
        ? undefined
        : total == null
          ? formatBytes(downloaded)
          : `${formatBytes(downloaded)} / ${formatBytes(total)}`,
    percent:
      total != null && downloaded != null && downloaded >= 0
        ? Math.min(100, Math.max(0, (downloaded / total) * 100))
        : 0,
    indeterminate: total == null,
  };
}

/**
 * A batch separation is still running until every song has either completed or
 * failed; skipped songs are counted as completed by the backend. Every surface
 * that draws batch progress asks here, so the global bar, the sidebar footer
 * and the song rows cannot disagree about when the run is over.
 */
export function batchSeparationInProgress(
  batch: BatchSeparationProgress | null,
): boolean {
  return batch != null && batch.completed + batch.failed < batch.total;
}

export function batchSeparationLabelArgs(batch: BatchSeparationProgress): {
  current: number;
  total: number;
} {
  return {
    current: Math.min(batch.completed + 1, batch.total),
    total: batch.total,
  };
}

/**
 * A separation run contributes at most one global task: the aggregate while a
 * batch snapshot exists, otherwise the leading single-song run. Song rows draw
 * their own compact bar for the active batch song.
 */
function separationTask(inputs: TaskProgressInputs): ActiveTask | null {
  const { t, backend, batchSeparation, separationStatuses, songs } = inputs;

  if (batchSeparation != null) {
    if (!batchSeparationInProgress(batchSeparation)) return null;

    return {
      key: "batch-separation",
      label: t("sidebar.separating", batchSeparationLabelArgs(batchSeparation)),
      percent:
        ((batchSeparation.completed +
          (batchSeparation.current_percent ?? 0) / 100) /
          batchSeparation.total) *
        100,
      onCancel: () =>
        backend.maintenance.cancelBatchSeparation().catch(notifyError),
    };
  }

  const running = Object.values(separationStatuses).filter(
    (status) => status.state === "running",
  );
  if (running.length === 0) return null;

  const [leading] = running;
  return {
    key: `sep-${leading.song_id}`,
    label: t("sidebar.separating", { current: 1, total: running.length }),
    detail: songDisplayTitle(
      songs.find((song) => song.hash === leading.song_id),
    ),
    percent: leading.percent,
    onCancel: () =>
      backend.separation.cancelSeparation(leading.song_id).catch(notifyError),
  };
}

export function deriveActiveTasks(inputs: TaskProgressInputs): ActiveTask[] {
  const {
    t,
    modelBootstrap,
    runtimeBootstrap,
    modelDownloadCompleteFlash,
    uploadStatuses,
    songs,
  } = inputs;

  const tasks: ActiveTask[] = [];

  if (modelDownloadCompleteFlash && modelBootstrap?.state === "ready") {
    tasks.push({
      key: "model-download-complete",
      label: t("progress.modelDownloadComplete"),
      percent: 100,
    });
  }

  if (
    runtimeBootstrap?.state === "downloading" ||
    runtimeBootstrap?.state === "downloading_candidate"
  ) {
    tasks.push(
      downloadTask(
        "runtime-download",
        t("bootstrap.downloadingRuntime"),
        runtimeBootstrap.downloaded_bytes,
        runtimeBootstrap.total_bytes,
      ),
    );
  }

  if (
    runtimeBootstrap?.state === "installing" ||
    runtimeBootstrap?.state === "probing" ||
    runtimeBootstrap?.state === "activating"
  ) {
    tasks.push({
      key: "runtime-post-download",
      label: t(RUNTIME_POST_DOWNLOAD_LABELS[runtimeBootstrap.state]),
      percent: 0,
      indeterminate: true,
    });
  }

  if (modelBootstrap?.state === "downloading") {
    tasks.push(
      downloadTask(
        "model-download",
        t("bootstrap.downloadingModel"),
        modelBootstrap.downloaded_bytes,
        modelBootstrap.total_bytes,
      ),
    );
  }

  const separation = separationTask(inputs);
  if (separation != null) {
    tasks.push(separation);
  }

  for (const upload of Object.values(uploadStatuses)) {
    if (upload.state !== "running") continue;
    tasks.push({
      key: `upload-${upload.song_id}`,
      label: t("progress.uploadingToRemote", {
        title: songDisplayTitle(
          songs.find((song) => song.hash === upload.song_id),
        ),
      }),
      percent: upload.percent,
    });
  }

  return tasks;
}

export interface ModelDownloadFlash {
  observe(state: ModelBootstrapState | undefined): void;
  dispose(): void;
}

export function createModelDownloadFlash(
  emit: (visible: boolean) => void,
  flashMs: number = MODEL_DOWNLOAD_FLASH_MS,
): ModelDownloadFlash {
  let previous: ModelBootstrapState | undefined;
  let shown = false;
  let timers: ReturnType<typeof setTimeout>[] = [];

  const clearTimers = () => {
    timers.forEach((timer) => globalThis.clearTimeout(timer));
    timers = [];
  };

  const hide = () => {
    if (!shown) return;
    shown = false;
    emit(false);
  };

  return {
    observe(state) {
      if (state === previous) return;

      const downloadSettled = previous === "downloading" && state === "ready";
      previous = state;
      clearTimers();
      hide();
      if (!downloadSettled) return;

      timers.push(
        globalThis.setTimeout(() => {
          shown = true;
          emit(true);
          timers.push(globalThis.setTimeout(hide, flashMs));
        }, 0),
      );
    },
    dispose: clearTimers,
  };
}
