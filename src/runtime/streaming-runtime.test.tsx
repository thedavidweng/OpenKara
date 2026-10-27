// @vitest-environment jsdom
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vite-plus/test";
import { StreamingImportBar } from "@/components/Catalog/StreamingImportBar";
import { LibraryDecisionDialog } from "@/components/Library/LibraryDecisionDialog";
import { useCatalogStore } from "@/stores/catalog-store";
import { useSettingsStore } from "@/stores/settings-store";
import type {
  SourceRegistryLink,
  StreamingImportCompleteEvent,
} from "@/types/ipc";
import { startStreamingRuntime } from "./streaming-runtime";

const { handlers, pendingLinks } = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  pendingLinks: [] as SourceRegistryLink[],
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (
    name: string,
    handler: (event: { payload: unknown }) => void,
  ) => {
    handlers.set(name, handler);
    return () => handlers.delete(name);
  },
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string) =>
    command === "take_source_registry_links" ? pendingLinks.splice(0) : [],
}));
vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({ t: (key: string) => key }),
}));

afterEach(() => {
  cleanup();
  handlers.clear();
  pendingLinks.splice(0);
  useSettingsStore.setState({ isOpen: false });
  useCatalogStore.setState({
    registryLink: null,
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

describe("source registry links", () => {
  const link: SourceRegistryLink = {
    kind: "add_registry",
    url: "https://sources.example.com/registry.json",
  };

  test("a link that started the app opens Settings once the runtime starts", async () => {
    pendingLinks.push(link);
    const stop = startStreamingRuntime();
    try {
      await waitFor(() =>
        expect(useSettingsStore.getState().isOpen).toBe(true),
      );
      expect(useCatalogStore.getState().registryLink).toEqual(link);
    } finally {
      stop();
    }
  });

  test("a link opened while running opens Settings with the newest request", async () => {
    const stop = startStreamingRuntime();
    try {
      await waitFor(() =>
        expect(handlers.has("source-registry-link")).toBe(true),
      );
      expect(useSettingsStore.getState().isOpen).toBe(false);
      const invalid: SourceRegistryLink = {
        kind: "invalid",
        link: "openkara://add-registry",
      };
      pendingLinks.push(invalid, link);
      act(() => handlers.get("source-registry-link")?.({ payload: null }));
      await waitFor(() =>
        expect(useSettingsStore.getState().isOpen).toBe(true),
      );
      expect(useCatalogStore.getState().registryLink).toEqual(link);
    } finally {
      stop();
    }
  });
});
