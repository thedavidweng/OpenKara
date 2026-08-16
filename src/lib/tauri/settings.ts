import type { SettingsBackend } from "@/lib/backend/types";
import type {
  AppSettings,
  DebugInfo,
  ModelBootstrapStatusSnapshot,
  ModelStatusSnapshot,
  ModelUpdateReport,
  RuntimeBootstrapStatusSnapshot,
  RuntimeUpdateReport,
  WindowShellStateSnapshot,
} from "@/types/ipc";
import type { InvokeCommand } from "./invoke";

export function createSettingsCommands(invoke: InvokeCommand): SettingsBackend {
  return {
    getSettings: () => invoke<AppSettings>("get_settings"),

    getDebugInfo: () => invoke<DebugInfo>("get_debug_info"),

    getWindowShellState: () =>
      invoke<WindowShellStateSnapshot>("get_window_shell_state"),

    setNativeSidebarVisibility: (visible) =>
      invoke<void>("set_native_sidebar_visibility", { visible }),

    windowReady: () => invoke<void>("window_ready"),

    setNativeAppMenuLabels: (labels) =>
      invoke<void>("set_native_app_menu_labels", { labels }),

    restartApp: () => invoke<void>("restart_app"),

    setLanguage: (language) =>
      invoke<AppSettings>("set_language", { language }),

    setStemMode: (mode) => invoke<AppSettings>("set_stem_mode", { mode }),

    setModelVariant: (variant) =>
      invoke<AppSettings>("set_model_variant", { variant }),

    setHideBatchSeparate: (value) =>
      invoke<AppSettings>("set_hide_batch_separate", { value }),

    setCoverArtBackdrop: (value) =>
      invoke<AppSettings>("set_cover_art_backdrop", { value }),

    setLyricsBlurInactive: (value) =>
      invoke<AppSettings>("set_lyrics_blur_inactive", { value }),

    setHideUpgradeAll: (value) =>
      invoke<AppSettings>("set_hide_upgrade_all", { value }),

    setExecutionProvider: (provider) =>
      invoke<AppSettings>("set_execution_provider", { provider }),

    setLyricsFontStep: (step) =>
      invoke<AppSettings>("set_lyrics_font_step", { step }),

    setEqEnabled: (enabled) =>
      invoke<AppSettings>("set_eq_enabled", { enabled }),

    setEqGains: (gainsDb) => invoke<AppSettings>("set_eq_gains", { gainsDb }),

    setCrossfadeEnabled: (enabled) =>
      invoke<AppSettings>("set_crossfade_enabled", { enabled }),

    setCrossfadeDurationMs: (durationMs) =>
      invoke<AppSettings>("set_crossfade_duration_ms", { durationMs }),

    setLibrarySortMode: (mode) =>
      invoke<AppSettings>("set_library_sort_mode", { mode }),

    setThemePreference: (preference) =>
      invoke<AppSettings>("set_theme_preference", { preference }),

    setUpdatePolicy: (policy) =>
      invoke<AppSettings>("set_update_policy", { policy }),

    getModelBootstrapStatus: () =>
      invoke<ModelBootstrapStatusSnapshot>("get_model_bootstrap_status"),

    getModelStatus: (variant) =>
      invoke<ModelStatusSnapshot>("get_model_status", { variant }),

    downloadModel: (variant) =>
      invoke<ModelBootstrapStatusSnapshot>("download_model", { variant }),

    deleteModel: (variant) => invoke<void>("delete_model", { variant }),

    checkModelUpdates: () => invoke<ModelUpdateReport>("check_model_updates"),

    getRuntimeBootstrapStatus: () =>
      invoke<RuntimeBootstrapStatusSnapshot>("get_runtime_bootstrap_status"),

    downloadRuntime: () =>
      invoke<RuntimeBootstrapStatusSnapshot>("download_runtime"),

    deleteRuntime: () => invoke<void>("delete_runtime"),

    checkRuntimeUpdates: () =>
      invoke<RuntimeUpdateReport>("check_runtime_updates"),
  };
}
