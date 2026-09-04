import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { useCatalogStore } from "@/stores/catalog-store";
import { useModalDialog } from "@/hooks/use-modal-dialog";
import { DialogBackdrop } from "@/components/Overlay/DialogBackdrop";
import { formatBytes, formatDuration } from "@/lib/format";
import type { LibraryDecisionMeta } from "@/types/ipc";

function MetaList({ meta }: { meta: LibraryDecisionMeta }) {
  return (
    <dl className="mt-2 grid grid-cols-2 gap-1 text-[11px] text-[var(--color-text-dim)]">
      <dt>{meta.title ?? "—"}</dt>
      <dd>{meta.artist ?? "—"}</dd>
      <dt>{meta.album ?? "—"}</dt>
      <dd>{meta.format}</dd>
      {meta.bit_rate_bps != null ? <dd>{meta.bit_rate_bps} kbps</dd> : null}
      {meta.duration_ms != null ? (
        <dd>{formatDuration(meta.duration_ms)}</dd>
      ) : null}
      <dd>{formatBytes(meta.file_size_bytes)}</dd>
    </dl>
  );
}

export function LibraryDecisionDialog() {
  const { t } = useTranslation();
  const pending = useCatalogStore((s) => s.pendingConflict);
  const resolveConflict = useCatalogStore((s) => s.resolveConflict);
  const dialogRef = useRef<HTMLDivElement>(null);

  useModalDialog({
    dialogRef,
    onDismiss: () => void resolveConflict("cancel"),
    canDismiss: pending !== null,
    enabled: pending !== null,
  });

  if (!pending) {
    return null;
  }

  return (
    <div className="fixed inset-0 z-[80] flex items-center justify-center p-4">
      <DialogBackdrop
        ariaLabel={t("common.close")}
        onDismiss={() => void resolveConflict("cancel")}
        className="absolute inset-0 bg-black/60"
      />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="library-decision-title"
        tabIndex={-1}
        className="app-panel-surface relative w-full max-w-lg rounded-xl border border-[var(--color-border)] bg-[color-mix(in_srgb,var(--color-sidebar)_96%,transparent)] p-6"
      >
        <h3
          id="library-decision-title"
          className="text-[16px] font-semibold text-[var(--color-text)]"
        >
          {t("library.decision.title")}
        </h3>
        <p className="mt-1 text-[13px] text-[var(--color-text-dim)]">
          {t("library.decision.importConflict")}
        </p>
        <div className="mt-4 grid grid-cols-2 gap-3">
          <div>
            <div className="text-[12px] font-medium">
              {t("library.decision.librarySong")}
            </div>
            <MetaList meta={pending.library} />
          </div>
          <div>
            <div className="text-[12px] font-medium">
              {t("library.decision.incoming")}
            </div>
            <MetaList meta={pending.incoming} />
          </div>
        </div>
        <div className="mt-5 flex flex-wrap justify-end gap-2">
          <button
            type="button"
            onClick={() => void resolveConflict("keep")}
            className="rounded-md border border-[var(--color-border-light)] px-4 py-2 text-[13px]"
          >
            {t("library.decision.keep")}
          </button>
          <button
            type="button"
            onClick={() => void resolveConflict("replace")}
            className="rounded-md border border-[var(--color-border-light)] px-4 py-2 text-[13px]"
          >
            {t("library.decision.replace")}
          </button>
          <button
            type="button"
            onClick={() => void resolveConflict("apply_replace")}
            className="rounded-md border border-[var(--color-border-light)] px-4 py-2 text-[13px]"
          >
            {t("library.decision.applyToRemaining")}
          </button>
        </div>
      </div>
    </div>
  );
}
