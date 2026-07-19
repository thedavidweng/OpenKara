import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettingsOverlay } from "./SettingsOverlay.context";
import { formatBytes } from "./SettingsOverlay.utils";
import type { ModelVariant } from "@/types/ipc";

interface ModelVariantOptionProps {
  selected: boolean;
  disabled: boolean;
  title: ReactNode;
  description: ReactNode;
  status: ReactNode;
  onClick: () => void;
}

function ModelVariantOption({
  selected,
  disabled,
  title,
  description,
  status,
  onClick,
}: ModelVariantOptionProps) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`flex-1 rounded-md border px-3 py-2 text-[13px] transition-colors ${
        selected
          ? "border-[var(--color-accent)] bg-[var(--color-accent)]/15 text-[var(--color-text)]"
          : "border-[var(--color-border-light)] bg-[var(--color-surface)] text-[var(--color-text)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text)]"
      } disabled:opacity-50`}
    >
      <div className="font-medium">{title}</div>
      <div className="mt-0.5 text-[11px] opacity-70">{description}</div>
      <div className="mt-1 text-[10px] opacity-50">{status}</div>
    </button>
  );
}

export function SettingsModelVariantSection() {
  const { t } = useTranslation();
  const { state, meta, actions } = useSettingsOverlay();

  const runtimeReady = state.runtimeStatus?.state === "ready";

  const modelStatusLabel = (variant: ModelVariant) => {
    if (!runtimeReady) {
      return t("settings.modelVariant.runtimeRequired");
    }

    if (state.downloadingModel === variant) {
      return t("settings.modelVariant.downloading");
    }

    const status = state.modelStatuses[variant];

    if (status?.legacy_install_present && !status.downloaded) {
      return `${t("settings.modelVariant.legacyOnDisk")}${
        status.file_size ? ` (${formatBytes(status.file_size)})` : ""
      }`;
    }

    if (status?.downloaded) {
      return `${t("settings.modelVariant.downloaded")}${
        status.file_size ? ` (${formatBytes(status.file_size)})` : ""
      }`;
    }

    return t("settings.modelVariant.notDownloaded");
  };

  // B3: Model download controls are disabled when runtime is missing.
  const controlsDisabled =
    meta.isInitializing || state.downloadingModel !== null || !runtimeReady;

  // B3: Show runtime install CTA when runtime is missing.
  if (!runtimeReady && state.runtimeStatus?.state !== "downloading") {
    return (
      <SettingsSectionCard
        title={t("settings.modelVariant.label")}
        description={t("settings.modelVariant.description")}
      >
        <div className="flex flex-col gap-2">
          <p className="text-[12px] text-[var(--color-text-dim)]">
            {t("settings.runtime.installRequired")}
          </p>
          <button
            onClick={() => void actions.downloadRuntime()}
            className="self-start rounded-md bg-[var(--color-control-primary)] px-3 py-1.5 text-[12px] text-[var(--color-control-primary-foreground)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-control-primary)_88%,white)]"
          >
            {t("settings.runtime.installButton")}
          </button>
        </div>
      </SettingsSectionCard>
    );
  }

  if (state.runtimeStatus?.state === "downloading") {
    return (
      <SettingsSectionCard
        title={t("settings.modelVariant.label")}
        description={t("settings.modelVariant.description")}
      >
        <p className="text-[12px] text-[var(--color-text-dim)]">
          {t("settings.runtime.downloading")}
        </p>
      </SettingsSectionCard>
    );
  }

  return (
    <SettingsSectionCard
      title={t("settings.modelVariant.label")}
      description={t("settings.modelVariant.description")}
    >
      <div className="flex gap-2">
        <ModelVariantOption
          selected={state.modelVariant === "htdemucs"}
          disabled={controlsDisabled}
          title={t("settings.modelVariant.htdemucs")}
          description={t("settings.modelVariant.htdemucsDescription")}
          status={modelStatusLabel("htdemucs")}
          onClick={() => void actions.selectModelVariant("htdemucs")}
        />
        <ModelVariantOption
          selected={state.modelVariant === "htdemucs_ft"}
          disabled={controlsDisabled}
          title={t("settings.modelVariant.htdemucsFt")}
          description={t("settings.modelVariant.htdemucsFtDescription")}
          status={modelStatusLabel("htdemucs_ft")}
          onClick={() => void actions.selectModelVariant("htdemucs_ft")}
        />
      </div>
    </SettingsSectionCard>
  );
}
