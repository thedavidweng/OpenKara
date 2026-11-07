import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useCatalogStore } from "@/stores/catalog-store";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettings } from "./SettingsController.context";
import { SourceMarket } from "./SourceMarket";
import { SourceRegistries } from "./SourceRegistries";
import { StreamingSources } from "./StreamingSources";
import { SettingsTabList, SettingsTabPanel } from "./SettingsTabs";
import { isInstalledSource } from "./source-market-utils";

type SourcesTab = "installed" | "discover";
const TABS_ID = "streaming-sources";

export function SettingsOnlineSourcesSection() {
  const { t } = useTranslation();
  const { view, preferences } = useSettings();
  const { isInitializing } = view;
  const settings = view.preferences;
  const sourcesSupported = useCatalogStore((s) => s.sourcesSupported);
  const sources = useCatalogStore((s) => s.sources);
  const hasRegistryLink = useCatalogStore((s) => s.registryLink !== null);
  const [tab, setTab] = useState<SourcesTab>(
    hasRegistryLink ? "discover" : "installed",
  );
  useEffect(() => {
    if (hasRegistryLink) setTab("discover");
  }, [hasRegistryLink]);

  return (
    <SettingsSectionCard title={t("settings.onlineSources.label")}>
      <p className="text-[11px] text-[var(--color-text-dim)]">
        {t("settings.onlineSources.description")}
      </p>
      <div className="space-y-4">
        <div className="space-y-2">
          <label className="flex items-center gap-3">
            <input
              type="checkbox"
              checked={settings.youtubeSourceEnabled}
              onChange={(event) =>
                void preferences.set({
                  youtubeSourceEnabled: event.target.checked,
                })
              }
              disabled={isInitializing}
              className="h-4 w-4 rounded border-[var(--color-border-light)] bg-[var(--color-surface)] accent-[var(--color-accent)]"
              data-testid="online-source-youtube"
            />
            <span className="text-[13px] text-[var(--color-text)]">
              {t("settings.onlineSources.youtube")}
            </span>
          </label>
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.onlineSources.youtubeDescription")}
          </p>
        </div>
        {sourcesSupported === false ? (
          <p className="border-t border-[var(--color-border)] pt-4 text-[11px] text-[var(--color-text-dim)]">
            {t("settings.streaming.notIncluded")}
          </p>
        ) : null}
        {sourcesSupported ? (
          <div className="space-y-3 border-t border-[var(--color-border)] pt-4">
            <h3 className="text-sm font-semibold">
              {t("settings.streaming.title")}
            </h3>
            <SettingsTabList
              idPrefix={TABS_ID}
              label={t("settings.streaming.tabsLabel")}
              value={tab}
              onChange={setTab}
              className="flex gap-1 border-b border-[var(--color-border)]"
              tabs={[
                {
                  id: "installed",
                  label: t("settings.streaming.tabs.installed", {
                    count: sources.filter(isInstalledSource).length,
                  }),
                },
                {
                  id: "discover",
                  label: t("settings.streaming.tabs.discover"),
                },
              ]}
              tabClassName={(selected) =>
                `-mb-px min-h-9 border-b-2 px-3 text-xs focus-visible:outline-2 ${
                  selected
                    ? "border-[var(--color-accent)] font-semibold text-[var(--color-text)]"
                    : "border-transparent text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
                }`
              }
            />
            <SettingsTabPanel
              idPrefix={TABS_ID}
              value={tab}
              className="space-y-3"
            >
              {tab === "installed" ? (
                <StreamingSources />
              ) : (
                <>
                  <p className="text-xs text-[var(--color-text-dim)]">
                    {t("settings.streaming.trustNotice")}
                  </p>
                  <SourceRegistries />
                  <SourceMarket />
                </>
              )}
            </SettingsTabPanel>
          </div>
        ) : null}
      </div>
    </SettingsSectionCard>
  );
}
