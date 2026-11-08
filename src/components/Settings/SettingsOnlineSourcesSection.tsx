import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useCatalogStore } from "@/stores/catalog-store";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettings } from "./SettingsController.context";
import { SourceMarket } from "./SourceMarket";
import { SourceRegistries } from "./SourceRegistries";
import { StreamingSources } from "./StreamingSources";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Switch } from "@/components/ui/switch";
import { isInstalledSource } from "./source-market-utils";

type SourcesTab = "installed" | "discover";

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
            <Switch
              checked={settings.youtubeSourceEnabled}
              onCheckedChange={(checked) =>
                void preferences.set({ youtubeSourceEnabled: checked })
              }
              disabled={isInitializing}
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
            <h3 className="text-[13px] font-semibold">
              {t("settings.streaming.title")}
            </h3>
            <Tabs
              value={tab}
              onValueChange={(value) => setTab(value as SourcesTab)}
              className="gap-3"
            >
              <TabsList
                variant="line"
                activateOnFocus
                aria-label={t("settings.streaming.tabsLabel")}
              >
                <TabsTrigger value="installed" className="px-3">
                  {t("settings.streaming.tabs.installed", {
                    count: sources.filter(isInstalledSource).length,
                  })}
                </TabsTrigger>
                <TabsTrigger value="discover" className="px-3">
                  {t("settings.streaming.tabs.discover")}
                </TabsTrigger>
              </TabsList>
              <TabsContent value="installed" className="space-y-3">
                <StreamingSources />
              </TabsContent>
              <TabsContent value="discover" className="space-y-3">
                <p className="text-[12px] text-muted-foreground">
                  {t("settings.streaming.trustNotice")}
                </p>
                <SourceRegistries />
                <SourceMarket />
              </TabsContent>
            </Tabs>
          </div>
        ) : null}
      </div>
    </SettingsSectionCard>
  );
}
