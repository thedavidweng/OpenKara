import { useEffect, useMemo } from "react";
import App from "@/App";
import { TooltipProvider } from "@/components/Overlay/Tooltip";
import i18next from "@/lib/i18n";
import { MOCK_DATA } from "@/mock/tauri-mock-data";
import { createTauriMock } from "@/mock/tauri-mock-impl";
import { useLayoutStore } from "@/stores/layout-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import { useSettingsStore } from "@/stores/settings-store";
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

export function AppPreview({
  language,
  theme,
}: {
  language: "en" | "zh-CN";
  theme: "dark" | "light";
}) {
  // Inject the mock IPC once before first render.  The real app initialization
  // (useAppStartupRuntime) will call invoke() to load settings, library,
  // playback state, etc. — the same code path as the real Tauri app.
  ensureTauriMock();

  // The website preview starts with the library already registered so the
  // setup wizard is skipped.  The mock IPC's get_library_registry returns a
  // valid library, but we pre-set libraryReady=true to avoid a flash of the
  // LibrarySetup component during the async init.
  const app = useMemo(() => <App initialLibraryReady={true} previewMode />, []);

  // Apply the site's language prop to i18next.  The startup runtime
  // (useAppStartupRuntime → loadStartupSettings) asynchronously calls
  // changeLanguage(settings.language) where the mock returns "en", which
  // would override the prop.  Re-apply after settings hydration completes
  // so the prop-driven language wins the race.
  const settingsHydrated = useSettingsStore((s) => s.hydrated);
  useEffect(() => {
    void i18next.changeLanguage(language);
  }, [language, settingsHydrated]);

  // Mirror the landing page theme into the app settings store so the embedded
  // preview's theme-runtime resolves to the same theme as the surrounding
  // chrome. The mock's get_app_settings returns theme_preference "dark", which
  // would otherwise desync the preview from the landing toggle. Re-apply after
  // hydration so the prop-driven theme wins the race, matching the language
  // pattern above.
  useEffect(() => {
    if (!settingsHydrated) {
      return;
    }
    useSettingsStore.setState({ themePreference: theme });
  }, [theme, settingsHydrated]);

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
      // Default to the full library view (no active playlist). Visitors can
      // open a playlist from the sidebar and exit it via the back button.
      activePlaylistId: null,
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
