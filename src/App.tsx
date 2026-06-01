import { useCallback, useState } from "react";
import { AppLayout } from "@/components/Layout/AppLayout";
import { LibrarySetup } from "@/components/Settings/LibrarySetup";
import {
  useAppReadyRuntime,
  useAppStartupRuntime,
  useAppRuntime,
} from "@/runtime/app-runtime";

interface AppProps {
  initialLibraryReady?: boolean | null;
}

function App({ initialLibraryReady = null }: AppProps) {
  const [libraryReady, setLibraryReady] = useState<boolean | null>(
    initialLibraryReady,
  );
  const [windowShown, setWindowShown] = useState(false);
  useAppStartupRuntime(libraryReady, setLibraryReady);
  useAppRuntime(libraryReady === true);
  useAppReadyRuntime(libraryReady, windowShown, setWindowShown);

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
