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

const LIBRARY_REFRESH_DELAY_MS = 300;
let libraryRefreshTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * Imported songs are written by the backend, not through the library store, so
 * the store must be told to reload. Bursts of events collapse into one reload.
 */
function scheduleLibraryRefresh() {
  if (libraryRefreshTimer) clearTimeout(libraryRefreshTimer);
  libraryRefreshTimer = setTimeout(() => {
    libraryRefreshTimer = null;
    void useLibraryStore.getState().loadLibrary();
    void usePlaylistStore.getState().loadPlaylists();
  }, LIBRARY_REFRESH_DELAY_MS);
}

// Module-level guard: sign-in events must reach the store exactly once, no
// matter how many times the hosting component (re)mounts.
let activeRuntime: RuntimeEventRuntime | null = null;

export function startNeteaseRuntime(): () => void {
  activeRuntime?.stop();
  const runtime = createRuntimeEventRuntime([
    eventSubscription("netease-sign-in-complete", (session) =>
      useCatalogStore.getState().acceptSignIn(session),
    ),
    eventSubscription("netease-sign-in-error", (error) =>
      useCatalogStore.getState().setSignInError(error),
    ),
    eventSubscription("streaming-import-progress", (event) => {
      useCatalogStore.getState().applyImportEvent(event);
      if (event.state.kind === "imported") scheduleLibraryRefresh();
    }),
  ]);
  activeRuntime = runtime;
  void runtime.start().catch(notifyError);
  return () => {
    runtime.stop();
    if (activeRuntime === runtime) activeRuntime = null;
  };
}

export function useNeteaseRuntime(enabled: boolean) {
  const sourceEnabled = useSettingsStore((s) => s.neteaseSourceEnabled);
  const active = enabled && sourceEnabled;

  useEffect(() => {
    if (!active) return;
    void useCatalogStore.getState().loadSession();
    void useCatalogStore.getState().recoverImportJob();
    return startNeteaseRuntime();
  }, [active]);
}
