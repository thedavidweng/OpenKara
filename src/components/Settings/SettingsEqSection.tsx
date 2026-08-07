import { useEffect, useRef, useState } from "react";
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

const EQ_BAND_NAME_KEYS = [
  "settings.eq.bandNameBass",
  "settings.eq.bandNameLowMid",
  "settings.eq.bandNameMid",
  "settings.eq.bandNameHighMid",
  "settings.eq.bandNameTreble",
] as const;

const EQ_DEBOUNCE_MS = 75;

type EqGains = [number, number, number, number, number];

const EQ_PRESETS = [
  { key: "settings.eq.presetFlat", gains: [0, 0, 0, 0, 0] },
  { key: "settings.eq.presetVocalBoost", gains: [-1, -1, 2, 4, 1] },
  { key: "settings.eq.presetBassBoost", gains: [6, 3, 0, 0, 1] },
  { key: "settings.eq.presetTrebleBoost", gains: [0, 0, 0, 3, 6] },
  { key: "settings.eq.presetWarm", gains: [3, 2, 0, -1, -2] },
  { key: "settings.eq.presetBright", gains: [-2, -1, 0, 2, 4] },
  { key: "settings.eq.presetRock", gains: [4, 2, -1, 2, 4] },
  { key: "settings.eq.presetPop", gains: [2, 1, 2, 3, 2] },
] as const satisfies ReadonlyArray<{
  key: string;
  gains: readonly [number, number, number, number, number];
}>;

const EQ_PRESET_MATCH_EPSILON = 0.25;

function matchEqPreset(gains: EqGains): string | null {
  for (const preset of EQ_PRESETS) {
    if (
      preset.gains.every(
        (gain, band) => Math.abs(gain - gains[band]) < EQ_PRESET_MATCH_EPSILON,
      )
    ) {
      return preset.key;
    }
  }
  return null;
}

export function SettingsEqSection() {
  const { t } = useTranslation();
  const { state, meta, actions } = useSettingsOverlay();

  const [draft, setDraft] = useState<EqGains>(state.eqGainsDb);
  useEffect(() => {
    setDraft(state.eqGainsDb);
  }, [state.eqGainsDb]);

  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingRef = useRef<EqGains | null>(null);

  const flushRef = useRef<() => void>(() => {});
  flushRef.current = () => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    const pending = pendingRef.current;
    pendingRef.current = null;
    if (pending !== null) {
      void actions.setEqGains(pending);
    }
  };

  useEffect(() => {
    return () => {
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      pendingRef.current = null;
    };
  }, []);

  const handleBandChange = (band: number, gainDb: number) => {
    const clamped = Math.max(-12, Math.min(12, gainDb));
    setDraft((prev) => {
      if (prev[band] === clamped) {
        return prev;
      }
      const next = [...prev] as EqGains;
      next[band] = clamped;
      pendingRef.current = next;
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
      }
      timerRef.current = setTimeout(() => {
        timerRef.current = null;
        const pending = pendingRef.current;
        pendingRef.current = null;
        if (pending !== null) {
          void actions.setEqGains(pending);
        }
      }, EQ_DEBOUNCE_MS);
      return next;
    });
  };

  const activePresetKey = matchEqPreset(draft);

  const applyPreset = (
    gains: readonly [number, number, number, number, number],
  ) => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    pendingRef.current = null;
    setDraft([...gains] as EqGains);
    void actions.setEqGains([...gains] as EqGains);
  };

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
            <span className="text-[13px] text-[var(--color-text)]">
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
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <span className="text-[12px] font-medium text-[var(--color-text-dim)]">
                {t("settings.eq.presetLabel")}
              </span>
              {activePresetKey === null ? (
                <span className="text-[11px] text-[var(--color-text-dim)]">
                  {t("settings.eq.presetCustom")}
                </span>
              ) : null}
            </div>
            <div className="flex flex-wrap gap-1.5">
              {EQ_PRESETS.map((preset) => (
                <button
                  key={preset.key}
                  type="button"
                  onClick={() => applyPreset(preset.gains)}
                  disabled={meta.isInitializing || !state.eqEnabled}
                  aria-pressed={activePresetKey === preset.key}
                  className={`rounded-full border px-2.5 py-1 text-[11px] transition-colors disabled:opacity-50 ${
                    activePresetKey === preset.key
                      ? "border-[var(--color-control-selected-border)] bg-[var(--color-control-selected-bg)] text-[var(--color-text)]"
                      : "border-[var(--color-border-light)] bg-[var(--color-surface)] text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
                  }`}
                >
                  {t(preset.key)}
                </button>
              ))}
            </div>
          </div>

          {draft.map((gain, band) => {
            const bandInputId = `settings-eq-band-${band}`;
            const bandLabel = `${t(EQ_BAND_NAME_KEYS[band])} (${t(EQ_BAND_KEYS[band])})`;
            return (
              <div key={band} className="space-y-1">
                <div className="flex items-center justify-between">
                  <label
                    htmlFor={bandInputId}
                    className="flex items-baseline gap-2 text-[12px]"
                  >
                    <span className="font-medium text-[var(--color-text)]">
                      {t(EQ_BAND_NAME_KEYS[band])}
                    </span>
                    <span className="text-[10px] text-[var(--color-text-dim)]">
                      {t(EQ_BAND_KEYS[band])}
                    </span>
                  </label>
                  <span className="text-[11px] tabular-nums text-[var(--color-text-dim)]">
                    {t("settings.eq.gainValue", {
                      value: `${gain > 0 ? "+" : ""}${gain.toFixed(1)}`,
                    })}
                  </span>
                </div>
                <input
                  id={bandInputId}
                  type="range"
                  min={-12}
                  max={12}
                  step={0.5}
                  value={gain}
                  aria-label={bandLabel}
                  onChange={(event) =>
                    handleBandChange(band, parseFloat(event.target.value))
                  }
                  onPointerUp={() => flushRef.current()}
                  onKeyUp={() => flushRef.current()}
                  disabled={meta.isInitializing || !state.eqEnabled}
                  className="w-full accent-[var(--color-accent)]"
                />
              </div>
            );
          })}

          <div
            aria-hidden="true"
            className="flex items-center justify-between text-[10px] tabular-nums text-[var(--color-text-dim)]"
          >
            <span>{t("settings.eq.minimumGain")}</span>
            <span>{t("settings.eq.neutralGain")}</span>
            <span>{t("settings.eq.maximumGain")}</span>
          </div>

          <button
            type="button"
            onClick={() => {
              if (timerRef.current !== null) {
                clearTimeout(timerRef.current);
                timerRef.current = null;
              }
              pendingRef.current = null;
              void actions.resetEqGains();
            }}
            disabled={meta.isInitializing || !state.eqEnabled}
            className="text-[11px] text-[var(--color-text-dim)] underline hover:text-[var(--color-text)] disabled:opacity-50"
          >
            {t("settings.eq.reset")}
          </button>
        </div>
      </div>
    </SettingsSectionCard>
  );
}
