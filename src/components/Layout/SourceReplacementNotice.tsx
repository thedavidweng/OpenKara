import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useBackend } from "@/lib/backend";
import { notifyError } from "@/lib/errors";
import { useSettingsStore } from "@/stores/settings-store";
import type { RetiredSource } from "@/types/ipc";

export function SourceReplacementNotice() {
  const { catalog } = useBackend();
  const { t } = useTranslation();
  const openSettings = useSettingsStore((s) => s.open);
  const [retired, setRetired] = useState<RetiredSource[]>([]);

  useEffect(() => {
    let cancelled = false;
    catalog
      .getSourceReplacementNotice()
      .then((sources) => {
        if (!cancelled) setRetired(sources);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [catalog]);

  if (retired.length === 0) return null;

  const names = retired.map((source) => source.name).join(", ");
  const dismiss = () => {
    setRetired([]);
    void catalog.dismissSourceReplacementNotice().catch(notifyError);
  };

  return (
    <section
      aria-labelledby="source-replacement-notice-title"
      className="animate-expand shrink-0 border-b border-[var(--color-border)] bg-[var(--color-sidebar)] px-4 py-3"
    >
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="text-[12px] text-[var(--color-text)]">
          <h2 id="source-replacement-notice-title" className="font-semibold">
            {t("settings.streaming.replacementNotice.title")}
          </h2>
          <p className="mt-0.5 text-[11px] text-[var(--color-text-dim)]">
            {t("settings.streaming.replacementNotice.body", { names })}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            onClick={openSettings}
            className="min-h-6 shrink-0 rounded-md bg-[var(--color-control-primary)] px-3 py-1.5 text-[11px] text-[var(--color-control-primary-foreground)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-control-primary)_88%,white)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
          >
            {t("settings.streaming.replacementNotice.openSettings")}
          </button>
          <button
            type="button"
            onClick={dismiss}
            aria-label={t("settings.streaming.replacementNotice.dismiss")}
            className="min-h-6 min-w-6 shrink-0 rounded-md p-1 text-[var(--color-text-dim)] transition-colors hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
          >
            <X size={14} aria-hidden="true" />
          </button>
        </div>
      </div>
    </section>
  );
}
