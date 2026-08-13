import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { copyDebugInfo } from "@/lib/debug-info";
import { notifyError } from "@/lib/errors";
import { useBackend } from "@/lib/backend";
import type { DebugInfo } from "@/types/ipc";
import { SettingsSectionCard } from "./SettingsSectionCard";

const COPIED_RESET_MS = 2000;

function AboutRow({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt className="break-words text-[var(--color-text-dim)]">{label}</dt>
      <dd className="min-w-0 break-words text-[var(--color-text)]">{value}</dd>
    </>
  );
}

export function SettingsAboutSection() {
  const { settings } = useBackend();
  const { t } = useTranslation();
  const [info, setInfo] = useState<DebugInfo | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let cancelled = false;
    settings
      .getDebugInfo()
      .then((value) => {
        if (!cancelled) {
          setInfo(value);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [settings]);

  const handleCopy = async () => {
    try {
      await copyDebugInfo({
        fetchDebugInfo: settings.getDebugInfo,
        translate: t,
      });
      setCopied(true);
      window.setTimeout(() => setCopied(false), COPIED_RESET_MS);
    } catch (error) {
      notifyError(error);
    }
  };

  const emptyValue = "—";
  const version = info
    ? `${info.app_version} (${t("settings.about.build")} ${info.build_sha})`
    : emptyValue;
  const system = info ? `${info.os} · ${info.arch}` : emptyValue;
  const catalog = info
    ? `${info.catalog_generation} · ${info.catalog_release_id}`
    : emptyValue;
  const model = info
    ? `${info.model_variant} · ${info.model_state}`
    : emptyValue;
  const runtime = info
    ? `${info.runtime_state} · ${info.runtime_version}`
    : emptyValue;
  const executionProvider = info ? info.execution_provider : emptyValue;
  const modelPath = info ? info.model_path : emptyValue;
  const logFile = info ? info.log_file : emptyValue;

  return (
    <SettingsSectionCard
      title={t("settings.about.label")}
      description={t("settings.about.description")}
    >
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-[12px]">
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

      <p className="break-words text-[11px] text-[var(--color-text-dim)]">
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
