import { createContext, use, useSyncExternalStore } from "react";
import type {
  SettingsController,
  SettingsLibraryCommands,
  SettingsMaintenanceCommands,
  SettingsPreferenceCommands,
  SettingsView,
} from "@/lib/settings-controller";

export interface SettingsSurface {
  view: SettingsView;
  library: SettingsLibraryCommands;
  preferences: SettingsPreferenceCommands;
  maintenance: SettingsMaintenanceCommands;
}

export const SettingsControllerContext =
  createContext<SettingsController | null>(null);

export function useSettings(): SettingsSurface {
  const controller = use(SettingsControllerContext);

  if (!controller) {
    throw new Error("Settings components must be used within the provider.");
  }

  const view = useSyncExternalStore(
    controller.subscribe,
    controller.getView,
    controller.getView,
  );

  return {
    view,
    library: controller.library,
    preferences: controller.preferences,
    maintenance: controller.maintenance,
  };
}
