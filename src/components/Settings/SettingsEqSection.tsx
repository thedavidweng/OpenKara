import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettingsOverlay } from "./SettingsOverlay.context";
import type { EqGains } from "@/stores/settings-store";

const EQ_BAND_LABELS: [string, string, string, string, string] = [
  "60 Hz",
  "230 Hz",
  "910 Hz",
  "3.6 kHz",
  "14 kHz",
];

/** Trailing debounce delay for batched gain persistence (ms). */
const EQ_GAIN_DEBOUNCE_MS = 75;

function formatGainDb(gain: number): string {
  return gain > 0 ? `+${gain.toFixed(1)}` : gain.toFixed(1);
}

export function SettingsEqSection() {
  const { t } = useTranslation();
  const { state, meta, actions } = useSettingsOverlay();

  // Local draft mirrors the authoritative gains for immediate slider
  // feedback. The 75 ms trailing debounce batches the complete five-value
  // array into a single `setEqGains` call.
  const [draft, setDraft] = useState<EqGains>(state.eqGainsDb);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Re-sync the local draft when the authoritative store value changes
  // (e.g. after hydration, reset, or a rollback from a failed API call).
  useEffect(() => {
    setDraft(state.eqGainsDb);
  }, [state.eqGainsDb]);

  // Cancel any pending debounced call on unmount.
  useEffect(() => {
    return () => {
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, []);

  const flushGains = useCallback(
    (gains: EqGains) => {
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      void actions.setEqGains(gains);
    },
    [actions],
  );

  const handleGainChange = useCallback(
    (index: number, value: number) => {
      const next: EqGains = [...draft] as EqGains;
      next[index] = value;
      setDraft(next);

      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
      }
      timerRef.current = setTimeout(() => {
        timerRef.current = null;
        void actions.setEqGains(next);
      }, EQ_GAIN_DEBOUNCE_MS);
    },
    [draft, actions],
  );

  const handleReset = useCallback(() => {
    flushGains([0, 0, 0, 0, 0]);
  }, [flushGains]);

  const disabled = meta.isInitializing || !state.eqEnabled;

  return (
    <SettingsSectionCard
      title={t("settings.eq.label")}
      description={t("settings.eq.description")}
    >
      <div className="space-y-4">
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

        <div className="space-y-3 border-t border-[var(--color-border)] pt-4">
          {EQ_BAND_LABELS.map((label, index) => (
            <div key={label} className="flex items-center gap-3">
              <label
                className="w-16 shrink-0 text-[12px] text-[var(--color-text-dim)]"
                htmlFor={`eq-band-${index}`}
              >
                {label}
              </label>
              <input
                id={`eq-band-${index}`}
                type="range"
                min={-12}
                max={12}
                step={0.5}
                value={draft[index]}
                onChange={(event) =>
                  handleGainChange(index, parseFloat(event.target.value))
                }
                onPointerUp={() => flushGains(draft)}
                onKeyUp={() => flushGains(draft)}
                disabled={disabled}
                className="flex-1 accent-[var(--color-accent)] disabled:opacity-50"
              />
              <span className="w-12 shrink-0 text-right text-[12px] tabular-nums text-white">
                {formatGainDb(draft[index])} dB
              </span>
            </div>
          ))}
        </div>

        <div className="border-t border-[var(--color-border)] pt-4">
          <button
            type="button"
            onClick={handleReset}
            disabled={disabled}
            className="rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] hover:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 disabled:opacity-50"
          >
            {t("settings.eq.reset")}
          </button>
        </div>
      </div>
    </SettingsSectionCard>
  );
}
