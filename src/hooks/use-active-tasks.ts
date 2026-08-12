import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useBackend } from "@/lib/backend";
import {
  createModelDownloadFlash,
  deriveActiveTasks,
  type ActiveTask,
} from "@/lib/task-progress";
import { useBootstrapStore } from "@/stores/bootstrap-store";
import { useLibraryStore } from "@/stores/library-store";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";

function useModelDownloadCompleteFlash(): boolean {
  const modelState = useBootstrapStore((s) => s.status?.state);
  const [visible, setVisible] = useState(false);
  const [flash] = useState(() => createModelDownloadFlash(setVisible));

  useEffect(() => {
    flash.observe(modelState);
  }, [flash, modelState]);

  useEffect(() => () => flash.dispose(), [flash]);

  return visible;
}

export function useActiveTasks(): ActiveTask[] {
  const { t } = useTranslation();
  const backend = useBackend();
  const modelDownloadCompleteFlash = useModelDownloadCompleteFlash();
  const modelBootstrap = useBootstrapStore((s) => s.status);
  const runtimeBootstrap = useRuntimeBootstrapStore((s) => s.status);
  const separationStatuses = useLibraryStore((s) => s.separationStatuses);
  const uploadStatuses = useLibraryStore((s) => s.uploadStatuses);
  const batchSeparation = useLibraryStore((s) => s.batchSeparation);
  const songs = useLibraryStore((s) => s.songs);

  return deriveActiveTasks({
    t: (key, options) => t(key, options),
    backend,
    modelBootstrap,
    runtimeBootstrap,
    modelDownloadCompleteFlash,
    separationStatuses,
    uploadStatuses,
    batchSeparation,
    songs,
  });
}
