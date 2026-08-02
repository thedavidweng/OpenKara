import type { HTMLAttributes } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@/stores/settings-store";

type LyricsFontSizeControlProps = HTMLAttributes<HTMLDivElement>;

function fontSizeStepKey(step: number) {
  switch (step) {
    case -2:
      return "lyrics.fontSizeExtraSmall";
    case -1:
      return "lyrics.fontSizeSmall";
    case 1:
      return "lyrics.fontSizeLarge";
    case 2:
      return "lyrics.fontSizeExtraLarge";
    default:
      return "lyrics.fontSizeMedium";
  }
}

export function LyricsFontSizeControl({
  className = "",
  ...props
}: LyricsFontSizeControlProps) {
  const { t } = useTranslation();
  const lyricsFontStep = useSettingsStore((s) => s.lyricsFontStep);
  const adjustLyricsFontStep = useSettingsStore((s) => s.adjustLyricsFontStep);
  const resetLyricsFontStep = useSettingsStore((s) => s.resetLyricsFontStep);

  return (
    <div
      className={`flex shrink-0 items-center gap-2 rounded-full border border-[var(--color-border-light)] bg-[var(--color-sidebar)] px-2.5 py-2 text-[11px] text-[var(--color-text-dim)] ${className}`}
      {...props}
    >
      <button
        onClick={() => void adjustLyricsFontStep(-1)}
        className="motion-surface rounded-full border border-[var(--color-border-light)] px-2.5 py-1 font-medium hover:border-[color-mix(in_srgb,var(--color-accent)_28%,var(--color-border-light))] hover:bg-[color-mix(in_srgb,var(--color-hover)_72%,transparent)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
        aria-label={t("lyrics.fontSizeDecrease")}
      >
        {t("lyrics.fontSizeDecreaseShort")}
      </button>
      <div className="min-w-[3.25rem] text-center">
        <div
          className={`tabular-nums text-[12px] font-medium ${
            lyricsFontStep === 0
              ? "text-[var(--color-text-dim)]"
              : "text-[var(--color-control-primary)]"
          }`}
        >
          {t(fontSizeStepKey(lyricsFontStep))}
        </div>
      </div>
      <button
        onClick={() => void adjustLyricsFontStep(1)}
        className="motion-surface rounded-full border border-[var(--color-border-light)] px-2.5 py-1 font-medium hover:border-[color-mix(in_srgb,var(--color-accent)_28%,var(--color-border-light))] hover:bg-[color-mix(in_srgb,var(--color-hover)_72%,transparent)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
        aria-label={t("lyrics.fontSizeIncrease")}
      >
        {t("lyrics.fontSizeIncreaseShort")}
      </button>
      <button
        onClick={() => void resetLyricsFontStep()}
        className="motion-surface rounded-full border border-[var(--color-border-light)] px-2.5 py-1 font-medium hover:border-[color-mix(in_srgb,var(--color-accent)_28%,var(--color-border-light))] hover:bg-[color-mix(in_srgb,var(--color-hover)_72%,transparent)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
        aria-label={t("lyrics.fontSizeReset")}
      >
        {t("lyrics.fontSizeResetShort")}
      </button>
    </div>
  );
}
