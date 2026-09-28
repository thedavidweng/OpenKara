import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useCatalogStore } from "@/stores/catalog-store";

export function NeteaseSignIn() {
  const { t } = useTranslation();
  const session = useCatalogStore((s) => s.session);
  const openSignIn = useCatalogStore((s) => s.openSignIn);
  const [busy, setBusy] = useState(false);
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
    } finally {
      setBusy(false);
    }
  };
  const buttonClass =
    "min-h-9 rounded-md border border-[var(--color-border-light)] px-3 text-[13px] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)] disabled:opacity-50";
  return (
    <section
      data-testid="netease-signin"
      className="space-y-3 px-2"
      aria-busy={busy}
    >
      {session?.expired && (
        <p role="status" data-testid="netease-session-expired">
          {t("catalog.netease.sessionExpired")}
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className={buttonClass}
          disabled={busy}
          data-testid="netease-connect"
          onClick={() => void run(openSignIn)}
        >
          {t("catalog.netease.connect")}
        </button>
      </div>
    </section>
  );
}
