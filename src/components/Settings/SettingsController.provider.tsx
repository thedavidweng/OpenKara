import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, type ReactNode } from "react";
import { useBackend } from "@/lib/backend";
import { createLibrarySession } from "@/lib/library-session";
import {
  createSettingsController,
  type SettingsController,
} from "@/lib/settings-controller";
import { SettingsControllerContext } from "./SettingsController.context";

async function selectSingleDirectory(
  dialogTitle: string,
): Promise<string | null> {
  const selected = await open({ directory: true, title: dialogTitle });

  if (!selected) {
    return null;
  }

  return typeof selected === "string" ? selected : (selected[0] ?? null);
}

export function SettingsControllerProvider({
  children,
}: {
  children: ReactNode;
}) {
  const backend = useBackend();
  const controller = useMemo(
    () =>
      createSettingsController({
        backend,
        createLibrarySession: (views) =>
          createLibrarySession({ backend, views }),
        selectDirectory: selectSingleDirectory,
      }),
    [backend],
  );
  const initializedRef = useRef<SettingsController | null>(null);

  useEffect(() => {
    if (initializedRef.current === controller) {
      return;
    }

    initializedRef.current = controller;
    void controller.initialize();
  }, [controller]);

  return (
    <SettingsControllerContext value={controller}>
      {children}
    </SettingsControllerContext>
  );
}
