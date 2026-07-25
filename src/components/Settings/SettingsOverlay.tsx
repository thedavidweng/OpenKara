import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { SettingsCrossfadeSection } from "./SettingsCrossfadeSection";
import { SettingsDangerZoneSection } from "./SettingsDangerZoneSection";
import { SettingsDialogHost } from "./SettingsDialogHost";
import { SettingsEqSection } from "./SettingsEqSection";
import { SettingsExecutionProviderSection } from "./SettingsExecutionProviderSection";
import { SettingsGeneralSection } from "./SettingsGeneralSection";
import { SettingsLibrarySection } from "./SettingsLibrarySection";
import { SettingsModelVariantSection } from "./SettingsModelVariantSection";
import { SettingsRuntimeSection } from "./SettingsRuntimeSection";
import { SettingsOverlayProvider } from "./SettingsOverlay.controller";
import { SettingsRemoteCacheSection } from "./SettingsRemoteCacheSection";
import { SettingsRemoteDiagnosticsSection } from "./SettingsRemoteDiagnosticsSection";
import { SettingsStemModeSection } from "./SettingsStemModeSection";
import { useSettingsStore } from "@/stores/settings-store";

export function SettingsOverlay() {
  const { t } = useTranslation();
  const closeSettings = useSettingsStore((s) => s.close);

  return (
    <div
      data-testid="settings-overlay"
      className="pointer-events-auto absolute inset-0 z-30 flex flex-1 flex-col overflow-y-auto overscroll-y-contain bg-[var(--color-surface-muted)] p-10"
    >
      <div className="mx-auto w-full max-w-xl space-y-6">
        <div className="flex items-start justify-between gap-4">
          <h2 className="text-lg font-semibold text-[var(--color-text)]">
            {t("settings.title")}
          </h2>
          <button
            type="button"
            onClick={closeSettings}
            aria-label={t("common.close")}
            className="motion-icon-button rounded-xl p-2 text-[var(--color-text-dim)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
          >
            <X size={16} />
          </button>
        </div>
        <SettingsOverlayProvider>
          <SettingsLibrarySection />
          <SettingsStemModeSection />
          <SettingsModelVariantSection />
          <SettingsRuntimeSection />
          <SettingsExecutionProviderSection />
          <SettingsGeneralSection />
          <SettingsEqSection />
          <SettingsCrossfadeSection />
          <SettingsRemoteCacheSection />
          <SettingsRemoteDiagnosticsSection />
          <SettingsDangerZoneSection />
          <SettingsDialogHost />
        </SettingsOverlayProvider>
      </div>
    </div>
  );
}
