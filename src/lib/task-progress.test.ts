import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createMockBackend } from "@/lib/backend/mock-backend";
import {
  batchSeparationInProgress,
  batchSeparationLabelArgs,
  createModelDownloadFlash,
  deriveActiveTasks,
  MODEL_DOWNLOAD_FLASH_MS,
  type TaskProgressInputs,
} from "./task-progress";
import type {
  BatchSeparationProgress,
  ModelBootstrapStatusSnapshot,
  RuntimeBootstrapStatusSnapshot,
  SeparationStatusSnapshot,
  Song,
  UploadStatusSnapshot,
} from "@/types/ipc";

vi.mock("@/lib/errors", () => ({ notifyError: vi.fn() }));

const cancelSeparation = vi.fn(() => Promise.resolve());
const cancelBatchSeparation = vi.fn(() => Promise.resolve());
const backend = createMockBackend({
  overrides: {
    separation: { cancelSeparation },
    maintenance: { cancelBatchSeparation },
  },
});

const t = (key: string, options?: Record<string, string | number>) =>
  options == null
    ? key
    : `${key}:${Object.entries(options)
        .map(([name, value]) => `${name}=${value}`)
        .join(",")}`;

function separationStatus(
  songId: string,
  overrides: Partial<SeparationStatusSnapshot> = {},
): SeparationStatusSnapshot {
  return {
    song_id: songId,
    state: "running",
    percent: 0,
    cache_hit: false,
    vocals_path: null,
    accomp_path: null,
    drums_path: null,
    bass_path: null,
    other_path: null,
    model_variant: null,
    error: null,
    ...overrides,
  };
}

function uploadStatus(
  songId: string,
  overrides: Partial<UploadStatusSnapshot> = {},
): UploadStatusSnapshot {
  return {
    song_id: songId,
    state: "running",
    percent: 0,
    remote_library_id: null,
    detail: null,
    error: null,
    ...overrides,
  };
}

function batchProgress(
  overrides: Partial<BatchSeparationProgress> = {},
): BatchSeparationProgress {
  return {
    total: 3,
    completed: 1,
    skipped: 0,
    failed: 0,
    current_song_id: "song-a",
    current_percent: 50,
    ...overrides,
  };
}

function runtimeStatus(
  overrides: Partial<RuntimeBootstrapStatusSnapshot>,
): RuntimeBootstrapStatusSnapshot {
  return {
    state: "downloading",
    runtime_path: "/tmp/runtime",
    downloaded_bytes: null,
    total_bytes: null,
    version: "v1.27.1",
    active_artifact_id: null,
    target_triple: "aarch64-apple-darwin",
    candidate_version: null,
    restart_required: false,
    error: null,
    ...overrides,
  };
}

function modelStatus(
  overrides: Partial<ModelBootstrapStatusSnapshot>,
): ModelBootstrapStatusSnapshot {
  return {
    state: "ready",
    model_path: "/models/htdemucs.onnx",
    downloaded_bytes: null,
    total_bytes: null,
    error: null,
    ...overrides,
  };
}

function song(hash: string, title: string | null): Song {
  return {
    hash,
    file_path: `/music/${hash}.mp3`,
    audio_source_kind: "original",
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language: null,
    title,
    artist: "Artist",
    album: null,
    duration_ms: 120_000,
    cover_art: null,
    has_cover_art: false,
    artwork_thumb_path: null,
    imported_at: 0,
    original_ext: "mp3",
  };
}

