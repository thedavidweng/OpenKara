import type { RuntimeBootstrapFailurePhase } from "@/types/ipc";

/**
 * Copy keys for a failed runtime bootstrap, selected by the phase where the
 * failure happened. Download failures (and unclassified failures from older
 * backends) keep the check-your-network copy; install and load failures must
 * not blame the network (#284).
 */
export function runtimeFailureStatusKey(
  phase: RuntimeBootstrapFailurePhase | null | undefined,
):
  | "settings.runtime.downloadFailed"
  | "settings.runtime.installFailed"
  | "settings.runtime.loadFailed" {
  switch (phase) {
    case "install":
      return "settings.runtime.installFailed";
    case "probe":
    case "activate":
      return "settings.runtime.loadFailed";
    default:
      return "settings.runtime.downloadFailed";
  }
}

export interface RuntimeFailureBannerKeys {
  message:
    | "settings.runtime.banner.downloadFailed"
    | "settings.runtime.banner.installFailed"
    | "settings.runtime.banner.loadFailed";
  hint:
    | "settings.runtime.banner.downloadFailedHint"
    | "settings.runtime.banner.installFailedHint"
    | "settings.runtime.banner.loadFailedHint";
}

export function runtimeFailureBannerKeys(
  phase: RuntimeBootstrapFailurePhase | null | undefined,
): RuntimeFailureBannerKeys {
  switch (phase) {
    case "install":
      return {
        message: "settings.runtime.banner.installFailed",
        hint: "settings.runtime.banner.installFailedHint",
      };
    case "probe":
    case "activate":
      return {
        message: "settings.runtime.banner.loadFailed",
        hint: "settings.runtime.banner.loadFailedHint",
      };
    default:
      return {
        message: "settings.runtime.banner.downloadFailed",
        hint: "settings.runtime.banner.downloadFailedHint",
      };
  }
}
