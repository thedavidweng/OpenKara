import { useTranslation } from "react-i18next";
import { SUPPORTED_LANGUAGES } from "@/lib/i18n";
import type { ThemePreference } from "@/types/ipc";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettings } from "./SettingsController.context";

const THEME_OPTIONS: ThemePreference[] = ["system", "light", "dark"];

export function SettingsGeneralSection() {
  const { t } = useTranslation();
  const { view, preferences } = useSettings();
  const { isInitializing } = view;
  const settings = view.preferences;

  return (
    <SettingsSectionCard title={t("settings.language.label")}>
      <div className="space-y-4">
        <fieldset className="space-y-2">
          <legend className="text-[12px] font-medium text-[var(--color-text-dim)]">
            {t("settings.appearance.label")}
          </legend>
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.appearance.description")}
          </p>
          <div className="flex gap-4">
            {THEME_OPTIONS.map((option) => (
              <label
                key={option}
                className="flex items-center gap-2 text-[13px] text-[var(--color-text)]"
              >
                <input
                  type="radio"
                  name="theme-preference"
                  value={option}
                  checked={settings.themePreference === option}
                  onChange={() =>
                    void preferences.set({ themePreference: option })
                  }
                  disabled={isInitializing}
                  className="accent-[var(--color-accent)]"
                />
                {t(`settings.appearance.${option}`)}
              </label>
            ))}
          </div>
        </fieldset>

        <div className="space-y-2 border-t border-[var(--color-border)] pt-4">
          <label className="flex items-center gap-3">
            <input
              type="checkbox"
              checked={settings.hideBatchSeparate}
              onChange={(event) =>
                void preferences.set({
                  hideBatchSeparate: event.target.checked,
                })
              }
              disabled={isInitializing}
              className="h-4 w-4 rounded border-[var(--color-border-light)] bg-[var(--color-surface)] accent-[var(--color-accent)]"
            />
            <span className="text-[13px] text-[var(--color-text)]">
              {t("settings.hideBatchSeparate.hide")}
            </span>
          </label>
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.hideBatchSeparate.description")}
          </p>
        </div>

        <div className="space-y-2 border-t border-[var(--color-border)] pt-4">
          <label className="flex items-center gap-3">
            <input
              type="checkbox"
              checked={!settings.coverArtBackdrop}
              onChange={(event) =>
                void preferences.set({
                  coverArtBackdrop: !event.target.checked,
                })
              }
              disabled={isInitializing}
              className="h-4 w-4 rounded border-[var(--color-border-light)] bg-[var(--color-surface)] accent-[var(--color-accent)]"
            />
            <span className="text-[13px] text-[var(--color-text)]">
              {t("settings.coverArtBackdrop.hide")}
            </span>
          </label>
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.coverArtBackdrop.description")}
          </p>
        </div>

        <div className="space-y-2 border-t border-[var(--color-border)] pt-4">
          <label className="flex items-center gap-3">
            <input
              type="checkbox"
              checked={settings.lyricsBlurInactive}
              onChange={(event) =>
                void preferences.set({
                  lyricsBlurInactive: event.target.checked,
                })
              }
              disabled={isInitializing}
              className="h-4 w-4 rounded border-[var(--color-border-light)] bg-[var(--color-surface)] accent-[var(--color-accent)]"
            />
            <span className="text-[13px] text-[var(--color-text)]">
              {t("settings.lyricsBlurInactive.enable")}
            </span>
          </label>
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.lyricsBlurInactive.description")}
          </p>
        </div>

        <div className="space-y-2 border-t border-[var(--color-border)] pt-4">
          <label
            htmlFor="settings-output-device"
            className="text-[12px] font-medium text-[var(--color-text-dim)]"
          >
            {t("settings.outputDevice.label")}
          </label>
          <select
            id="settings-output-device"
            aria-label={t("settings.outputDevice.label")}
            className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px] text-[var(--color-text)] focus:border-[var(--color-accent)] focus:outline-none"
          >
            <option>{t("settings.outputDevice.systemDefault")}</option>
          </select>
        </div>

        <div className="space-y-2 border-t border-[var(--color-border)] pt-4">
          <label
            htmlFor="settings-language"
            className="text-[12px] font-medium text-[var(--color-text-dim)]"
          >
            {t("settings.language.label")}
          </label>
          <select
            id="settings-language"
            aria-label={t("settings.language.label")}
            value={settings.language}
            onChange={(event) =>
              void preferences.set({ language: event.target.value })
            }
            disabled={isInitializing}
            className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px] text-[var(--color-text)] focus:border-[var(--color-accent)] focus:outline-none disabled:opacity-50"
          >
            {SUPPORTED_LANGUAGES.map((language) => (
              <option key={language.code} value={language.code}>
                {t(language.nameKey)}
              </option>
            ))}
          </select>
        </div>
      </div>
    </SettingsSectionCard>
  );
}
