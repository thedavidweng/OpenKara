import { X } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { SettingsAboutSection } from "./SettingsAboutSection";
import { SettingsCrossfadeSection } from "./SettingsCrossfadeSection";
import { SettingsDangerZoneSection } from "./SettingsDangerZoneSection";
import { SettingsDialogHost } from "./SettingsDialogHost";
import { SettingsEqSection } from "./SettingsEqSection";
import { SettingsExecutionProviderSection } from "./SettingsExecutionProviderSection";
import { SettingsGeneralSection } from "./SettingsGeneralSection";
import { SettingsLibrarySection } from "./SettingsLibrarySection";
import { SettingsOnlineSourcesSection } from "./SettingsOnlineSourcesSection";
import { SettingsModelVariantSection } from "./SettingsModelVariantSection";
import { SettingsRuntimeSection } from "./SettingsRuntimeSection";
import { SettingsControllerProvider } from "./SettingsController.provider";
import { SettingsRemoteCacheSection } from "./SettingsRemoteCacheSection";
import { SettingsRemoteDiagnosticsSection } from "./SettingsRemoteDiagnosticsSection";
import { SettingsStemModeSection } from "./SettingsStemModeSection";
import { SettingsTabList, SettingsTabPanel } from "./SettingsTabs";
import { useCatalogStore } from "@/stores/catalog-store";
import { useSettingsStore } from "@/stores/settings-store";
import { useModalDialog } from "@/hooks/use-modal-dialog";

type SettingsPage =
  | "library"
  | "onlineSources"
  | "separation"
  | "playback"
  | "general";

const PAGES: { id: SettingsPage; render: () => ReactNode }[] = [
  {
    id: "library",
    render: () => (
      <>
        <SettingsLibrarySection />
        <SettingsRemoteCacheSection />
        <SettingsRemoteDiagnosticsSection />
      </>
    ),
  },
  { id: "onlineSources", render: () => <SettingsOnlineSourcesSection /> },
  {
    id: "separation",
    render: () => (
      <>
        <SettingsStemModeSection />
        <SettingsModelVariantSection />
        <SettingsRuntimeSection />
        <SettingsExecutionProviderSection />
      </>
    ),
  },
  {
    id: "playback",
    render: () => (
      <>
        <SettingsEqSection />
        <SettingsCrossfadeSection />
      </>
    ),
  },
  {
    id: "general",
    render: () => (
      <>
        <SettingsGeneralSection />
        <SettingsAboutSection />
        <SettingsDangerZoneSection />
      </>
    ),
  },
];
const NAV_ID = "settings-pages";

export function SettingsOverlay() {
  const { t } = useTranslation();
  const closeSettings = useSettingsStore((s) => s.close);
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const titleId = "settings-overlay-title";
  const hasRegistryLink = useCatalogStore((s) => s.registryLink !== null);
  const [page, setPage] = useState<SettingsPage>(
    hasRegistryLink ? "onlineSources" : "library",
  );
  useEffect(() => {
    if (hasRegistryLink) setPage("onlineSources");
  }, [hasRegistryLink]);

  useModalDialog({
    dialogRef,
    initialFocusRef: closeButtonRef,
    onDismiss: closeSettings,
  });

  return (
    <div
      ref={dialogRef}
      id="settings-overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      tabIndex={-1}
      data-testid="settings-overlay"
      className="pointer-events-auto absolute inset-0 z-30 flex flex-1 flex-col overflow-y-auto overscroll-y-contain bg-[var(--color-surface-muted)] p-10"
    >
      <div className="mx-auto w-full max-w-5xl space-y-6">
        <div className="flex items-start justify-between gap-4">
          <h2
            id={titleId}
            className="text-lg font-semibold text-[var(--color-text)]"
          >
            {t("settings.title")}
          </h2>
          <button
            ref={closeButtonRef}
            id="settings-close"
            type="button"
            onClick={closeSettings}
            aria-label={t("common.close")}
            className="motion-icon-button rounded-xl p-2 text-[var(--color-text-dim)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
          >
            <X size={16} />
          </button>
        </div>
        <SettingsControllerProvider>
          <div className="flex flex-col gap-6 sm:flex-row sm:items-start">
            <SettingsTabList
              idPrefix={NAV_ID}
              label={t("settings.pagesLabel")}
              orientation="vertical"
              value={page}
              onChange={setPage}
              className="flex shrink-0 gap-1 overflow-x-auto sm:sticky sm:top-0 sm:w-44 sm:flex-col sm:overflow-visible"
              tabs={PAGES.map(({ id }) => ({
                id,
                label: t(`settings.pages.${id}`),
              }))}
              tabClassName={(selected) =>
                `min-h-9 shrink-0 rounded-lg px-3 text-left text-[13px] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 ${
                  selected
                    ? "bg-[var(--color-ghost-hover)] font-semibold text-[var(--color-text)]"
                    : "text-[var(--color-text-dim)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)]"
                }`
              }
            />
            <SettingsTabPanel
              idPrefix={NAV_ID}
              value={page}
              className="min-w-0 flex-1 space-y-6"
            >
              {PAGES.find(({ id }) => id === page)?.render()}
            </SettingsTabPanel>
          </div>
          <SettingsDialogHost />
        </SettingsControllerProvider>
      </div>
    </div>
  );
}
