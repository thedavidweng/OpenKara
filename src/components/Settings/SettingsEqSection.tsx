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

/// Trailing debounce window for slider → IPC commits. Local draft updates
/// immediately so the slider feels responsive; the complete five-value array
/// is sent once after this quiet period (or immediately on pointer/key
/// release).
const EQ_DEBOUNCE_MS = 75;

type EqGains = [number, number, number, number, number];

export function SettingsEqSection() {
  const { t } = useTranslation();
  const { state, meta, actions } = useSettingsOverlay();

  // Local draft gives immediate slider feedback without firing an IPC call
  // on every onChange tick. It syncs from the authoritative store state
  // whenever the store changes externally (reset, hydration, rollback).
  const [draft, setDraft] = useState<EqGains>(state.eqGainsDb);
  useEffect(() => {
    setDraft(state.eqGainsDb);
  }, [state.eqGainsDb]);

  // Pending debounce timer and the gains array it will commit. Stored in
  // refs so the timer survives re-renders without resetting.
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingRef = useRef<EqGains | null>(null);

  // Flush the pending debounced commit immediately, clearing the timer.
  // Called on pointer/key release so the value is persisted as soon as the
  // user lets go of the slider.
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

  // Cancel any pending debounced commit without flushing. Called on unmount
  // so a drag in progress does not fire an IPC call after the section is
  // gone.
  useEffect(() => {
    return () => {
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      pendingRef.current = null;
    };
  }, []);

  /// Update one band of the local draft and schedule a trailing 75 ms commit
  /// carrying the complete five-value array. A previous pending commit is
  /// cancelled and replaced with the latest array.
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
          {draft.map((gain, band) => (
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
                  handleBandChange(band, parseFloat(event.target.value))
                }
                onPointerUp={() => flushRef.current()}
                onKeyUp={() => flushRef.current()}
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
