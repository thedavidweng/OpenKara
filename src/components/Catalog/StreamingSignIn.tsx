import { useState } from "react";
import { QRCodeSVG } from "qrcode.react";
import { useTranslation } from "react-i18next";
import { useCatalogStore } from "@/stores/catalog-store";

export function StreamingSignIn({
  compact = false,
  sourceId,
}: {
  compact?: boolean;
  sourceId?: string;
}) {
  const { t } = useTranslation();
  const id = useCatalogStore((state) => sourceId ?? state.activeSourceId ?? "");
  const source = useCatalogStore((state) => state.sourceStates[id]);
  const open = useCatalogStore((state) => state.openSignIn);
  const cancel = useCatalogStore((state) => state.cancelSignIn);
  const [busy, setBusy] = useState(false);
  const challenge = source?.challenge;
  const waiting = busy || source?.authStatus === "loading";
  const start = async () => {
    setBusy(true);
    try {
      await open(id);
    } finally {
      setBusy(false);
    }
  };
  if (source?.session?.signed_in && !source.session.expired)
    return (
      <p role="status">
        {source.session.display_name ?? t("catalog.streaming.signedIn")}
      </p>
    );
  return (
    <section
      aria-label={t("catalog.streaming.connectTitle")}
      aria-busy={waiting}
      className={
        compact
          ? "text-sm"
          : "mx-auto flex max-w-md flex-col items-center gap-3 py-8 text-center"
      }
    >
      {!compact ? (
        <h2 className="font-semibold">{t("catalog.streaming.connectTitle")}</h2>
      ) : null}
      {source?.session?.expired ? (
        <p role="status">{t("catalog.streaming.sessionExpired")}</p>
      ) : null}
      {source?.errors?.auth ? (
        <p role="alert">
          {t(
            source.errors?.auth === "expired"
              ? "catalog.streaming.sessionExpired"
              : "catalog.streaming.signInFailed",
            { message: source.errors?.auth },
          )}
        </p>
      ) : null}
      {challenge ? (
        <div role="status" className="flex flex-col items-center gap-3">
          {challenge.presentation.kind === "qr" ? (
            <>
              <QRCodeSVG
                value={challenge.presentation.content}
                size={192}
                level="M"
                title={t("catalog.streaming.scanCode")}
              />
              <p className="whitespace-pre-wrap break-words">
                {challenge.presentation.text}
              </p>
            </>
          ) : challenge.presentation.kind === "text" ? (
            <p className="whitespace-pre-wrap break-words">
              {challenge.presentation.text}
            </p>
          ) : (
            <p>{t("catalog.streaming.connectHintWindow")}</p>
          )}
          <button
            type="button"
            onClick={() => void cancel(id)}
            className="min-h-9 rounded-full border px-4"
          >
            {t("common.cancel")}
          </button>
        </div>
      ) : (
        <button
          type="button"
          disabled={waiting}
          onClick={() => void start()}
          className="min-h-9 rounded-full border border-[var(--color-border)] px-4"
        >
          {t("catalog.streaming.connect")}
        </button>
      )}
    </section>
  );
}
