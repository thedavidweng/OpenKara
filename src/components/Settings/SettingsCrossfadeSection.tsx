import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettingsOverlay } from "./SettingsOverlay.context";

/** Accepted crossfade duration range in milliseconds. */
const CROSSFADE_MIN_MS = 500;
const CROSSFADE_MAX_MS = 10_000;
const CROSSFADE_STEP_MS = 100;

/** Trailing debounce delay for batched duration persistence (ms). */
const CROSSFADE_DURATION_DEBOUNCE_MS = 75;

function formatDuration(ms: number): string {
  const seconds = ms / 1000;
  return `${seconds.toFixed(1)} s`;
}

export function SettingsCrossfadeSection() {
  const { t } = useTranslation();
  const { state, meta, actions } = useSettingsOverlay();

  // Local draft mirrors the authoritative duration for immediate slider
  // feedback. The 75 ms trailing debounce batches persistence.
  const [draft, setDraft] = useState<number>(state.crossfadeDurationMs);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Re-sync the local draft when the authoritative store value changes.
  useEffect(() => {
    setDraft(state.crossfadeDurationMs);
  }, [state.crossfadeDurationMs]);

  // Cancel any pending debounced call on unmount.
  useEffect(() => {
    return () => {
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, []);

  const flushDuration = useCallback(
    (durationMs: number) => {
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      void actions.setCrossfadeDurationMs(durationMs);
    },
    [actions],
  );

  const handleDurationChange = useCallback(
    (value: number) => {
      setDraft(value);
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
      }
      timerRef.current = setTimeout(() => {
        timerRef.current = null;
        void actions.setCrossfadeDurationMs(value);
      }, CROSSFADE_DURATION_DEBOUNCE_MS);
    },
    [actions],
  );

  const disabled = meta.isInitializing || !state.crossfadeEnabled;

  return (
    <SettingsSectionCard
      title={t("settings.crossfade.label")}
      description={t("settings.crossfade.description")}
    >
      <div className="space-y-4">
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
          <span className="text-[13px] text-white">
            {t("settings.crossfade.enable")}
          </span>
        </label>

        <div className="space-y-3 border-t border-[var(--color-border)] pt-4">
          <div className="flex items-center gap-3">
            <label
              className="w-20 shrink-0 text-[12px] text-[var(--color-text-dim)]"
              htmlFor="crossfade-duration"
            >
              {t("settings.crossfade.duration")}
            </label>
            <input
              id="crossfade-duration"
              type="range"
              min={CROSSFADE_MIN_MS}
              max={CROSSFADE_MAX_MS}
              step={CROSSFADE_STEP_MS}
              value={draft}
              onChange={(event) =>
                handleDurationChange(parseInt(event.target.value, 10))
              }
              onPointerUp={() => flushDuration(draft)}
              onKeyUp={() => flushDuration(draft)}
              disabled={disabled}
              className="flex-1 accent-[var(--color-accent)] disabled:opacity-50"
            />
            <span className="w-16 shrink-0 text-right text-[12px] tabular-nums text-white">
              {formatDuration(draft)}
            </span>
          </div>
        </div>
      </div>
    </SettingsSectionCard>
  );
}
