// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { GlobalProgressBar, TaskProgressBar } from "./GlobalProgressBar";
import * as api from "@/lib/tauri";
import type {
  RuntimeBootstrapStatusSnapshot,
  SeparationStatusSnapshot,
  Song,
  UploadStatusSnapshot,
} from "@/types/ipc";

const { mockLibraryState, mockBootstrapState, mockRuntimeState } = vi.hoisted(
  () => ({
    mockLibraryState: {
      separationStatuses: {} as Record<string, SeparationStatusSnapshot>,
      uploadStatuses: {} as Record<string, UploadStatusSnapshot>,
      batchSeparation: null as null,
      songs: [] as Song[],
    },
    mockBootstrapState: {
      status: null as null,
    },
    mockRuntimeState: {
      status: null as RuntimeBootstrapStatusSnapshot | null,
    },
  }),
);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, string | number>) =>
      vars?.title ? `${key}:${vars.title}` : key,
  }),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (selector: (state: typeof mockLibraryState) => unknown) =>
    selector(mockLibraryState),
}));

vi.mock("@/stores/bootstrap-store", () => ({
  useBootstrapStore: (
    selector: (state: typeof mockBootstrapState) => unknown,
  ) => selector(mockBootstrapState),
}));

vi.mock("@/stores/runtime-bootstrap-store", () => ({
  useRuntimeBootstrapStore: (
    selector: (state: typeof mockRuntimeState) => unknown,
  ) => selector(mockRuntimeState),
}));

vi.mock("@/lib/tauri", () => ({
  cancelBatchSeparation: vi.fn(() => Promise.resolve()),
  cancelSeparation: vi.fn(() => Promise.resolve()),
}));

vi.mock("@/lib/errors", () => ({
  notifyError: vi.fn(),
}));

describe("GlobalProgressBar", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  test("cancels a single-song separation when its cancel affordance is clicked", () => {
    mockLibraryState.batchSeparation = null;
    mockLibraryState.uploadStatuses = {};
    mockLibraryState.separationStatuses = {
      "song-cancel": {
        song_id: "song-cancel",
        state: "running",
        percent: 20,
        cache_hit: false,
        vocals_path: null,
        accomp_path: null,
        drums_path: null,
        bass_path: null,
        other_path: null,
        model_variant: null,
        error: null,
      },
    };
    mockLibraryState.songs = [];

    const { getByRole } = render(<GlobalProgressBar />);
    fireEvent.click(getByRole("button"));

    expect(api.cancelSeparation).toHaveBeenCalledWith("song-cancel");

    mockLibraryState.separationStatuses = {};
  });
  test("renders separation and upload tasks with the shared task bar", () => {
    mockLibraryState.separationStatuses = {
      "song-separate": {
        song_id: "song-separate",
        state: "running",
        percent: 42,
        cache_hit: false,
        vocals_path: null,
        accomp_path: null,
        drums_path: null,
        bass_path: null,
        other_path: null,
        model_variant: null,
        error: null,
      },
    };
    mockLibraryState.uploadStatuses = {
      "song-upload": {
        song_id: "song-upload",
        state: "running",
        percent: 67,
        remote_library_id: null,
        detail: null,
        error: null,
      },
    };
    mockLibraryState.songs = [
      {
        hash: "song-separate",
        file_path: "/music/separate.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        title: "Separate Song",
        artist: "Artist",
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
      {
        hash: "song-upload",
        file_path: "/music/upload.mp3",
        audio_source_kind: "original",
        cdg_path: null,
        media_g_container: null,
        instrumental: false,
        language: null,
        title: "Upload Song",
        artist: "Artist",
        album: null,
        duration_ms: 120000,
        cover_art: null,
        has_cover_art: false,
        artwork_thumb_path: null,
        imported_at: 0,
        original_ext: "mp3",
      },
    ];

    const markup = renderToStaticMarkup(<GlobalProgressBar />);

    expect(markup).toContain("progress.separating:Separate Song");
    expect(markup).toContain("progress.uploadingToRemote:Upload Song");
    expect(markup).toContain("motion-surface");
  });

  test("shows a named runtime download task with live progress while the runtime downloads", () => {
    mockLibraryState.batchSeparation = null;
    mockLibraryState.separationStatuses = {};
    mockLibraryState.uploadStatuses = {};
    mockLibraryState.songs = [];
    mockRuntimeState.status = {
      state: "downloading",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: 3_200_000,
      total_bytes: 6_400_000,
      version: "v1.27.1",
      active_artifact_id: null,
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
      error: null,
    };

    const markup = renderToStaticMarkup(<GlobalProgressBar />);

    expect(markup).toContain("bootstrap.downloadingRuntime");
    expect(markup).toContain("width:50%");

    mockRuntimeState.status = null;
  });

  test("TaskProgressBar compact mode clamps percent and supports indeterminate fill", () => {
    const clamped = renderToStaticMarkup(
      <TaskProgressBar compact label="" ariaLabel="batch-song" percent={140} />,
    );
    expect(clamped).toContain('role="progressbar"');
    expect(clamped).toContain('aria-label="batch-song"');
    expect(clamped).toContain("width:100%");
    expect(clamped).toContain("h-1 w-full");

    const below = renderToStaticMarkup(
      <TaskProgressBar compact label="x" percent={-10} />,
    );
    expect(below).toContain("width:0%");
    expect(below).toContain('aria-label="x"');

    const indeterminate = renderToStaticMarkup(
      <TaskProgressBar
        compact
        label=""
        percent={0}
        indeterminate
        ariaLabel="unknown-total"
      />,
    );
    expect(indeterminate).toContain("model-indeterminate-bar");
    expect(indeterminate).toContain('aria-label="unknown-total"');
  });

  test("TaskProgressBar exposes non-compact progress and a named cancel control", () => {
    const markup = renderToStaticMarkup(
      <TaskProgressBar
        label="Separating song"
        percent={42}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain('role="progressbar"');
    expect(markup).toContain('aria-label="Separating song"');
    expect(markup).toContain('aria-valuenow="42"');
    expect(markup).toContain('aria-label="common.cancel"');
  });
});
