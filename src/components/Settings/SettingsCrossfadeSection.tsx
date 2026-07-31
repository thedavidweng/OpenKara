import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettingsOverlay } from "./SettingsOverlay.context";

const CROSSFADE_DEBOUNCE_MS = 75;

const CROSSFADE_MIN_MS = 500;
const CROSSFADE_MAX_MS = 10_000;

export function SettingsCrossfadeSection() {
  const { t } = useTranslation();
  const { state, meta, actions } = useSettingsOverlay();

  const [draft, setDraft] = useState(state.crossfadeDurationMs);
  useEffect(() => {
    setDraft(state.crossfadeDurationMs);
  }, [state.crossfadeDurationMs]);

  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingRef = useRef<number | null>(null);

  const flushRef = useRef<() => void>(() => {});
  flushRef.current = () => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    const pending = pendingRef.current;
    pendingRef.current = null;
    if (pending !== null) {
      void actions.setCrossfadeDurationMs(pending);
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

  const handleDurationChange = (durationMs: number) => {
    const clamped = Math.max(
      CROSSFADE_MIN_MS,
      Math.min(CROSSFADE_MAX_MS, Math.round(durationMs)),
    );
    setDraft((prev) => {
      if (prev === clamped) {
        return prev;
      }
      pendingRef.current = clamped;
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
      }
      timerRef.current = setTimeout(() => {
        timerRef.current = null;
        const pending = pendingRef.current;
        pendingRef.current = null;
        if (pending !== null) {
          void actions.setCrossfadeDurationMs(pending);
        }
      }, CROSSFADE_DEBOUNCE_MS);
      return clamped;
    });
  };

  return (
    <SettingsSectionCard title={t("settings.crossfade.label")}>
      <div className="space-y-4">
        <div className="space-y-2">
          <label className="flex items-center gap-3">
            <input
              type="checkbox"
              checked={state.crossfadeEnabled}
              onChange={(event) =>
                void actions.setCrossfadeEnabled(event.target.checked)
              }
              disabled={meta.isInitializing}
              className="h-4 w-4 rounded border-[var(--color-border-light)] bg-[var(--color-surface)] accent-[var(--color-accent)]"
            />
            <span className="text-[13px] text-[var(--color-text)]">
              {t("settings.crossfade.enable")}
            </span>
          </label>
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.crossfade.description")}
          </p>
        </div>

        <div
          className={`space-y-2 border-t border-[var(--color-border)] pt-4 ${
            state.crossfadeEnabled ? "" : "opacity-50"
          }`}
        >
          <div className="flex items-center justify-between">
            <label
              htmlFor="settings-crossfade-duration"
              className="text-[12px] font-medium text-[var(--color-text-dim)]"
            >
              {t("settings.crossfade.duration")}
            </label>
            <span className="text-[11px] tabular-nums text-[var(--color-text-dim)]">
              {(draft / 1000).toFixed(1)} s
            </span>
          </div>
          <input
            id="settings-crossfade-duration"
            type="range"
            min={CROSSFADE_MIN_MS}
            max={CROSSFADE_MAX_MS}
            step={100}
            value={draft}
            aria-label={t("settings.crossfade.duration")}
            onChange={(event) =>
              handleDurationChange(parseInt(event.target.value, 10))
            }
            onPointerUp={() => flushRef.current()}
            onKeyUp={() => flushRef.current()}
            disabled={meta.isInitializing || !state.crossfadeEnabled}
            className="w-full accent-[var(--color-accent)]"
          />
        </div>
      </div>
    </SettingsSectionCard>
  );
}
