import { useCallback, useState } from "react";
import { AppLayout } from "@/components/Layout/AppLayout";
import { AppShellSkeleton } from "@/components/Layout/AppShellSkeleton";
import { MissingLibraryRecovery } from "@/components/Settings/MissingLibraryRecovery";
import { LibrarySetup } from "@/components/Settings/LibrarySetup";
import {
  useAppReadyRuntime,
  useAppRuntime,
  useAppStartupRuntime,
} from "@/runtime/app-runtime";
import type { LibraryRegistrySnapshot } from "@/types/ipc";
import { useThemeRuntime } from "@/runtime/theme-runtime";
import { useSettingsStore } from "@/stores/settings-store";

interface AppProps {
  initialLibraryReady?: boolean | null;
  previewMode?: boolean;
}

function App({ initialLibraryReady = null, previewMode = false }: AppProps) {
  const [libraryReady, setLibraryReady] = useState<boolean | null>(
    initialLibraryReady,
  );
  const [unavailableRegistry, setUnavailableRegistry] =
    useState<LibraryRegistrySnapshot | null>(null);
  const [windowShown, setWindowShown] = useState(false);
  const settingsHydrated = useSettingsStore((s) => s.hydrated);

  useAppStartupRuntime(libraryReady, setLibraryReady, setUnavailableRegistry);
  useAppRuntime(libraryReady === true);
  const { startupThemeReady } = useThemeRuntime(previewMode);
  useAppReadyRuntime(
    libraryReady,
    settingsHydrated,
    startupThemeReady,
    windowShown,
    setWindowShown,
  );

  const handleLibrarySetupComplete = useCallback(() => {
    setLibraryReady(true);
  }, []);

  if (libraryReady === null) {
    return <AppShellSkeleton />;
  }

  if (!libraryReady && unavailableRegistry) {
    return (
      <MissingLibraryRecovery
        registry={unavailableRegistry}
        onResolved={handleLibrarySetupComplete}
      />
    );
  }

  if (!libraryReady) {
    return <LibrarySetup onComplete={handleLibrarySetupComplete} />;
  }

  return <AppLayout previewMode={previewMode} />;
}

export default App;
