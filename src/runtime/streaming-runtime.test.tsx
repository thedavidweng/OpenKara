// @vitest-environment jsdom
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { StreamingImportBar } from "@/components/Catalog/StreamingImportBar";
import { LibraryDecisionDialog } from "@/components/Library/LibraryDecisionDialog";
import { useCatalogStore } from "@/stores/catalog-store";
import type { StreamingImportCompleteEvent } from "@/types/ipc";
import { startStreamingRuntime } from "./streaming-runtime";

const { handlers } = vi.hoisted(() => ({
  handlers: new Map<
    string,
    (event: { payload: StreamingImportCompleteEvent }) => void
  >(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (
    name: string,
    handler: (event: { payload: StreamingImportCompleteEvent }) => void,
  ) => {
    handlers.set(name, handler);
    return () => handlers.delete(name);
  },
}));
vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({ t: (key: string) => key }),
}));

afterEach(() => {
  cleanup();
  handlers.clear();
  useCatalogStore.setState({
    importRun: null,
    pendingConflict: null,
    activeImportIds: [],
    trackStates: {},
    importFailures: [],
  });
});

describe("global streaming import completion", () => {
  test.each(["cancelled", "import_failed"] as const)(
    "settles a parked decision on %s outside its source view and ignores foreign jobs",
    async (reason) => {
      const metadata = {
        title: "Shared",
        artist: "Artist",
        album: null,
        format: "MP3",
        bit_rate_bps: null,
        duration_ms: null,
        file_size_bytes: 100,
      };
      useCatalogStore.setState({
        activeView: "library",
        activeSourceId: "beta",
        importRun: {
          sourceId: "alpha",
          jobId: "parked-job",
          phase: "running",
          total: 1,
          completed: 0,
          imported: 0,
          skipped: 0,
          label: null,
          cancelling: true,
        },
        activeImportIds: ["shared"],
        trackStates: {},
        importFailures: [],
        pendingConflict: {
          source_id: "alpha",
          remote_track_id: "shared",
          library: metadata,
          incoming: metadata,
        },
      });
      const stop = startStreamingRuntime();
      try {
        render(
          <>
            <StreamingImportBar />
            <LibraryDecisionDialog />
          </>,
        );
        expect(screen.getByRole("dialog")).toBeTruthy();
        await waitFor(() =>
          expect(handlers.has("streaming-import-complete")).toBe(true),
        );
        const event: StreamingImportCompleteEvent = {
          source_id: "alpha",
          job_id: "parked-job",
          progress: {
            source_id: "alpha",
            status: "completed",
            imported_song_ids: [],
            failed: [
              {
                remote_track_id: "shared",
                title: "Shared",
                artist: "Artist",
                reason,
                refusal: null,
              },
            ],
            playlist_id: null,
            conflict: null,
          },
        };
        act(() => {
          handlers.get("streaming-import-complete")?.({
            payload: { ...event, source_id: "beta" },
          });
          handlers.get("streaming-import-complete")?.({
            payload: { ...event, job_id: "other-job" },
          });
        });
        expect(screen.getByRole("dialog")).toBeTruthy();
        act(() =>
          handlers.get("streaming-import-complete")?.({ payload: event }),
        );
        expect(screen.queryByRole("dialog")).toBeNull();
        expect(screen.getByText("catalog.streaming.importDone")).toBeTruthy();
        expect(
          screen.queryByRole("button", {
            name: "catalog.streaming.cancelImport",
          }),
        ).toBeNull();
        expect(useCatalogStore.getState().importRun).toMatchObject({
          sourceId: "alpha",
          jobId: "parked-job",
          phase: "done",
          cancelling: false,
          completed: 1,
          skipped: 1,
        });
      } finally {
        stop();
      }
    },
  );
});
