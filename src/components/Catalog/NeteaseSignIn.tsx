import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useCatalogStore } from "@/stores/catalog-store";

export type NeteaseSignInMode = "qr" | "phone" | "email";

const inputClassName =
  "w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px] text-[var(--color-text)]";

const buttonClassName =
  "rounded-md border border-[var(--color-border-light)] px-3 py-1.5 text-[12px] text-[var(--color-text)]";

export function NeteaseSignIn() {
  const { t } = useTranslation();
  const session = useCatalogStore((s) => s.session);
  const qr = useCatalogStore((s) => s.qr);
  const qrStatus = useCatalogStore((s) => s.qrStatus);
  const startQr = useCatalogStore((s) => s.startQr);
  const pollQr = useCatalogStore((s) => s.pollQr);
  const signInPassword = useCatalogStore((s) => s.signInPassword);

  const [mode, setMode] = useState<NeteaseSignInMode>("qr");
  const [countryCode, setCountryCode] = useState("+86");
  const [phone, setPhone] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (mode !== "qr") {
      return;
    }
    if (!qr) {
      void startQr();
    }
  }, [mode, qr, startQr]);

  useEffect(() => {
    if (mode !== "qr" || !qr || qrStatus === "expired") {
      return;
    }
    const timer = window.setInterval(() => {
      void pollQr();
    }, 1500);
    return () => window.clearInterval(timer);
  }, [mode, qr, qrStatus, pollQr]);

  const switchMode = (next: NeteaseSignInMode) => {
    setMode(next);
    setPassword("");
    if (next === "qr" && !qr) {
      void startQr();
    }
  };

  const submitPassword = async (method: "phone" | "email") => {
    if (submitting) {
      return;
    }
    setSubmitting(true);
    try {
      if (method === "phone") {
        await signInPassword(
          "phone",
          phone.replace(/\s/g, ""),
          password,
          countryCode.replace(/[+\s]/g, ""),
        );
      } else {
        await signInPassword("email", email.replace(/\s/g, ""), password);
      }
    } finally {
      setPassword("");
      setSubmitting(false);
    }
  };

  const qrStatusCopy =
    qrStatus === "scanned"
      ? t("catalog.netease.scanned")
      : qrStatus === "expired"
        ? t("catalog.netease.qrExpired")
        : t("catalog.netease.qrHint");

  return (
    <div
      className="flex flex-col items-stretch gap-3 px-2"
      data-testid="netease-signin"
      data-mode={mode}
    >
      {session?.expired ? (
        <p
          className="text-[12px] text-[var(--color-text-dim)]"
          role="status"
          data-testid="netease-session-expired"
        >
          {t("catalog.netease.sessionExpired")}
        </p>
      ) : null}

      {mode === "qr" ? (
        <div className="flex flex-col items-center gap-2">
          {qr ? (
            <div
              className="flex h-[188px] w-[188px] items-center justify-center rounded-lg border border-[var(--color-border-light)] bg-[var(--color-surface)] p-2 text-[var(--color-text)]"
              role="img"
              aria-label={t("catalog.netease.qr")}
              data-testid="netease-qr"
              data-login-url={qr.login_url}
              dangerouslySetInnerHTML={{ __html: qr.qr_svg }}
            />
          ) : (
            <div
              className="flex h-[188px] w-[188px] items-center justify-center rounded-lg border border-dashed border-[var(--color-border-light)] text-[12px] text-[var(--color-text-dim)]"
              data-testid="netease-qr-loading"
            >
              {t("common.loading")}
            </div>
          )}
          <p
            className="text-center text-[12px] text-[var(--color-text-dim)]"
            role="status"
            data-testid="netease-qr-status"
            data-status={qrStatus ?? "waiting"}
          >
            {qrStatusCopy}
          </p>
          {qrStatus === "expired" ? (
            <button
              type="button"
              className={buttonClassName}
              onClick={() => void startQr()}
            >
              {t("catalog.netease.refreshQr")}
            </button>
          ) : null}
        </div>
      ) : null}

      {mode === "phone" ? (
        <form
          className="space-y-2"
          data-testid="netease-phone-form"
          onSubmit={(event) => {
            event.preventDefault();
            void submitPassword("phone");
          }}
        >
          <div className="flex gap-2">
            <label className="sr-only" htmlFor="netease-country-code">
              {t("catalog.netease.countryCode")}
            </label>
            <input
              id="netease-country-code"
              value={countryCode}
              onChange={(event) => setCountryCode(event.target.value)}
              inputMode="tel"
              autoComplete="tel-country-code"
              className={`${inputClassName} w-[72px] shrink-0`}
            />
            <label className="sr-only" htmlFor="netease-phone-number">
              {t("catalog.netease.phoneNumber")}
            </label>
            <input
              id="netease-phone-number"
              value={phone}
              onChange={(event) => setPhone(event.target.value)}
              inputMode="tel"
              autoComplete="tel-national"
              placeholder={t("catalog.netease.phoneNumber")}
              className={inputClassName}
            />
          </div>
          <label className="sr-only" htmlFor="netease-phone-password">
            {t("catalog.netease.password")}
          </label>
          <input
            id="netease-phone-password"
            type="password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            autoComplete="current-password"
            placeholder={t("catalog.netease.password")}
            className={inputClassName}
          />
          <button
            type="submit"
            className={buttonClassName}
            disabled={submitting}
          >
            {t("catalog.netease.signIn")}
          </button>
        </form>
      ) : null}

      {mode === "email" ? (
        <form
          className="space-y-2"
          data-testid="netease-email-form"
          onSubmit={(event) => {
            event.preventDefault();
            void submitPassword("email");
          }}
        >
          <label className="sr-only" htmlFor="netease-email">
            {t("catalog.netease.email")}
          </label>
          <input
            id="netease-email"
            type="email"
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            autoComplete="username"
            placeholder={t("catalog.netease.email")}
            className={inputClassName}
          />
          <label className="sr-only" htmlFor="netease-email-password">
            {t("catalog.netease.password")}
          </label>
          <input
            id="netease-email-password"
            type="password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            autoComplete="current-password"
            placeholder={t("catalog.netease.password")}
            className={inputClassName}
          />
          <button
            type="submit"
            className={buttonClassName}
            disabled={submitting}
          >
            {t("catalog.netease.signIn")}
          </button>
        </form>
      ) : null}

      {mode !== "qr" ? (
        <p className="text-[11px] text-[var(--color-text-dim)]">
          {t("catalog.netease.passwordNotice")}
        </p>
      ) : null}

      <div className="flex flex-wrap items-center justify-center gap-x-2 gap-y-1 text-[12px]">
        {mode !== "email" ? (
          <button
            type="button"
            className="rounded-md px-2 py-1.5 text-[var(--color-accent)]"
            data-testid="netease-use-email"
            onClick={() => switchMode("email")}
          >
            {t("catalog.netease.loginWithEmail")}
          </button>
        ) : null}
        {mode !== "phone" ? (
          <button
            type="button"
            className="rounded-md px-2 py-1.5 text-[var(--color-accent)]"
            data-testid="netease-use-phone"
            onClick={() => switchMode("phone")}
          >
            {t("catalog.netease.loginWithPhone")}
          </button>
        ) : null}
        {mode !== "qr" ? (
          <button
            type="button"
            className="rounded-md px-2 py-1.5 text-[var(--color-accent)]"
            data-testid="netease-use-qr"
            onClick={() => switchMode("qr")}
          >
            {t("catalog.netease.loginWithQr")}
          </button>
        ) : null}
      </div>
    </div>
  );
}
