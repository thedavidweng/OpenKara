import type { HTMLAttributes } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@/stores/settings-store";

type LyricsFontSizeControlProps = HTMLAttributes<HTMLDivElement>;

const FONT_STEP_LABELS: Record<number, string> = {
  [-2]: "XS",
  [-1]: "S",
  [0]: "M",
  [1]: "L",
  [2]: "XL",
};

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
        A-
      </button>
      <div className="min-w-[3.25rem] text-center">
        <div
          className={`tabular-nums text-[12px] font-medium ${
            lyricsFontStep === 0
              ? "text-[var(--color-text-dim)]"
              : "text-[var(--color-control-primary)]"
          }`}
        >
          {FONT_STEP_LABELS[lyricsFontStep] ?? "M"}
        </div>
      </div>
      <button
        onClick={() => void adjustLyricsFontStep(1)}
        className="motion-surface rounded-full border border-[var(--color-border-light)] px-2.5 py-1 font-medium hover:border-[color-mix(in_srgb,var(--color-accent)_28%,var(--color-border-light))] hover:bg-[color-mix(in_srgb,var(--color-hover)_72%,transparent)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
        aria-label={t("lyrics.fontSizeIncrease")}
      >
        A+
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
