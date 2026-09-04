import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettings } from "./SettingsController.context";

export function SettingsOnlineSourcesSection() {
  const { t } = useTranslation();
  const { view, preferences } = useSettings();
  const { isInitializing } = view;
  const settings = view.preferences;

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
        <div className="space-y-2 border-t border-[var(--color-border)] pt-4">
          <label className="flex items-center gap-3">
            <input
              type="checkbox"
              checked={settings.neteaseSourceEnabled}
              onChange={(event) =>
                void preferences.set({
                  neteaseSourceEnabled: event.target.checked,
                })
              }
              disabled={isInitializing}
              className="h-4 w-4 rounded border-[var(--color-border-light)] bg-[var(--color-surface)] accent-[var(--color-accent)]"
              data-testid="online-source-netease"
            />
            <span className="text-[13px] text-[var(--color-text)]">
              {t("settings.onlineSources.netease")}
            </span>
          </label>
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.onlineSources.neteaseDescription")}
          </p>
        </div>
      </div>
    </SettingsSectionCard>
  );
}
