import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { copyDebugInfo } from "@/lib/debug-info";
import { notifyError } from "@/lib/errors";
import { getDebugInfo } from "@/lib/tauri";
import type { DebugInfo } from "@/types/ipc";
import { SettingsSectionCard } from "./SettingsSectionCard";

const COPIED_RESET_MS = 2000;

function AboutRow({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt className="text-[var(--color-text-dim)]">{label}</dt>
      <dd className="break-words text-[var(--color-text)]">{value}</dd>
    </>
  );
}

/**
 * Cross-platform About panel: shows the app version + build SHA, catalog
 * generation, model/runtime status, execution provider, and the log-file
 * path, plus a "Copy debug info" button. This is the version/diagnostic
 * surface for Windows and Linux (which have no native menu) and complements
 * the macOS Help menu — both copy paths share {@link copyDebugInfo}.
 */
export function SettingsAboutSection() {
  const { t } = useTranslation();
  const [info, setInfo] = useState<DebugInfo | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let cancelled = false;
    getDebugInfo()
      .then((value) => {
        if (!cancelled) {
          setInfo(value);
        }
      })
      .catch(() => {
        // About is display-only; a failed fetch just leaves placeholders. The
        // copy button still fetches fresh on demand.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleCopy = async () => {
    try {
      await copyDebugInfo();
      setCopied(true);
      window.setTimeout(() => setCopied(false), COPIED_RESET_MS);
    } catch (error) {
      notifyError(error);
    }
  };

  const placeholder = "—";
  const version = info
    ? `${info.app_version} (${t("settings.about.build")} ${info.build_sha})`
    : placeholder;
  const system = info ? `${info.os} · ${info.arch}` : placeholder;
  const catalog = info
    ? `${info.catalog_generation} · ${info.catalog_release_id}`
    : placeholder;
  const model = info
    ? `${info.model_variant} · ${info.model_state}`
    : placeholder;
  const runtime = info
    ? `${info.runtime_state} · ${info.runtime_version}`
    : placeholder;
  const executionProvider = info ? info.execution_provider : placeholder;
  const modelPath = info ? info.model_path : placeholder;
  const logFile = info ? info.log_file : placeholder;

  return (
    <SettingsSectionCard
      title={t("settings.about.label")}
      description={t("settings.about.description")}
    >
      <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-[12px]">
        <AboutRow label={t("settings.about.version")} value={version} />
        <AboutRow label={t("settings.about.system")} value={system} />
        <AboutRow label={t("settings.about.catalog")} value={catalog} />
        <AboutRow label={t("settings.about.model")} value={model} />
        <AboutRow label={t("settings.about.modelPath")} value={modelPath} />
        <AboutRow label={t("settings.about.runtime")} value={runtime} />
        <AboutRow
          label={t("settings.about.executionProvider")}
          value={executionProvider}
        />
        <AboutRow label={t("settings.about.logFile")} value={logFile} />
      </dl>

      <p className="text-[11px] text-[var(--color-text-dim)]">
        {t("settings.about.reportHint")}
      </p>

      <button
        type="button"
        onClick={() => void handleCopy()}
        className="self-start rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)]"
      >
        {copied
          ? t("settings.about.copied")
          : t("settings.about.copyDebugInfo")}
      </button>
    </SettingsSectionCard>
  );
}