function inputs(
  overrides: Partial<TaskProgressInputs> = {},
): TaskProgressInputs {
  return {
    t,
    backend,
    modelBootstrap: null,
    runtimeBootstrap: null,
    modelDownloadCompleteFlash: false,
    separationStatuses: {},
    uploadStatuses: {},
    batchSeparation: null,
    songs: [],
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("batchSeparationInProgress", () => {
  test("is false without a batch snapshot", () => {
    expect(batchSeparationInProgress(null)).toBe(false);
  });

  test("is true while songs are still outstanding", () => {
    expect(
      batchSeparationInProgress(batchProgress({ total: 3, completed: 1 })),
    ).toBe(true);
  });

  test("is false once completions and failures account for the whole batch", () => {
    expect(
      batchSeparationInProgress(
        batchProgress({ total: 3, completed: 2, failed: 1 }),
      ),
    ).toBe(false);
  });

  test("counts failures as settled work rather than outstanding work", () => {
    expect(
      batchSeparationInProgress(
        batchProgress({ total: 3, completed: 0, failed: 3 }),
      ),
    ).toBe(false);
  });
});

describe("batchSeparationLabelArgs", () => {
  test("names the song being worked on as one past the completed count", () => {
    expect(
      batchSeparationLabelArgs(batchProgress({ total: 3, completed: 1 })),
    ).toEqual({ current: 2, total: 3 });
  });

  test("clamps the song counter to the batch total on the last song", () => {
    expect(
      batchSeparationLabelArgs(batchProgress({ total: 2, completed: 2 })),
    ).toEqual({ current: 2, total: 2 });
  });
});

describe("deriveActiveTasks separation de-duplication", () => {
  test("keeps a running batch to one aggregate task while per-song progress streams in", () => {
    const tasks = deriveActiveTasks(
      inputs({
        batchSeparation: batchProgress(),
        separationStatuses: {
          "song-a": separationStatus("song-a", { percent: 50 }),
        },
        songs: [song("song-a", "Song A")],
      }),
    );

    expect(tasks.map((task) => task.key)).toEqual(["batch-separation"]);
    expect(tasks[0].percent).toBe(50);
    expect(tasks[0].label).toBe("sidebar.separating:current=2,total=3");
  });

  test("clamps the aggregate song counter to the batch total on the last song", () => {
    const tasks = deriveActiveTasks(
      inputs({
        batchSeparation: batchProgress({
          total: 2,
          completed: 1,
          failed: 0,
          current_percent: 0,
        }),
      }),
    );

    expect(tasks[0].label).toBe("sidebar.separating:current=2,total=2");
  });

  test("drops every separation task while a finished batch snapshot lingers", () => {
    const tasks = deriveActiveTasks(
      inputs({
        batchSeparation: batchProgress({
          total: 2,
          completed: 1,
          failed: 1,
          current_percent: 100,
        }),
        separationStatuses: {
          "song-a": separationStatus("song-a", { percent: 90 }),
        },
      }),
    );

    expect(tasks).toEqual([]);
  });

  test("surfaces the leading single-song run with its title and running count", () => {
    const tasks = deriveActiveTasks(
      inputs({
        separationStatuses: {
          "song-a": separationStatus("song-a", { percent: 20 }),
          "song-b": separationStatus("song-b", { percent: 80 }),
          "song-c": separationStatus("song-c", { state: "completed" }),
        },
        songs: [song("song-a", "Song A"), song("song-b", "Song B")],
      }),
    );

    expect(tasks).toHaveLength(1);
    expect(tasks[0]).toMatchObject({
      key: "sep-song-a",
      label: "sidebar.separating:current=1,total=2",
      detail: "Song A",
      percent: 20,
    });
  });

  test("falls back to the file name when the running song has no title", () => {
    const tasks = deriveActiveTasks(
      inputs({
        separationStatuses: { "song-a": separationStatus("song-a") },
        songs: [song("song-a", null)],
      }),
    );

    expect(tasks[0].detail).toBe("song-a.mp3");
  });

  test("routes both cancel affordances through the backend", () => {
    const [batchTask] = deriveActiveTasks(
      inputs({ batchSeparation: batchProgress() }),
    );
    batchTask.onCancel?.();
    expect(cancelBatchSeparation).toHaveBeenCalledTimes(1);

    const [songTask] = deriveActiveTasks(
      inputs({
        separationStatuses: { "song-a": separationStatus("song-a") },
      }),
    );
    songTask.onCancel?.();
    expect(cancelSeparation).toHaveBeenCalledWith("song-a");
  });
});

describe("deriveActiveTasks download tasks", () => {
  test("reports runtime download bytes and percent when the total is known", () => {
    const tasks = deriveActiveTasks(
      inputs({
        runtimeBootstrap: runtimeStatus({
          downloaded_bytes: 3_200_000,
          total_bytes: 6_400_000,
        }),
      }),
    );

    expect(tasks).toEqual([
      {
        key: "runtime-download",
        label: "bootstrap.downloadingRuntime",
        detail: "3.1 MB / 6.1 MB",
        percent: 50,
        indeterminate: false,
      },
    ]);
  });

  test("marks a runtime download with an unknown total as indeterminate", () => {
    const tasks = deriveActiveTasks(
      inputs({
        runtimeBootstrap: runtimeStatus({
          state: "downloading_candidate",
          downloaded_bytes: 1_024,
          total_bytes: 0,
        }),
      }),
    );

    expect(tasks[0]).toMatchObject({
      detail: "1.0 KB",
      percent: 0,
      indeterminate: true,
    });
  });

  test("omits the byte readout before the first progress event", () => {
    const tasks = deriveActiveTasks(
      inputs({
        runtimeBootstrap: runtimeStatus({ total_bytes: 6_400_000 }),
      }),
    );

    expect(tasks[0].detail).toBeUndefined();
    expect(tasks[0].percent).toBe(0);
  });

  test.each([
    ["installing", "bootstrap.installingRuntime"],
    ["probing", "bootstrap.checkingRuntimeCompatibility"],
    ["activating", "bootstrap.activatingRuntime"],
  ] as const)("names the indeterminate runtime %s phase", (state, label) => {
    const tasks = deriveActiveTasks(
      inputs({ runtimeBootstrap: runtimeStatus({ state }) }),
    );

    expect(tasks).toEqual([
      {
        key: "runtime-post-download",
        label,
        percent: 0,
        indeterminate: true,
      },
    ]);
  });

  test("reports model download progress", () => {
    const tasks = deriveActiveTasks(
      inputs({
        modelBootstrap: modelStatus({
          state: "downloading",
          downloaded_bytes: 512,
          total_bytes: 2_048,
        }),
      }),
    );

    expect(tasks[0]).toMatchObject({
      key: "model-download",
      label: "bootstrap.downloadingModel",
      detail: "512 B / 2.0 KB",
      percent: 25,
    });
  });
});

describe("deriveActiveTasks completion flash and uploads", () => {
  test("shows the completion flash only while the model is ready", () => {
    const ready = deriveActiveTasks(
      inputs({
        modelDownloadCompleteFlash: true,
        modelBootstrap: modelStatus({ state: "ready" }),
      }),
    );
    expect(ready).toEqual([
      {
        key: "model-download-complete",
        label: "progress.modelDownloadComplete",
        percent: 100,
      },
    ]);

    const stillDownloading = deriveActiveTasks(
      inputs({
        modelDownloadCompleteFlash: true,
        modelBootstrap: modelStatus({ state: "downloading" }),
      }),
    );
    expect(stillDownloading.map((task) => task.key)).toEqual([
      "model-download",
    ]);

    const settled = deriveActiveTasks(
      inputs({ modelBootstrap: modelStatus({ state: "ready" }) }),
    );
    expect(settled).toEqual([]);
  });

  test("names one upload task per running upload", () => {
    const tasks = deriveActiveTasks(
      inputs({
        uploadStatuses: {
          "song-a": uploadStatus("song-a", { percent: 67 }),
          "song-b": uploadStatus("song-b", { state: "completed" }),
          "song-c": uploadStatus("song-c", { percent: 12 }),
        },
        songs: [song("song-a", "Song A"), song("song-c", "Song C")],
      }),
    );

    expect(tasks).toEqual([
      {
        key: "upload-song-a",
        label: "progress.uploadingToRemote:title=Song A",
        percent: 67,
      },
      {
        key: "upload-song-c",
        label: "progress.uploadingToRemote:title=Song C",
        percent: 12,
      },
    ]);
  });

  test("orders the flash, runtime, model, separation and upload tasks", () => {
    const tasks = deriveActiveTasks(
      inputs({
        modelDownloadCompleteFlash: true,
        modelBootstrap: modelStatus({ state: "ready" }),
        runtimeBootstrap: runtimeStatus({
          downloaded_bytes: 1,
          total_bytes: 2,
        }),
        separationStatuses: { "song-a": separationStatus("song-a") },
        uploadStatuses: { "song-b": uploadStatus("song-b") },
        songs: [song("song-a", "Song A"), song("song-b", "Song B")],
      }),
    );

    expect(tasks.map((task) => task.key)).toEqual([
      "model-download-complete",
      "runtime-download",
      "sep-song-a",
      "upload-song-b",
    ]);
  });
});

describe("createModelDownloadFlash", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  test("shows the flash once a download settles, then hides it", () => {
    vi.useFakeTimers();
    const emit = vi.fn();
    const flash = createModelDownloadFlash(emit);

    flash.observe("downloading");
    flash.observe("ready");
    expect(emit).not.toHaveBeenCalled();

    vi.advanceTimersByTime(0);
    expect(emit).toHaveBeenNthCalledWith(1, true);

    vi.advanceTimersByTime(MODEL_DOWNLOAD_FLASH_MS - 1);
    expect(emit).toHaveBeenCalledTimes(1);

    vi.advanceTimersByTime(1);
    expect(emit).toHaveBeenNthCalledWith(2, false);
  });

  test("ignores repeated states and transitions that are not a finished download", () => {
    vi.useFakeTimers();
    const emit = vi.fn();
    const flash = createModelDownloadFlash(emit);

    flash.observe("ready");
    flash.observe("ready");
    flash.observe("downloading");
    flash.observe("failed");
    vi.advanceTimersByTime(MODEL_DOWNLOAD_FLASH_MS * 2);

    expect(emit).not.toHaveBeenCalled();
  });

  test("cancels a pending settle when the model state changes again", () => {
    vi.useFakeTimers();
    const emit = vi.fn();
    const flash = createModelDownloadFlash(emit);

    flash.observe("downloading");
    flash.observe("ready");
    vi.advanceTimersByTime(0);
    flash.observe("failed");
    vi.advanceTimersByTime(MODEL_DOWNLOAD_FLASH_MS * 2);

    expect(emit.mock.calls).toEqual([[true]]);
  });

  test("stops a scheduled flash when disposed", () => {
    vi.useFakeTimers();
    const emit = vi.fn();
    const flash = createModelDownloadFlash(emit);

    flash.observe("downloading");
    flash.observe("ready");
    flash.dispose();
    vi.advanceTimersByTime(MODEL_DOWNLOAD_FLASH_MS * 2);

    expect(emit).not.toHaveBeenCalled();
  });
});
