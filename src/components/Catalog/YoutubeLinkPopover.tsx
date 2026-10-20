import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { YoutubePasteLink } from "@/components/Catalog/YoutubePasteLink";
import { useModalDialog } from "@/hooks/use-modal-dialog";
import { useCatalogStore } from "@/stores/catalog-store";
import { useSettingsStore } from "@/stores/settings-store";

const TRIGGER_SELECTOR = "[data-youtube-link-trigger]";

export function YoutubeLinkPopover() {
  const open = useCatalogStore((s) => s.youtubeLinkOpen);
  const enabled = useSettingsStore((s) => s.youtubeSourceEnabled);
  if (!open || !enabled) return null;
  return <PopoverBody />;
}

function PopoverBody() {
  const { t } = useTranslation();
  const setOpen = useCatalogStore((s) => s.setYoutubeLinkOpen);
  const dialogRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [pos, setPos] = useState<{ left: number; bottom: number }>({
    left: 12,
    bottom: 12,
  });

  const close = useCallback(() => {
    setOpen(false);
    document.querySelector<HTMLElement>(TRIGGER_SELECTOR)?.focus();
  }, [setOpen]);

  useModalDialog({ dialogRef, initialFocusRef: inputRef, onDismiss: close });

  useEffect(() => {
    const trigger = document.querySelector<HTMLElement>(TRIGGER_SELECTOR);
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    setPos({
      left: Math.max(8, rect.left),
      bottom: Math.max(8, window.innerHeight - rect.bottom),
    });
  }, []);

  useEffect(() => {
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (dialogRef.current?.contains(target)) return;
      if (document.querySelector(TRIGGER_SELECTOR)?.contains(target)) return;
      setOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [setOpen]);

  return (
    <div
      ref={dialogRef}
      role="dialog"
      aria-modal="false"
      aria-labelledby="youtube-popover-title"
      style={{ left: pos.left, bottom: pos.bottom }}
      className="fixed z-40 w-[320px] rounded-xl border border-[var(--color-border-light)] bg-[var(--color-sidebar)] p-4 shadow-[var(--shadow-popover)]"
    >
      <h2
        id="youtube-popover-title"
        className="text-[14px] font-semibold text-[var(--color-text)]"
      >
        {t("youtube.popoverTitle")}
      </h2>
      <p className="mb-3 mt-1 text-[12px] text-[var(--color-text-dim)]">
        {t("youtube.popoverHint")}
      </p>
      <YoutubePasteLink inputRef={inputRef} onDone={close} />
    </div>
  );
}
