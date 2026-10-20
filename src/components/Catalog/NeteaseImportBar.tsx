import { useState } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useCatalogStore } from "@/stores/catalog-store";

const focusRing =
  "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]";

export function NeteaseImportBar() {
  const { t } = useTranslation();
  const run = useCatalogStore((s) => s.importRun);
  const failures = useCatalogStore((s) => s.importFailures);
  const dismiss = useCatalogStore((s) => s.dismissImportRun);
  const cancel = useCatalogStore((s) => s.cancelImport);
  const [showSkipped, setShowSkipped] = useState(false);

  if (!run) return null;

  const skippedToggle =
    failures.length > 0 ? (
      <button
        type="button"
        aria-expanded={showSkipped}
        onClick={() => setShowSkipped((open) => !open)}
        className={`rounded px-1 text-[12px] text-[var(--color-accent)] hover:underline ${focusRing}`}
      >
        {showSkipped
          ? t("catalog.netease.hideSkipped")
          : t("catalog.netease.viewSkipped")}
      </button>
    ) : null;
  const skippedList =
    showSkipped && failures.length > 0 ? (
      <ul className="mt-2 max-h-40 space-y-1 overflow-auto text-[12px] text-[var(--color-text-dim)]">
        {failures.map((failure, index) => (
          <li key={`${failure.remote_track_id}-${index}`}>
            <span className="text-[var(--color-text)]">
              {failure.title || failure.remote_track_id}
            </span>
            {failure.artist ? ` — ${failure.artist}` : ""} ·{" "}
            {failure.refusal
              ? t(`catalog.netease.refusalReason.${failure.refusal.reason}`)
              : t(`catalog.netease.skipReason.${failure.reason}`)}
          </li>
        ))}
      </ul>
    ) : null;

  return (
    <div
      data-testid="netease-import-bar"
      className="absolute bottom-4 left-1/2 z-10 w-[min(440px,calc(100%-2rem))] -translate-x-1/2 overflow-hidden rounded-2xl border border-[var(--color-border-light)] bg-[var(--color-sidebar)] px-4 pb-2.5 pt-2 shadow-[var(--shadow-popover)]"
    >
      {run.phase === "running" ? (
        <div className="space-y-1.5">
          <div className="flex items-center gap-3">
            <p
              role="status"
              className="min-w-0 flex-1 truncate text-[13px] text-[var(--color-text)]"
            >
              {run.cancelling
                ? t("catalog.netease.cancelling")
                : t("catalog.netease.importingProgress", {
                    completed: run.completed,
                    total: run.total,
                  })}
            </p>
            {run.skipped > 0 ? (
              <>
                <span className="text-[12px] text-[var(--color-text-dim)]">
                  {t("catalog.netease.skippedSoFar", { skipped: run.skipped })}
                </span>
                {skippedToggle}
              </>
            ) : null}
            <button
              type="button"
              disabled={run.cancelling}
              onClick={() => void cancel()}
              className={`rounded px-1 text-[12px] text-[var(--color-accent)] hover:underline disabled:opacity-50 ${focusRing}`}
            >
              {t("catalog.netease.cancelImport")}
            </button>
          </div>
          <div
            role="progressbar"
            aria-label={t("catalog.netease.importProgressLabel")}
            aria-valuemin={0}
            aria-valuemax={run.total}
            aria-valuenow={run.completed}
            className="h-1 overflow-hidden rounded-full bg-[var(--color-border)]"
          >
            <div
              className="h-full rounded-full bg-[var(--color-accent)] transition-[width]"
              style={{
                width: `${run.total > 0 ? (run.completed / run.total) * 100 : 0}%`,
              }}
            />
          </div>
          {skippedList}
        </div>
      ) : (
        <>
          <div className="flex items-center gap-3">
            <p
              role="status"
              className="min-w-0 flex-1 text-[13px] text-[var(--color-text)]"
            >
              {t("catalog.netease.importDone", {
                imported: run.imported,
                skipped: run.skipped,
              })}
            </p>
            {skippedToggle}
            <button
              type="button"
              aria-label={t("common.close")}
              onClick={dismiss}
              className={`inline-flex size-7 items-center justify-center rounded text-[var(--color-text-dim)] hover:bg-[var(--sidebar-row-overlay-bg)] ${focusRing}`}
            >
              <X size={14} aria-hidden="true" />
            </button>
          </div>
          {skippedList}
        </>
      )}
    </div>
  );
}
