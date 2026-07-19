import { useCallback, useState } from "react";
import { AppLayout } from "@/components/Layout/AppLayout";
import { LibrarySetup } from "@/components/Settings/LibrarySetup";
import {
  useAppReadyRuntime,
  useAppRuntime,
  useAppStartupRuntime,
} from "@/runtime/app-runtime";
import { useThemeRuntime } from "@/runtime/theme-runtime";
import { useSettingsStore } from "@/stores/settings-store";

interface AppProps {
  initialLibraryReady?: boolean | null;
}

function App({ initialLibraryReady = null }: AppProps) {
  const [libraryReady, setLibraryReady] = useState<boolean | null>(
    initialLibraryReady,
  );
  const [windowShown, setWindowShown] = useState(false);
  const settingsHydrated = useSettingsStore((s) => s.hydrated);

  // Hook declaration order: startup → main runtime → theme runtime → ready
  // runtime, so the layout effect applies CSS before the ready effect can
  // schedule showing the hidden window.
  useAppStartupRuntime(libraryReady, setLibraryReady);
  useAppRuntime(libraryReady === true);
  const { startupThemeReady } = useThemeRuntime();
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
    return null;
  }

  if (!libraryReady) {
    return <LibrarySetup onComplete={handleLibrarySetupComplete} />;
  }

  return <AppLayout />;
}

export default App;
