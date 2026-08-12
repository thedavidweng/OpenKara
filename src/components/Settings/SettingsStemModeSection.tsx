import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettings } from "./SettingsController.context";
import type { StemMode } from "@/types/ipc";

interface StemModeOptionProps {
  selected: boolean;
  disabled: boolean;
  title: ReactNode;
  description: ReactNode;
  onClick: () => void;
}

function StemModeOption({
  selected,
  disabled,
  title,
  description,
  onClick,
}: StemModeOptionProps) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`flex-1 rounded-md border px-3 py-2 text-[13px] transition-colors ${
        selected
          ? "border-[var(--color-accent)] bg-[var(--color-accent)]/15 text-[var(--color-text)]"
          : "border-[var(--color-border-light)] bg-[var(--color-surface)] text-[var(--color-text)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text)]"
      } disabled:opacity-50`}
    >
      <div className="font-medium">{title}</div>
      <div className="mt-0.5 text-[11px] opacity-70">{description}</div>
    </button>
  );
}

export function SettingsStemModeSection() {
  const { t } = useTranslation();
  const { view, preferences } = useSettings();
  const { isInitializing } = view;
  const settings = view.preferences;

  const selectMode = (mode: StemMode) => {
    void preferences.set({ stemMode: mode });
  };

  return (
    <SettingsSectionCard
      title={t("settings.stemMode.label")}
      description={t("settings.stemMode.description")}
    >
      <div className="flex gap-2">
        <StemModeOption
          selected={settings.stemMode === "two_stem"}
          disabled={isInitializing}
          title={t("settings.stemMode.twoStem")}
          description={t("settings.stemMode.twoStemDescription")}
          onClick={() => selectMode("two_stem")}
        />
        <StemModeOption
          selected={settings.stemMode === "four_stem"}
          disabled={isInitializing}
          title={t("settings.stemMode.fourStem")}
          description={t("settings.stemMode.fourStemDescription")}
          onClick={() => selectMode("four_stem")}
        />
      </div>

      <div className="space-y-2 border-t border-[var(--color-border)] pt-4">
        <label className="flex items-center gap-3">
          <input
            type="checkbox"
            checked={settings.hideUpgradeAll}
            onChange={(event) =>
              void preferences.set({ hideUpgradeAll: event.target.checked })
            }
            disabled={isInitializing}
            className="h-4 w-4 rounded border-[var(--color-border-light)] bg-[var(--color-surface)] accent-[var(--color-accent)]"
          />
          <span className="text-[13px] text-[var(--color-text)]">
            {t("settings.hideUpgradeAll.hide")}
          </span>
        </label>
        <p className="text-[11px] text-[var(--color-text-dim)]">
          {t("settings.hideUpgradeAll.description")}
        </p>
      </div>
    </SettingsSectionCard>
  );
}
