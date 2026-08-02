import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { notifyError } from "@/lib/errors";
import * as api from "@/lib/tauri";
import type { RemoteDiagnostics } from "@/types/ipc";

// Remote repository diagnostics panel (PR #8, issue #151).
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

  const [resolving, setResolving] = useState(false);

  const resolveConflict = useCallback(
    async (resolution: api.RemoteConflictResolution) => {
      setResolving(true);
      try {
        await api.resolveRemoteConflict(resolution);
        await refresh();
      } catch (err) {
        notifyError(err);
      } finally {
        setResolving(false);
      }
    },
    [refresh],
  );

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
      title={t("settings.remoteDiagnostics.label")}
      description={t("settings.remoteDiagnostics.description")}
    >
      {diagnostics.local_state === "conflicted" && (
        <div className="space-y-2 rounded-lg border border-[var(--color-destructive)]/40 bg-[var(--color-destructive)]/8 p-3">
          <p className="text-[12px] text-[var(--color-text)]">
            {t("settings.remoteDiagnostics.conflictTitle")}
          </p>
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.remoteDiagnostics.conflictBody")}
          </p>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => void resolveConflict("keep_local")}
              disabled={resolving}
              className="rounded-md border border-[var(--color-border-light)] px-3 py-1.5 text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] disabled:opacity-60"
            >
              {t("settings.remoteDiagnostics.conflictKeepLocal")}
            </button>
            <button
              type="button"
              onClick={() => void resolveConflict("use_remote")}
              disabled={resolving}
              className="rounded-md border border-[var(--color-border-light)] px-3 py-1.5 text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] disabled:opacity-60"
            >
              {t("settings.remoteDiagnostics.conflictUseRemote")}
            </button>
          </div>
        </div>
      )}

      <div className="space-y-3">
        <div className="space-y-1 text-[12px] text-[var(--color-text-dim)]">
          <div className="flex justify-between">
            <span>{t("settings.remoteDiagnostics.state")}</span>
            <span className={stateColor}>{diagnostics.local_state}</span>
          </div>
          <div className="flex justify-between">
            <span>{t("settings.remoteDiagnostics.generation")}</span>
            <span className="text-[var(--color-text)]">
              {diagnostics.committed_generation}
            </span>
          </div>
          {diagnostics.repository_id && (
            <div className="flex justify-between">
              <span>{t("settings.remoteDiagnostics.repositoryId")}</span>
              <span className="text-[var(--color-text)] font-mono text-[10px]">
                {diagnostics.repository_id.slice(0, 8)}
              </span>
            </div>
          )}
          {diagnostics.last_error_code && (
            <div className="flex justify-between">
              <span>{t("settings.remoteDiagnostics.lastError")}</span>
              <span className="text-[var(--color-destructive)]">
                {diagnostics.last_error_code}
              </span>
            </div>
          )}
        </div>

        {diagnostics.recent_operations.length > 0 && (
          <div className="space-y-1">
            <p className="text-[11px] font-semibold text-[var(--color-text)]">
              {t("settings.remoteDiagnostics.recentOps")}
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
          {t("settings.remoteDiagnostics.refresh")}
        </button>
      </div>
    </SettingsSectionCard>
  );
}
