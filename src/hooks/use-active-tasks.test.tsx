// @vitest-environment jsdom

import { act, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { MODEL_DOWNLOAD_FLASH_MS } from "@/lib/task-progress";
import { renderWithBackend } from "@/test-utils/backend";
import type {
  ModelBootstrapStatusSnapshot,
  SeparationStatusSnapshot,
  Song,
} from "@/types/ipc";
import { useActiveTasks } from "./use-active-tasks";

const { mockLibraryState, mockBootstrapState, mockRuntimeState } = vi.hoisted(
  () => ({
    mockLibraryState: {
      separationStatuses: {} as Record<string, SeparationStatusSnapshot>,
      uploadStatuses: {},
      batchSeparation: null as null,
      songs: [] as Song[],
    },
    mockBootstrapState: {
      status: null as ModelBootstrapStatusSnapshot | null,
    },
    mockRuntimeState: {
      status: null as null,
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

vi.mock("@/lib/errors", () => ({ notifyError: vi.fn() }));

const backend = createMockBackend();

function TaskKeys() {
  const tasks = useActiveTasks();
  return <span data-testid="keys">{tasks.map((task) => task.key).join()}</span>;
}

function modelStatus(
  state: ModelBootstrapStatusSnapshot["state"],
): ModelBootstrapStatusSnapshot {
  return {
    state,
    model_path: "/models/htdemucs.onnx",
    downloaded_bytes: null,
    total_bytes: null,
    error: null,
  };
}

describe("useActiveTasks", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    mockLibraryState.separationStatuses = {};
    mockLibraryState.songs = [];
    mockBootstrapState.status = null;
  });

  test("derives tasks from the progress stores", () => {
    mockLibraryState.separationStatuses = {
      "song-a": {
        song_id: "song-a",
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

    const { getByTestId } = renderWithBackend(<TaskKeys />, backend);

    expect(getByTestId("keys").textContent).toBe("sep-song-a");
  });

  test("flashes model-download completion and settles it away", () => {
    vi.useFakeTimers();
    mockBootstrapState.status = modelStatus("downloading");

    const { getByTestId, rerender } = renderWithBackend(<TaskKeys />, backend);
    expect(getByTestId("keys").textContent).toBe("model-download");

    mockBootstrapState.status = modelStatus("ready");
    act(() => {
      rerender(<TaskKeys />);
    });
    act(() => {
      vi.advanceTimersByTime(0);
    });
    expect(getByTestId("keys").textContent).toBe("model-download-complete");

    act(() => {
      vi.advanceTimersByTime(MODEL_DOWNLOAD_FLASH_MS);
    });
    expect(getByTestId("keys").textContent).toBe("");
  });
});
