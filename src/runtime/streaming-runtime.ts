import { useEffect } from "react";
import { notifyError } from "@/lib/errors";
import {
  createRuntimeEventRuntime,
  type RuntimeEventRuntime,
} from "@/runtime/event-runtime";
import { eventSubscription } from "@/runtime/event-source";
import { useCatalogStore } from "@/stores/catalog-store";
import { useLibraryStore } from "@/stores/library-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import { useSettingsStore } from "@/stores/settings-store";

let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let activeRuntime: RuntimeEventRuntime | null = null;
async function receiveRegistryLinks() {
  if (await useCatalogStore.getState().receiveRegistryLinks())
    useSettingsStore.getState().open();
}
export function startStreamingRuntime() {
  activeRuntime?.stop();
  const runtime = createRuntimeEventRuntime([
    eventSubscription(
      "source-registry-link",
      () => void receiveRegistryLinks(),
    ),
    eventSubscription("streaming-sign-in-complete", (session) =>
      useCatalogStore.getState().acceptSignIn(session),
    ),
    eventSubscription(
      "streaming-sign-in-error",
      ({ source_id, challenge, error }) =>
        useCatalogStore.getState().setSignInError(source_id, challenge, error),
    ),
    eventSubscription("streaming-source-status", (source) => {
      useCatalogStore.getState().applySource(source);
      if (
        source.enabled &&
        source.loaded &&
        source.state === "ready" &&
        useCatalogStore.getState().sourceStates[source.source_id]
          ?.sessionStatus !== "ready"
      )
        void useCatalogStore.getState().loadSession(source.source_id);
    }),
    eventSubscription("streaming-import-complete", (event) =>
      useCatalogStore.getState().applyImportComplete(event),
    ),
    eventSubscription("streaming-import-progress", (event) => {
      const state = useCatalogStore.getState();
      const run = state.importRun;
      if (
        !run ||
        run.sourceId !== event.source_id ||
        run.jobId !== event.job_id
      )
        return;
      state.applyImportEvent(event);
      if (event.state.kind === "imported") {
        clearTimeout(refreshTimer ?? undefined);
        refreshTimer = setTimeout(() => {
          refreshTimer = null;
          void useLibraryStore.getState().loadLibrary();
          void usePlaylistStore.getState().loadPlaylists();
        }, 300);
      }
    }),
  ]);
  activeRuntime = runtime;
  // Subscribe before collecting so a link that arrives in between is not lost.
  void runtime.start().then(receiveRegistryLinks).catch(notifyError);
  return () => {
    runtime.stop();
    if (activeRuntime === runtime) activeRuntime = null;
  };
}
export function useStreamingRuntime(enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;
    const stop = startStreamingRuntime();
    void useCatalogStore
      .getState()
      .loadSources()
      .then(() => {
        for (const source of useCatalogStore.getState().sources)
          if (source.enabled && source.loaded)
            void useCatalogStore.getState().loadSession(source.source_id);
      });
    void useCatalogStore.getState().recoverImportJob();
    return stop;
  }, [enabled]);
}
