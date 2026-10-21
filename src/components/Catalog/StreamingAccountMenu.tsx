import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { firstGrapheme } from "@/components/Catalog/streaming-utils";
import { useCatalogStore } from "@/stores/catalog-store";
import { ConfirmationDialog } from "@/components/Settings/ConfirmationDialog";

const focusRing =
  "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]";

export function StreamingAccountMenu({ name }: { name: string }) {
  const { t } = useTranslation();
  const signOut = useCatalogStore((s) => s.signOut);
  const source = useCatalogStore((s) =>
    s.sources.find((item) => item.source_id === s.activeSourceId),
  );
  const [confirmStop, setConfirmStop] = useState(false);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const itemRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    itemRef.current?.focus();
    const onPointerDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        setOpen(false);
        triggerRef.current?.focus();
      } else if (event.key === "Tab") {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative">
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        aria-label={t("catalog.streaming.accountMenu")}
        title={name}
        className={`flex size-9 items-center justify-center rounded-full bg-[var(--color-surface-muted)] text-[13px] font-semibold text-[var(--color-text)] hover:bg-[var(--color-ghost-hover)] ${focusRing}`}
      >
        <span aria-hidden="true">{firstGrapheme(name)}</span>
      </button>
      {open ? (
        <div
          role="menu"
          aria-label={t("catalog.streaming.accountMenu")}
          className="absolute right-0 top-full z-20 mt-1 min-w-[160px] rounded-lg border border-[var(--color-border-light)] bg-[var(--color-sidebar)] p-1 shadow-[var(--shadow-popover)]"
        >
          <button
            ref={itemRef}
            type="button"
            role="menuitem"
            onClick={() => {
              setOpen(false);
              if (source?.active_work.length) setConfirmStop(true);
              else void signOut();
            }}
            className={`w-full rounded-md px-3 py-2 text-left text-[13px] text-[var(--color-text)] hover:bg-[var(--sidebar-row-overlay-bg)] ${focusRing}`}
          >
            {t("catalog.streaming.signOut")}
          </button>
        </div>
      ) : null}
      {confirmStop && source ? (
        <ConfirmationDialog
          title={t("catalog.streaming.signOut")}
          message={t("settings.streaming.stopWorkBody")}
          detail={source.active_work
            .map((work) => t(`settings.streaming.work.${work.kind}`))
            .join(", ")}
          confirmLabel={t("settings.streaming.stopWork")}
          onCancel={() => setConfirmStop(false)}
          onConfirm={() => {
            setConfirmStop(false);
            void signOut(source.source_id, true);
          }}
        />
      ) : null}
    </div>
  );
}
