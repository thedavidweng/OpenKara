import { Download, Loader2, RotateCw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type Phase =
  | "hidden"
  | "available"
  | "downloading"
  | "installing"
  | "ready"
  | "failed";

/**
 * In-app updater banner (#255). Checks once on launch for a signed release
 * newer than the running build, and — only when one exists — surfaces a
 * dismissible strip to download, install, and relaunch.
 *
 * A karaoke session must never be interrupted by an updater error, so every
 * failure path is silent: a rejected `check()` (offline, dev build, or a
 * non-updatable install such as a Linux `.deb`/Flatpak, where the plugin either
 * errors or finds nothing) simply leaves the banner unrendered — no toast, no
 * modal. An install failure downgrades to an inline, dismissible message.
 */
export function UpdateBanner() {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>("hidden");
  const [version, setVersion] = useState<string | null>(null);
  const [percent, setPercent] = useState(0);
  const updateRef = useRef<Update | null>(null);

  useEffect(() => {
    let cancelled = false;
    check()
      .then((update) => {
        if (cancelled || !update) return;
        updateRef.current = update;
        setVersion(update.version);
        setPhase("available");
      })
      .catch(() => {
        // Silent by design: no network, dev build, or non-updatable install.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (phase === "hidden") return null;

  const dismiss = () => setPhase("hidden");

  const handleInstall = async () => {
    const update = updateRef.current;
    if (!update) return;
    setPhase("downloading");
    setPercent(0);
    let downloaded = 0;
    let total = 0;
    try {
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            total = event.data.contentLength ?? 0;
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            if (total > 0) {
              setPercent(Math.min(100, Math.round((downloaded / total) * 100)));
            }
            break;
          case "Finished":
            setPhase("installing");
            break;
        }
      });
      setPhase("ready");
    } catch {
      setPhase("failed");
    }
  };

  const containerClass =
    "animate-expand shrink-0 border-b border-[var(--color-border)] bg-[var(--color-sidebar)] px-4 py-3";
  const primaryButtonClass =
    "flex shrink-0 items-center gap-1.5 self-start rounded-md bg-[var(--color-control-primary)] px-3 py-1.5 text-[11px] text-[var(--color-control-primary-foreground)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-control-primary)_88%,white)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 sm:self-center";
  const dismissButtonClass =
    "shrink-0 rounded-md p-1 text-[var(--color-text-dim)] transition-colors hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50";

  if (phase === "downloading") {
    return (
      <div className={containerClass}>
        <div className="flex items-center gap-2 text-[12px] text-[var(--color-text)]">
          <Loader2 size={12} className="animate-spin" />
          {t("updater.downloading", { percent })}
        </div>
      </div>
    );
  }

  if (phase === "installing") {
    return (
      <div className={containerClass}>
        <div className="flex items-center gap-2 text-[12px] text-[var(--color-text)]">
          <Loader2 size={12} className="animate-spin" />
          {t("updater.installing")}
        </div>
      </div>
    );
  }

  if (phase === "ready") {
    return (
      <div className={containerClass}>
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <span className="text-[12px] text-[var(--color-text)]">
            {t("updater.restartToApply")}
          </span>
          <button
            type="button"
            onClick={() => {
              void relaunch();
            }}
            className={primaryButtonClass}
          >
            <RotateCw size={12} />
            {t("updater.restart")}
          </button>
        </div>
      </div>
    );
  }

  if (phase === "failed") {
    return (
      <div className={containerClass}>
        <div className="flex items-center justify-between gap-2">
          <span className="text-[12px] text-[var(--color-destructive)]">
            {t("updater.failed")}
          </span>
          <button
            type="button"
            onClick={dismiss}
            aria-label={t("common.close")}
            className={dismissButtonClass}
          >
            <X size={14} />
          </button>
        </div>
      </div>
    );
  }

  // phase === "available"
  return (
    <div className={containerClass}>
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <span className="text-[12px] text-[var(--color-text)]">
          {t("updater.available", { version: version ?? "" })}
        </span>
        <div className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            onClick={() => void handleInstall()}
            className={primaryButtonClass}
          >
            <Download size={12} />
            {t("updater.update")}
          </button>
          <button
            type="button"
            onClick={dismiss}
            aria-label={t("common.close")}
            className={dismissButtonClass}
          >
            <X size={14} />
          </button>
        </div>
      </div>
    </div>
  );
}
