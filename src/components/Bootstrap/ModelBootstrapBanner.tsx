import { Loader2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { notifyError } from "@/lib/errors";
import { downloadModel } from "@/lib/tauri";
import { useBootstrapStore } from "@/stores/bootstrap-store";
import { useSettingsStore } from "@/stores/settings-store";

export function ModelBootstrapBanner() {
  const { t } = useTranslation();
  const status = useBootstrapStore((s) => s.status);
  const updateStatus = useBootstrapStore((s) => s.updateStatus);
  const openSettings = useSettingsStore((s) => s.open);
  const modelVariant = useSettingsStore((s) => s.modelVariant);
  const [retrying, setRetrying] = useState(false);

  const retryDownload = async () => {
    if (retrying) return;
    setRetrying(true);
    try {
      const snapshot = await downloadModel(modelVariant);
      updateStatus(snapshot);
    } catch (error) {
      notifyError(error);
    } finally {
      setRetrying(false);
    }
  };

  if (!status || status.state === "ready") return null;

  return (
    <div
      className="animate-expand shrink-0 border-b border-[var(--color-border)] bg-[var(--color-sidebar)] px-4 py-3"
      role={status.state === "failed" ? "alert" : "status"}
      aria-live={status.state === "failed" ? "assertive" : "polite"}
      aria-atomic="true"
    >
      {status.state === "pending" && (
        <div className="flex items-center justify-between">
          <span className="text-[12px] text-[var(--color-text)]">
            {t("bootstrap.modelRequired")}
          </span>
          <span className="text-[11px] text-[var(--color-text-dim)]">
            {t("bootstrap.downloadingBackground")}
          </span>
        </div>
      )}

      {status.state === "downloading" && (
        <div className="flex items-center justify-between text-[12px]">
          <span className="flex items-center gap-2 text-[var(--color-text)]">
            <Loader2 size={12} className="animate-spin" />
            {t("bootstrap.downloadingModel")}
          </span>
        </div>
      )}

      {status.state === "outdated" && (
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div className="text-[12px] text-[var(--color-text)]">
            <p>{t("bootstrap.outdatedModel")}</p>
            <p className="mt-0.5 text-[11px] text-[var(--color-text-dim)]">
              {t("bootstrap.outdatedModelHint")}
            </p>
          </div>
          <button
            type="button"
            onClick={() => openSettings()}
            className="shrink-0 self-start rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-1.5 text-[11px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 sm:self-center"
          >
            {t("bootstrap.openSettingsToUpgrade")}
          </button>
        </div>
      )}

      {status.state === "failed" && (
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div className="text-[12px]">
            <p className="text-[var(--color-destructive)]">
              {t("bootstrap.downloadFailed", {
                error: status.error?.message || t("bootstrap.unknownError"),
              })}
            </p>
            <p className="mt-0.5 text-[11px] text-[var(--color-text-dim)]">
              {t("bootstrap.downloadFailedHint")}
            </p>
          </div>
          <button
            type="button"
            onClick={() => void retryDownload()}
            disabled={retrying}
            className="flex shrink-0 items-center gap-1.5 self-start rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-1.5 text-[11px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 disabled:cursor-not-allowed disabled:opacity-60 sm:self-center"
          >
            {retrying && <Loader2 size={12} className="animate-spin" />}
            {t("bootstrap.retryDownload")}
          </button>
        </div>
      )}
    </div>
  );
}
