import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { notifyError } from "@/lib/errors";
import * as api from "@/lib/tauri";
import type { RemoteDiagnostics } from "@/types/ipc";

/**
 * Remote repository diagnostics panel (PR #8, issue #151).
 *
 * Shows the repository health for the active remote library: generation,
 * cleanliness state, conflict status, and recent operation outcomes. When
 * no remote library is active, the section is hidden.
 */
export function SettingsRemoteDiagnosticsSection() {
  const { t } = useTranslation();
  const [diagnostics, setDiagnostics] = useState<RemoteDiagnostics | null>(
    null,
  );

  const refresh = useCallback(async () => {
    try {
      const d = await api.getRemoteDiagnostics();
      setDiagnostics(d);
    } catch (err) {
      notifyError(err);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Hide the section when no remote library is active.
  if (!diagnostics || !diagnostics.has_active_remote) {
    return null;
  }

  const stateColor =
    diagnostics.local_state === "conflicted"
      ? "text-[var(--color-destructive)]"
      : diagnostics.local_state === "dirty"
        ? "text-[var(--color-accent)]"
        : "text-[var(--color-text-dim)]";

  return (
    <SettingsSectionCard
      title={t("settings.remoteDiagnostics.label", {
        defaultValue: "Remote Diagnostics",
      })}
      description={t("settings.remoteDiagnostics.description", {
        defaultValue:
          "Repository health, generation, and recent operation outcomes.",
      })}
    >
      <div className="space-y-3">
        <div className="space-y-1 text-[12px] text-[var(--color-text-dim)]">
          <div className="flex justify-between">
            <span>
              {t("settings.remoteDiagnostics.state", {
                defaultValue: "Repository state",
              })}
            </span>
            <span className={stateColor}>{diagnostics.local_state}</span>
          </div>
          <div className="flex justify-between">
            <span>
              {t("settings.remoteDiagnostics.generation", {
                defaultValue: "Committed generation",
              })}
            </span>
            <span className="text-[var(--color-text)]">
              {diagnostics.committed_generation}
            </span>
          </div>
          {diagnostics.repository_id && (
            <div className="flex justify-between">
              <span>
                {t("settings.remoteDiagnostics.repositoryId", {
                  defaultValue: "Repository ID",
                })}
              </span>
              <span className="text-[var(--color-text)] font-mono text-[10px]">
                {diagnostics.repository_id.slice(0, 8)}
              </span>
            </div>
          )}
          {diagnostics.last_error_code && (
            <div className="flex justify-between">
              <span>
                {t("settings.remoteDiagnostics.lastError", {
                  defaultValue: "Last error",
                })}
              </span>
              <span className="text-[var(--color-destructive)]">
                {diagnostics.last_error_code}
              </span>
            </div>
          )}
        </div>

        {diagnostics.recent_operations.length > 0 && (
          <div className="space-y-1">
            <p className="text-[11px] font-semibold text-[var(--color-text)]">
              {t("settings.remoteDiagnostics.recentOps", {
                defaultValue: "Recent operations",
              })}
            </p>
            <div className="max-h-40 space-y-1 overflow-y-auto">
              {diagnostics.recent_operations.map((op) => (
                <div
                  key={op.operation_id}
                  className="flex items-center justify-between text-[10px] text-[var(--color-text-dim)]"
                >
                  <span className="font-mono">{op.operation_kind}</span>
                  <span
                    className={
                      op.state === "failed" || op.state === "conflicted"
                        ? "text-[var(--color-destructive)]"
                        : op.state === "completed"
                          ? "text-[var(--color-text)]"
                          : "text-[var(--color-text-dim)]"
                    }
                  >
                    {op.state}
                  </span>
                  {op.error_code && (
                    <span className="text-[var(--color-destructive)]">
                      {op.error_code}
                    </span>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}

        <button
          type="button"
          onClick={() => void refresh()}
          className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-ghost-hover)]"
        >
          {t("settings.remoteDiagnostics.refresh", {
            defaultValue: "Refresh",
          })}
        </button>
      </div>
    </SettingsSectionCard>
  );
}
