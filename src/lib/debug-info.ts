import { getDebugInfo } from "@/lib/tauri";
import type { DebugInfo } from "@/types/ipc";

export function formatDebugInfo(info: DebugInfo): string {
  const model = info.model_installed
    ? `installed ${info.model_installed_version ?? "unknown"}`
    : "not installed";
  const runtimeArtifact = info.runtime_artifact_id ?? "none";

  return [
    "OpenKara debug info",
    `Version: ${info.app_version} (build ${info.build_sha})`,
    `OS: ${info.os} ${info.arch}`,
    `Catalog: generation ${info.catalog_generation} · ${info.catalog_release_id}`,
    `Model: ${info.model_variant} · ${info.model_state} · ${model} (pinned ${info.model_pinned_version})`,
    `Runtime: ${info.runtime_state} · ${info.runtime_version} · ${runtimeArtifact} · ${info.runtime_target_triple}`,
    `Execution provider: ${info.execution_provider}`,
    `Log file: ${info.log_file}`,
  ].join("\n");
}

interface CopyDebugInfoDependencies {
  fetchDebugInfo?: () => Promise<DebugInfo>;
  writeText?: (text: string) => Promise<void>;
}

export async function copyDebugInfo({
  fetchDebugInfo = getDebugInfo,
  writeText = (text: string) => navigator.clipboard.writeText(text),
}: CopyDebugInfoDependencies = {}): Promise<void> {
  const info = await fetchDebugInfo();
  await writeText(formatDebugInfo(info));
}
