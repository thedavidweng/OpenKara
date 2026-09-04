import { tauriBackend } from "@/lib/backend";
import type { DebugInfo } from "@/types/ipc";
import i18next from "@/lib/i18n";
import type { TFunction } from "i18next";

export interface DebugInfoErrorContext {
  title: string;
  message: string;
}

export function formatDebugInfo(
  info: DebugInfo,
  t: TFunction = i18next.t.bind(i18next),
  error?: DebugInfoErrorContext,
): string {
  const modelVersion = info.model_installed_version ?? "—";
  const runtimeArtifact = info.runtime_artifact_id ?? "—";

  const runtimePath = info.runtime_path.trim() === "" ? "—" : info.runtime_path;
  const language = info.language ?? "system";

  const lines = [
    `${t("app.name")} · ${t("settings.about.label")}`,
    `${t("settings.about.version")}: ${info.app_version} (${t("settings.about.build")} ${info.build_sha})`,
    `${t("settings.about.system")}: ${info.os} · ${info.arch}`,
    `${t("settings.about.catalog")}: ${info.catalog_generation} · ${info.catalog_release_id}`,
    `${t("settings.about.model")}: ${info.model_variant} · ${info.model_state} · ${modelVersion} · ${info.model_pinned_version}`,
    `${t("settings.about.runtime")}: ${info.runtime_state} · ${info.runtime_version} · ${runtimeArtifact} · ${info.runtime_target_triple}`,
    `Runtime path: ${runtimePath}`,
    `${t("settings.about.executionProvider")}: ${info.execution_provider}`,
    `DirectML available: ${info.directml_available}`,
    `UI language: ${language}`,
    `${t("settings.about.logFile")}: ${info.log_file}`,
  ];
  if (error) {
    lines.push(`Error: ${error.title}`);
    if (error.message.trim() !== "") {
      lines.push(error.message);
    }
  }
  return lines.join("\n");
}

interface CopyDebugInfoDependencies {
  fetchDebugInfo?: () => Promise<DebugInfo>;
  writeText?: (text: string) => Promise<void>;
  translate?: TFunction;
  error?: DebugInfoErrorContext;
}

export async function copyDebugInfo({
  fetchDebugInfo = tauriBackend.settings.getDebugInfo,
  writeText = (text: string) => navigator.clipboard.writeText(text),
  translate = i18next.t.bind(i18next),
  error,
}: CopyDebugInfoDependencies = {}): Promise<void> {
  const info = await fetchDebugInfo();
  await writeText(formatDebugInfo(info, translate, error));
}
