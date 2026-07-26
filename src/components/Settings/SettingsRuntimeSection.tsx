import { useTranslation } from "react-i18next";
import { formatBytes } from "@/lib/format";
import type { UpdatePolicy } from "@/types/ipc";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettingsOverlay } from "./SettingsOverlay.context";

// Literal i18n keys keep the strictly-typed `t` happy (no dynamic concatenation).
const UPDATE_POLICY_OPTIONS = [
  {
    value: "manual",
    labelKey: "settings.runtime.updatePolicy.manual",
    descriptionKey: "settings.runtime.updatePolicy.manualDescription",
  },
  {
    value: "notify",
    labelKey: "settings.runtime.updatePolicy.notify",
    descriptionKey: "settings.runtime.updatePolicy.notifyDescription",
  },
  {
    value: "auto_download",
    labelKey: "settings.runtime.updatePolicy.autoDownload",
    descriptionKey: "settings.runtime.updatePolicy.autoDownloadDescription",
  },
] as const satisfies ReadonlyArray<{
  value: UpdatePolicy;
  labelKey: string;
  descriptionKey: string;
}>;

export function SettingsRuntimeSection() {
  const { t } = useTranslation();
  const { state, meta, actions } = useSettingsOverlay();

  const runtime = state.runtimeStatus;
  const update = state.runtimeUpdate;
  const runtimeState = runtime?.state;

  const isMissing = !runtime || runtimeState === "missing";
  const restartRequired =
    runtime?.restart_required === true ||
    runtimeState === "candidate_ready_restart_required";

  const statusLine = (() => {
    switch (runtimeState) {
      case "candidate_ready_restart_required":
        return t("settings.runtime.candidateReadyRestartRequired", {
          version: runtime?.candidate_version ?? "",
        });
      case "activation_failed_previous_restored":
        return t("settings.runtime.activationFailedPreviousRestored");
      case "downloading":
        return t("settings.runtime.downloading");
      case "downloading_candidate":
        return t("settings.runtime.downloadingCandidate");
      case "corrupt":
        return t("settings.runtime.corrupt");
      case "failed":
        return t("settings.runtime.downloadFailed");
      case "missing":
        return t("settings.runtime.statusMissing");
      default:
        return t("settings.runtime.statusReady");
    }
  })();

  const versionLine =
    runtime && !isMissing
      ? `${t("settings.runtime.version", {
          version: runtime.version,
        })} · ${runtime.target_triple}`
      : null;

  const report = update?.status === "checked" ? update.report : null;
  // A staged candidate supersedes the update CTA: showing both a Restart
  // button and an enabled Update button would offer a redundant re-download
  // of the runtime that is already staged.
  const updateAvailable =
    (report?.state === "update_available" ||
      report?.state === "installed_without_identity") &&
    !restartRequired;
  const downloadingCandidate = runtimeState === "downloading_candidate";
  const isDownloading = runtimeState === "downloading";

  // The runtime normally installs itself the first time a song is separated.
  // This CTA is the manual escape hatch for the states that cannot recover on
  // their own, and it lives here — next to the runtime status and version —
  // rather than inside the AI Model card.
  const needsInstall =
    isMissing || runtimeState === "corrupt" || runtimeState === "failed";
  const installLabel =
    runtimeState === "corrupt" || runtimeState === "failed"
      ? t("settings.runtime.retryButton")
      : t("settings.runtime.installButton");
  const installHint =
    runtimeState === "corrupt"
      ? t("settings.runtime.corrupt")
      : runtimeState === "failed"
        ? t("settings.runtime.downloadFailed")
        : t("settings.runtime.installRequired");

  return (
    <SettingsSectionCard
      title={t("settings.runtime.label")}
      description={t("settings.runtime.description")}
    >
      <div className="flex flex-col gap-1">
        <p className="text-[12px] text-[var(--color-text)]">{statusLine}</p>
        {versionLine ? (
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {versionLine}
          </p>
        ) : null}
        {runtimeState === "activation_failed_previous_restored" &&
        runtime?.error ? (
          <p className="text-[11px] text-[var(--color-danger,#e5484d)] opacity-90">
            {runtime.error}
          </p>
        ) : null}
        {needsInstall ? (
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {installHint}
          </p>
        ) : null}
        {needsInstall && runtime?.error ? (
          <p className="text-[11px] text-[var(--color-danger,#e5484d)] opacity-90">
            {runtime.error}
          </p>
        ) : null}
      </div>

      {needsInstall ? (
        <button
          onClick={() => void actions.downloadRuntime()}
          disabled={meta.isInitializing || isDownloading}
          data-testid="runtime-install-button"
          className="self-start rounded-md bg-[var(--color-control-primary)] px-3 py-1.5 text-[12px] text-[var(--color-control-primary-foreground)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-control-primary)_88%,white)] disabled:opacity-50"
        >
          {isDownloading ? t("settings.runtime.downloading") : installLabel}
        </button>
      ) : null}

      {restartRequired ? (
        <button
          onClick={() => void actions.restartApp()}
          className="self-start rounded-md bg-[var(--color-control-primary)] px-3 py-1.5 text-[12px] text-[var(--color-control-primary-foreground)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-control-primary)_88%,white)]"
        >
          {t("settings.runtime.restartButton")}
        </button>
      ) : null}

      <div className="mt-3 flex flex-col gap-2 border-t border-[var(--color-border-light)] pt-3">
        <div className="flex items-center gap-2">
          <button
            onClick={() => void actions.checkRuntimeUpdates()}
            disabled={
              meta.isInitializing ||
              isMissing ||
              downloadingCandidate ||
              update?.status === "checking"
            }
            className="rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50"
          >
            {update?.status === "checking"
              ? t("settings.runtime.checking")
              : t("settings.runtime.checkButton")}
          </button>
          {/* Only a genuine up_to_date verdict may claim the runtime is
              current. Reporting "up to date" for not_installed is what made a
              failed install look like a healthy one. */}
          {report?.state === "up_to_date" && !updateAvailable ? (
            <span className="text-[11px] text-[var(--color-text-dim)]">
              {t("settings.runtime.upToDate")}
            </span>
          ) : null}
          {report?.state === "not_installed" ? (
            <span className="text-[11px] text-[var(--color-text-dim)]">
              {t("settings.runtime.statusMissing")}
            </span>
          ) : null}
        </div>

        {update?.status === "failed" ? (
          <p className="text-[11px] text-[var(--color-danger,#e5484d)] opacity-90">
            {t("settings.runtime.checkFailed")}
            {update.error ? ` ${update.error}` : ""}
          </p>
        ) : null}

        {downloadingCandidate ? (
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.runtime.downloadingCandidate")}
          </p>
        ) : null}

        {report && updateAvailable ? (
          <div className="flex items-center justify-between gap-2 rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2">
            <div className="flex flex-col">
              <span className="text-[12px] text-[var(--color-text)]">
                {t("settings.runtime.updateAvailable", {
                  version: report.available_version,
                })}
              </span>
              <span className="text-[10px] text-[var(--color-text-dim)]">
                {report.installed_version
                  ? `${report.installed_version} → ${report.available_version}`
                  : report.available_version}
                {` · ${formatBytes(report.available_bytes)}`}
              </span>
            </div>
            <button
              onClick={() => void actions.updateRuntime()}
              disabled={downloadingCandidate}
              className="rounded-md bg-[var(--color-control-primary)] px-3 py-1.5 text-[12px] text-[var(--color-control-primary-foreground)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-control-primary)_88%,white)] disabled:opacity-50"
            >
              {downloadingCandidate
                ? t("settings.runtime.downloadingCandidate")
                : t("settings.runtime.updateButton")}
            </button>
          </div>
        ) : null}
      </div>

      <fieldset className="mt-3 space-y-2 border-t border-[var(--color-border-light)] pt-3">
        <legend className="text-[12px] font-medium text-[var(--color-text-dim)]">
          {t("settings.runtime.updatePolicy.label")}
        </legend>
        <p className="text-[11px] text-[var(--color-text-dim)]">
          {t("settings.runtime.updatePolicy.description")}
        </p>
        <div className="flex flex-col gap-2">
          {UPDATE_POLICY_OPTIONS.map((option) => (
            <label
              key={option.value}
              className="flex items-start gap-2 text-[13px] text-[var(--color-text)]"
            >
              <input
                type="radio"
                name="runtime-update-policy"
                value={option.value}
                checked={state.updatePolicy === option.value}
                onChange={() => void actions.setUpdatePolicy(option.value)}
                disabled={meta.isInitializing}
                className="mt-0.5 accent-[var(--color-accent)]"
              />
              <span className="flex flex-col">
                <span>{t(option.labelKey)}</span>
                <span className="text-[11px] text-[var(--color-text-dim)]">
                  {t(option.descriptionKey)}
                </span>
              </span>
            </label>
          ))}
        </div>
      </fieldset>
    </SettingsSectionCard>
  );
}
