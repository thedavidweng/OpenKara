import { Loader2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { downloadRuntime, restartApp } from "@/lib/tauri";
import { notifyError } from "@/lib/errors";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";

export function RuntimeUpdateBanner() {
  const { t } = useTranslation();
  const status = useRuntimeBootstrapStore((s) => s.status);
  const updateStatus = useRuntimeBootstrapStore((s) => s.updateStatus);
  const [activationDismissed, setActivationDismissed] = useState(false);
  const [retrying, setRetrying] = useState(false);

  const state = status?.state;

  const retryDownload = async () => {
    if (retrying) return;
    setRetrying(true);
    try {
      const snapshot = await downloadRuntime();
      updateStatus(snapshot);
    } catch (error) {
      notifyError(error);
    } finally {
      setRetrying(false);
    }
  };

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

  if (state === "missing") {
    return (
      <div className="animate-expand shrink-0 border-b border-[var(--color-border)] bg-[var(--color-sidebar)] px-4 py-3">
        <div className="flex flex-col gap-0.5">
          <span className="text-[12px] text-[var(--color-text)]">
            {t("settings.runtime.banner.runtimeRequired")}
          </span>
          <span className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.runtime.banner.runtimeRequiredHint")}
          </span>
        </div>
      </div>
    );
  }

  if (state === "downloading") {
    return (
      <div className="animate-expand shrink-0 border-b border-[var(--color-border)] bg-[var(--color-sidebar)] px-4 py-3">
        <div className="flex items-center gap-2 text-[12px] text-[var(--color-text)]">
          <Loader2 size={12} className="animate-spin" />
          {t("settings.runtime.banner.downloadingRuntime")}
        </div>
      </div>
    );
  }

  if (state === "failed") {
    return (
      <div className="animate-expand shrink-0 border-b border-[var(--color-border)] bg-[var(--color-sidebar)] px-4 py-3">
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div className="text-[12px]">
            <p className="text-[var(--color-destructive)]">
              {t("settings.runtime.banner.downloadFailed", {
                error: status?.error?.message || t("bootstrap.unknownError"),
              })}
            </p>
            <p className="mt-0.5 text-[11px] text-[var(--color-text-dim)]">
              {t("settings.runtime.banner.downloadFailedHint")}
            </p>
          </div>
          <button
            type="button"
            onClick={() => void retryDownload()}
            disabled={retrying}
            className="flex shrink-0 items-center gap-1.5 self-start rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-1.5 text-[11px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 disabled:cursor-not-allowed disabled:opacity-60 sm:self-center"
          >
            {retrying && <Loader2 size={12} className="animate-spin" />}
            {t("settings.runtime.banner.retryDownload")}
          </button>
        </div>
      </div>
    );
  }

  return null;
}
