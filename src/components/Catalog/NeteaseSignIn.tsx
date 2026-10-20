import { useState } from "react";
import { Music2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useCatalogStore } from "@/stores/catalog-store";

export function NeteaseSignIn() {
  const { t } = useTranslation();
  const session = useCatalogStore((s) => s.session);
  const openSignIn = useCatalogStore((s) => s.openSignIn);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    try {
      await openSignIn();
    } finally {
      setBusy(false);
    }
  };
  return (
    <section
      data-testid="netease-signin"
      aria-busy={busy}
      className="mx-auto flex w-[min(420px,calc(100%-2rem))] flex-col items-center gap-3 py-16 text-center"
    >
      <Music2
        size={28}
        aria-hidden="true"
        className="text-[var(--color-text-dim)]"
      />
      <h2 className="text-[17px] font-semibold text-[var(--color-text)]">
        {t("catalog.netease.connectTitle")}
      </h2>
      {session?.expired ? (
        <p
          role="status"
          data-testid="netease-session-expired"
          className="text-[13px] text-[var(--color-warning)]"
        >
          {t("catalog.netease.sessionExpired")}
        </p>
      ) : null}
      <p className="text-[13px] leading-relaxed text-[var(--color-text-dim)]">
        {t("catalog.netease.connectBody")}
      </p>
      <button
        type="button"
        disabled={busy}
        data-testid="netease-connect"
        onClick={() => void run()}
        className="mt-1 min-h-9 rounded-full bg-[var(--color-control-primary)] px-6 text-[13px] font-medium text-[var(--color-control-primary-foreground)] hover:opacity-90 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)] disabled:opacity-50"
      >
        {t("catalog.netease.connect")}
      </button>
      <p className="text-[12px] text-[var(--color-text-dim)]">
        {t("catalog.netease.connectHintWindow")}
      </p>
      <p className="text-[12px] text-[var(--color-text-dim)]">
        {t("catalog.netease.connectHintRights")}
      </p>
    </section>
  );
}
