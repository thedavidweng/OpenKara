import { useEffect, useRef, useState } from "react";
import { AlertCircle, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useBackend } from "@/lib/backend";
import { copyDebugInfo } from "@/lib/debug-info";
import { getErrorMessage, notifyError } from "@/lib/errors";
import { useCatalogStore } from "@/stores/catalog-store";

const COPIED_RESET_MS = 2000;
const focusRing =
  "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]";

export function StreamingSignInErrorBanner() {
  const { settings } = useBackend();
  const { t } = useTranslation();
  const error = useCatalogStore((s) => s.signInError);
  const clear = useCatalogStore((s) => s.clearSignInError);
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(
    () => () => {
      if (timer.current !== undefined) window.clearTimeout(timer.current);
    },
    [],
  );

  if (!error) return null;
  const message = t("catalog.streaming.signInFailed", {
    message: getErrorMessage(error),
  });

  const copy = async () => {
    try {
      await copyDebugInfo({
        fetchDebugInfo: settings.getDebugInfo,
        translate: t,
        error: { title: message, message: error.message },
      });
      setCopied(true);
      timer.current = window.setTimeout(
        () => setCopied(false),
        COPIED_RESET_MS,
      );
    } catch (copyError) {
      notifyError(copyError);
    }
  };

  return (
    <div
      role="alert"
      data-testid="streaming-sign-in-error"
      className="mx-5 mt-4 flex items-start gap-2.5 rounded-lg border border-[var(--color-destructive)] bg-[var(--color-surface-muted)] px-3 py-2.5"
    >
      <AlertCircle
        size={14}
        aria-hidden="true"
        className="mt-0.5 shrink-0 text-[var(--color-destructive)]"
      />
      <p className="min-w-0 flex-1 break-words text-[13px] text-[var(--color-text)]">
        {message}
      </p>
      <button
        type="button"
        onClick={() => void copy()}
        className={`shrink-0 rounded px-1 text-[12px] text-[var(--color-accent)] hover:underline ${focusRing}`}
      >
        {copied
          ? t("settings.about.copied")
          : t("settings.about.copyDebugInfo")}
      </button>
      <button
        type="button"
        aria-label={t("common.close")}
        onClick={clear}
        className={`inline-flex size-6 shrink-0 items-center justify-center rounded text-[var(--color-text-dim)] hover:bg-[var(--sidebar-row-overlay-bg)] ${focusRing}`}
      >
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
}
