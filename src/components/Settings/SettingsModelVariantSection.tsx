import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettings } from "./SettingsController.context";
import { formatBytes } from "@/lib/format";
import { runtimeFailureStatusKey } from "@/lib/runtime-failure-copy";
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
  const { view, preferences, maintenance } = useSettings();
  const models = view.models;

  const runtimeState = view.runtime.status?.state;
  const runtimeReady =
    runtimeState === "ready" ||
    runtimeState === "update_available" ||
    runtimeState === "downloading_candidate" ||
    runtimeState === "candidate_ready_restart_required" ||
    runtimeState === "activation_failed_previous_restored";

  const modelStatusLabel = (variant: ModelVariant) => {
    if (!runtimeReady) {
      return t("settings.modelVariant.runtimeRequired");
    }

    if (models.downloading === variant) {
      return t("settings.modelVariant.downloading");
    }

    if (models.statusesError != null) {
      return t("settings.modelVariant.statusUnavailable");
    }

    const status = models.statuses[variant];

    if (status?.legacy_install_present && !status.downloaded) {
      return `${t("settings.modelVariant.legacyOnDisk")}${
        status.file_size_bytes
          ? ` (${formatBytes(status.file_size_bytes)})`
          : ""
      }`;
    }

    if (status?.downloaded) {
      const size = status.file_size_bytes
        ? ` (${formatBytes(status.file_size_bytes)})`
        : "";
      const version = status.installed_version
        ? ` · ${status.installed_version}`
        : "";
      return `${t("settings.modelVariant.downloaded")}${size}${version}`;
    }

    return t("settings.modelVariant.notDownloaded");
  };

  const controlsDisabled =
    view.isInitializing || models.downloading !== null || !runtimeReady;

  const runtimeNotice = runtimeReady
    ? null
    : runtimeState === "downloading"
      ? t("settings.runtime.downloading")
      : runtimeState === "installing"
        ? t("settings.runtime.banner.installingRuntime")
        : runtimeState === "probing"
          ? t("settings.runtime.banner.checkingCompatibility")
          : runtimeState === "activating"
            ? t("settings.runtime.banner.activatingRuntime")
            : runtimeState === "corrupt"
              ? t("settings.runtime.corrupt")
              : runtimeState === "failed"
                ? t(runtimeFailureStatusKey(view.runtime.status?.failure_phase))
                : t("settings.modelVariant.runtimeRequired");

  const update = models.update;
  const updatableModels =
    update?.status === "checked"
      ? update.models.filter((model) => model.state === "update_available")
      : [];

  return (
    <SettingsSectionCard
      title={t("settings.modelVariant.label")}
      description={t("settings.modelVariant.description")}
    >
      {runtimeNotice ? (
        <p className="mb-2 text-[12px] text-[var(--color-text-dim)]">
          {runtimeNotice}
        </p>
      ) : null}

      <div className="flex gap-2">
        <ModelVariantOption
          selected={view.preferences.modelVariant === "htdemucs"}
          disabled={controlsDisabled}
          title={t("settings.modelVariant.htdemucs")}
          description={t("settings.modelVariant.htdemucsDescription")}
          status={modelStatusLabel("htdemucs")}
          onClick={() => void preferences.selectModelVariant("htdemucs")}
        />
        <ModelVariantOption
          selected={view.preferences.modelVariant === "htdemucs_ft"}
          disabled={controlsDisabled}
          title={t("settings.modelVariant.htdemucsFt")}
          description={t("settings.modelVariant.htdemucsFtDescription")}
          status={modelStatusLabel("htdemucs_ft")}
          onClick={() => void preferences.selectModelVariant("htdemucs_ft")}
        />
      </div>

      {models.statusesError != null ? (
        <p className="mt-2 text-[11px] text-[var(--color-danger,#e5484d)] opacity-90">
          {t("settings.modelVariant.statusReadFailed")}
          {` ${models.statusesError}`}
        </p>
      ) : null}

      <div className="mt-3 flex flex-col gap-2 border-t border-[var(--color-border-light)] pt-3">
        <div className="flex items-center gap-2">
          <button
            onClick={() => void maintenance.checkModelUpdates()}
            disabled={
              view.isInitializing ||
              models.downloading !== null ||
              update?.status === "checking"
            }
            className="rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50"
          >
            {update?.status === "checking"
              ? t("settings.modelUpdate.checking")
              : t("settings.modelUpdate.checkButton")}
          </button>
          {update?.status === "checked" && updatableModels.length === 0 ? (
            <span className="text-[11px] text-[var(--color-text-dim)]">
              {t("settings.modelUpdate.upToDate")}
            </span>
          ) : null}
        </div>

        {update?.status === "failed" ? (
          <p className="text-[11px] text-[var(--color-danger,#e5484d)] opacity-90">
            {t("settings.modelUpdate.checkFailed")}
            {update.error ? ` ${update.error}` : ""}
          </p>
        ) : null}

        {updatableModels.map((model) => (
          <div
            key={model.variant}
            className="flex items-center justify-between gap-2 rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2"
          >
            <div className="flex flex-col">
              <span className="text-[12px] text-[var(--color-text)]">
                {t("settings.modelUpdate.updateAvailable", {
                  variant:
                    model.variant === "htdemucs"
                      ? t("settings.modelVariant.htdemucs")
                      : t("settings.modelVariant.htdemucsFt"),
                  version: model.available_version,
                })}
              </span>
              <span className="text-[10px] text-[var(--color-text-dim)]">
                {model.installed_version
                  ? `${model.installed_version} → ${model.available_version}`
                  : model.available_version}
                {` · ${formatBytes(model.available_bytes)}`}
              </span>
            </div>
            <button
              onClick={() =>
                void maintenance.downloadModel(model.variant as ModelVariant)
              }
              disabled={models.downloading !== null}
              className="rounded-md bg-[var(--color-control-primary)] px-3 py-1.5 text-[12px] text-[var(--color-control-primary-foreground)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-control-primary)_88%,white)] disabled:opacity-50"
            >
              {models.downloading === model.variant
                ? t("settings.modelVariant.downloading")
                : t("settings.modelUpdate.updateButton")}
            </button>
          </div>
        ))}
      </div>
    </SettingsSectionCard>
  );
}
