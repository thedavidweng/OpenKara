import { useState } from "react";
import { useTranslation } from "react-i18next";
import { restartApp } from "@/lib/tauri";
import { notifyError } from "@/lib/errors";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";

export function RuntimeUpdateBanner() {
  const { t } = useTranslation();
  const status = useRuntimeBootstrapStore((s) => s.status);
  const [activationDismissed, setActivationDismissed] = useState(false);

  const state = status?.state;

  if (state === "candidate_ready_restart_required") {
    return (
      <div className="animate-expand shrink-0 border-b border-[var(--color-border)] bg-[var(--color-sidebar)] px-4 py-3">
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <span className="text-[12px] text-[var(--color-text)]">
            {t("settings.runtime.banner.updateReady")}
          </span>
          <button
            type="button"
            onClick={() => {
              void restartApp().catch(notifyError);
            }}
            className="shrink-0 self-start rounded-md bg-[var(--color-control-primary)] px-3 py-1.5 text-[11px] text-[var(--color-control-primary-foreground)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-control-primary)_88%,white)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 sm:self-center"
          >
            {t("settings.runtime.restartButton")}
          </button>
        </div>
      </div>
    );
  }

  if (state === "activation_failed_previous_restored" && !activationDismissed) {
    return (
      <div className="animate-expand shrink-0 border-b border-[var(--color-border)] bg-[var(--color-sidebar)] px-4 py-3">
        <div className="flex items-center justify-between gap-2">
          <span className="text-[12px] text-[var(--color-destructive)]">
            {t("settings.runtime.banner.activationFailed")}
          </span>
          <button
            type="button"
            onClick={() => setActivationDismissed(true)}
            aria-label={t("common.close")}
            className="shrink-0 rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-1.5 text-[11px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
          >
            {t("common.close")}
          </button>
        </div>
      </div>
    );
  }

  return null;
}
