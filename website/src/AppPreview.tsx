import { useEffect, useMemo } from "react";
import App from "@/App";
import { TooltipProvider } from "@/components/Overlay/Tooltip";
import { MOCK_DATA } from "@/mock/tauri-mock-data";
import { createTauriMock } from "@/mock/tauri-mock-impl";
import { useLayoutStore } from "@/stores/layout-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import type { PlaylistSong } from "@/lib/tauri/playlist";

// Set up the mock Tauri IPC before the app renders so that `useAppStartupRuntime`
// can call `invoke()` and populate stores through the real initialization path.
// This is the same mock mechanism used by the E2E fixture — both source from
// `src/mock/tauri-mock-impl.ts` so the website preview and E2E tests cannot
// drift apart.
function ensureTauriMock() {
  if (!window.__TAURI_INTERNALS__) {
    const { internals, eventPluginInternals } = createTauriMock(MOCK_DATA);
    window.__TAURI_INTERNALS__ = internals;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = eventPluginInternals;
  }
}

export function AppPreview({ language }: { language: "en" | "zh-CN" }) {
  // Inject the mock IPC once before first render.  The real app initialization
  // (useAppStartupRuntime) will call invoke() to load settings, library,
  // playback state, etc. — the same code path as the real Tauri app.
  ensureTauriMock();

  // The website preview starts with the library already registered so the
  // setup wizard is skipped.  The mock IPC's get_library_registry returns a
  // valid library, but we pre-set libraryReady=true to avoid a flash of the
  // LibrarySetup component during the async init.
  const app = useMemo(() => <App initialLibraryReady={true} previewMode />, []);

  // previewMode skips the Sidebar's loadPlaylists useEffect, so manually
  // populate the playlist store from the mock data.  This is the only store
  // manipulation needed — all other stores are populated by the real
  // initialization code path via mock IPC.
  useEffect(() => {
    const playlistSongSets = new Map(
      Object.entries(MOCK_DATA.playlistSongs).map(([playlistId, songIds]) => [
        playlistId,
        new Set(songIds),
      ]),
    );
    usePlaylistStore.setState({
      playlists: [...MOCK_DATA.playlists],
      activePlaylistId: MOCK_DATA.playlists[0]?.id ?? null,
      isLoading: false,
      playlistSongSets,
      loadPlaylists: async () => {},
      loadPlaylistSongSets: async () => {},
      createPlaylist: async () => MOCK_DATA.playlists[0],
      renamePlaylist: async () => {},
      deletePlaylist: async () => {},
      addSongsToPlaylist: async () => {},
      removeSongsFromPlaylist: async () => {},
      getPlaylistSongs: async (playlistId): Promise<PlaylistSong[]> =>
        (MOCK_DATA.playlistSongs[playlistId] ?? []).map((song_hash, index) => ({
          song_hash,
          added_at: index + 1,
          sort_order: index,
          singer: null,
        })),
    });
    useLayoutStore.setState({ sidebarVisible: true, sidebarWidth: 260 });
  }, [language]);

  return (
    <div className="product-preview" aria-label="Interactive OpenKara preview">
      <TooltipProvider>{app}</TooltipProvider>
    </div>
  );
}
