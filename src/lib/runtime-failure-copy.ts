import type { RuntimeBootstrapFailurePhase } from "@/types/ipc";

/**
 * Copy category for a failed runtime bootstrap, derived from the phase where
 * the failure happened. Download failures (and unclassified failures from
 * older backends) keep the check-your-network copy; install and load failures
 * must not blame the network (#284).
 */
type RuntimeFailureCategory = "download" | "install" | "load";

function runtimeFailureCategory(
  phase: RuntimeBootstrapFailurePhase | null | undefined,
): RuntimeFailureCategory {
  switch (phase) {
    case "install":
      return "install";
    case "probe":
    case "activate":
      return "load";
    default:
      return "download";
  }
}

const STATUS_KEYS = {
  download: "settings.runtime.downloadFailed",
  install: "settings.runtime.installFailed",
  load: "settings.runtime.loadFailed",
} as const;

const BANNER_KEYS = {
  download: {
    message: "settings.runtime.banner.downloadFailed",
    hint: "settings.runtime.banner.downloadFailedHint",
  },
  install: {
    message: "settings.runtime.banner.installFailed",
    hint: "settings.runtime.banner.installFailedHint",
  },
  load: {
    message: "settings.runtime.banner.loadFailed",
    hint: "settings.runtime.banner.loadFailedHint",
  },
} as const;

export function runtimeFailureStatusKey(
  phase: RuntimeBootstrapFailurePhase | null | undefined,
): (typeof STATUS_KEYS)[RuntimeFailureCategory] {
  return STATUS_KEYS[runtimeFailureCategory(phase)];
}

export function runtimeFailureBannerKeys(
  phase: RuntimeBootstrapFailurePhase | null | undefined,
): (typeof BANNER_KEYS)[RuntimeFailureCategory] {
  return BANNER_KEYS[runtimeFailureCategory(phase)];
}
