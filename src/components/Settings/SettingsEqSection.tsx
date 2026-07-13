import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettingsOverlay } from "./SettingsOverlay.context";

const EQ_BAND_KEYS = [
  "settings.eq.band60",
  "settings.eq.band230",
  "settings.eq.band910",
  "settings.eq.band3600",
  "settings.eq.band14000",
] as const;

export function SettingsEqSection() {
  const { t } = useTranslation();
  const { state, meta, actions } = useSettingsOverlay();

  return (
    <SettingsSectionCard title={t("settings.eq.label")}>
      <div className="space-y-4">
        <div className="space-y-2">
          <label className="flex items-center gap-3">
            <input
              type="checkbox"
              checked={state.eqEnabled}
              onChange={(event) =>
                void actions.setEqEnabled(event.target.checked)
              }
              disabled={meta.isInitializing}
              className="h-4 w-4 rounded border-[var(--color-border-light)] bg-[var(--color-surface)] accent-[var(--color-accent)]"
            />
            <span className="text-[13px] text-white">
              {t("settings.eq.enable")}
            </span>
          </label>
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.eq.description")}
          </p>
        </div>

        <div
          className={`space-y-3 border-t border-[var(--color-border)] pt-4 ${
            state.eqEnabled ? "" : "opacity-50"
          }`}
        >
          {state.eqGainsDb.map((gain, band) => (
            <div key={band} className="space-y-1">
              <div className="flex items-center justify-between">
                <label className="text-[12px] font-medium text-[var(--color-text-dim)]">
                  {t(EQ_BAND_KEYS[band])}
                </label>
                <span className="text-[11px] tabular-nums text-[var(--color-text-dim)]">
                  {gain > 0 ? "+" : ""}
                  {gain.toFixed(1)} dB
                </span>
              </div>
              <input
                type="range"
                min={-12}
                max={12}
                step={0.5}
                value={gain}
                onChange={(event) =>
                  void actions.setEqBandGain(
                    band,
                    parseFloat(event.target.value),
                  )
                }
                disabled={meta.isInitializing || !state.eqEnabled}
                className="w-full accent-[var(--color-accent)]"
              />
            </div>
          ))}

          <button
            type="button"
            onClick={() => void actions.resetEqGains()}
            disabled={meta.isInitializing || !state.eqEnabled}
            className="text-[11px] text-[var(--color-text-dim)] underline hover:text-white disabled:opacity-50"
          >
            {t("settings.eq.reset")}
          </button>
        </div>
      </div>
    </SettingsSectionCard>
  );
}
