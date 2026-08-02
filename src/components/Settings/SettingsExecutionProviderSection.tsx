import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { useSettingsOverlay } from "./SettingsOverlay.context";
import type { ExecutionProvider } from "@/types/ipc";

interface ExecutionProviderOptionProps {
  selected: boolean;
  compatible: boolean;
  disabled: boolean;
  title: ReactNode;
  description: ReactNode;
  incompatibleLabel: string;
  onClick: () => void;
}

function ExecutionProviderOption({
  selected,
  compatible,
  disabled,
  title,
  description,
  incompatibleLabel,
  onClick,
}: ExecutionProviderOptionProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-pressed={selected}
      aria-label={
        compatible ? String(title) : `${String(title)} — ${incompatibleLabel}`
      }
      data-incompatible={!compatible ? "true" : undefined}
      className={`flex-1 rounded-md border px-3 py-2 text-[13px] transition-colors ${
        selected
          ? "border-[var(--color-accent)] bg-[var(--color-accent)]/15 text-[var(--color-text)]"
          : "border-[var(--color-border-light)] bg-[var(--color-surface)] text-[var(--color-text)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text)]"
      } ${
        !compatible
          ? "border-[var(--color-destructive)]/70 text-[var(--color-destructive)]"
          : ""
      } disabled:opacity-50`}
    >
      <div className="flex items-center justify-center gap-1 font-medium">
        <span>{title}</span>
        {!compatible && (
          <AlertTriangle aria-hidden="true" className="h-3.5 w-3.5 shrink-0" />
        )}
      </div>
      <div className="mt-0.5 text-[11px] opacity-70">{description}</div>
      {!compatible && (
        <div className="mt-1 text-[10px] font-medium">{incompatibleLabel}</div>
      )}
    </button>
  );
}

function useEpLabels() {
  const { t } = useTranslation();
  return {
    cpu: {
      title: t("settings.executionProvider.cpu"),
      description: t("settings.executionProvider.cpuDescription"),
    },
    xnnpack: {
      title: t("settings.executionProvider.xnnpack"),
      description: t("settings.executionProvider.xnnpackDescription"),
    },
    coreml: {
      title: t("settings.executionProvider.coreml"),
      description: t("settings.executionProvider.coremlDescription"),
    },
    directml: {
      title: t("settings.executionProvider.directml"),
      description: t("settings.executionProvider.directmlDescription"),
    },
  } satisfies Record<ExecutionProvider, { title: string; description: string }>;
}

export function SettingsExecutionProviderSection() {
  const { t } = useTranslation();
  const { state, meta, actions } = useSettingsOverlay();
  const labels = useEpLabels();

  const selectProvider = (provider: ExecutionProvider) => {
    void actions.setExecutionProvider(provider);
  };
  const selectedProviderIsCompatible =
    state.compatibleExecutionProviders.includes(state.executionProvider);

  return (
    <SettingsSectionCard
      title={t("settings.executionProvider.label")}
      description={t("settings.executionProvider.description")}
    >
      {!selectedProviderIsCompatible && (
        <div
          role="alert"
          aria-live="assertive"
          className="mb-2 flex items-start gap-2 rounded-md border border-[var(--color-destructive)]/50 bg-[var(--color-destructive)]/8 px-3 py-2 text-[12px] text-[var(--color-destructive)]"
        >
          <AlertTriangle
            aria-hidden="true"
            className="mt-0.5 h-4 w-4 shrink-0"
          />
          <span>{t("settings.executionProvider.incompatibleWarning")}</span>
        </div>
      )}
      <div className="flex gap-2">
        {state.availableExecutionProviders.map((provider) => (
          <ExecutionProviderOption
            key={provider}
            selected={state.executionProvider === provider}
            compatible={state.compatibleExecutionProviders.includes(provider)}
            disabled={meta.isInitializing}
            title={labels[provider].title}
            description={labels[provider].description}
            incompatibleLabel={t("settings.executionProvider.incompatible")}
            onClick={() => selectProvider(provider)}
          />
        ))}
      </div>
      <p className="text-[11px] text-[var(--color-text-dim)]">
        {t("settings.executionProvider.note")}
      </p>
    </SettingsSectionCard>
  );
}
