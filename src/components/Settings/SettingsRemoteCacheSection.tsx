import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { formatBytes } from "@/lib/format";
import { notifyError } from "@/lib/errors";
import { useBackend } from "@/lib/backend";
import type { CacheUsage } from "@/types/ipc";

export function SettingsRemoteCacheSection() {
  const { remoteRepository } = useBackend();
  const { t } = useTranslation();
  const [usage, setUsage] = useState<CacheUsage | null>(null);
  const [clearing, setClearing] = useState(false);
  const [evictedCount, setEvictedCount] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    try {
      const u = await remoteRepository.getRemoteCacheUsage();
      setUsage(u);
    } catch (err) {
      notifyError(err);
    }
  }, [remoteRepository]);

  useEffect(() => {
    setEvictedCount(null);
    void refresh();
  }, [refresh]);

  const handleClear = useCallback(async () => {
    setClearing(true);
    try {
      const count = await remoteRepository.clearRemoteCache();
      setEvictedCount(count);
      await refresh();
    } catch (err) {
      notifyError(err);
    } finally {
      setClearing(false);
    }
  }, [refresh, remoteRepository]);

  return (
    <SettingsSectionCard
      title={t("settings.remoteCache.label")}
      description={t("settings.remoteCache.description")}
    >
      <div className="space-y-3">
        {usage && (
          <div className="space-y-1 text-[12px] text-[var(--color-text-dim)]">
            <div className="flex justify-between">
              <span>{t("settings.remoteCache.used")}</span>
              <span className="text-[var(--color-text)]">
                {formatBytes(usage.used_bytes)} /{" "}
                {formatBytes(usage.limit_bytes)}
              </span>
            </div>
            <div className="flex justify-between">
              <span>{t("settings.remoteCache.entries")}</span>
              <span className="text-[var(--color-text)]">
                {usage.entry_count}
                {usage.pinned_count > 0 && (
                  <span className="text-[var(--color-text-dim)]">
                    {" "}
                    ({usage.pinned_count} {t("settings.remoteCache.pinned")})
                  </span>
                )}
              </span>
            </div>
          </div>
        )}

        {evictedCount !== null && evictedCount > 0 && (
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.remoteCache.evicted", { count: evictedCount })}
          </p>
        )}

        <button
          type="button"
          onClick={() => void handleClear()}
          disabled={clearing || !usage || usage.used_bytes === 0}
          className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] text-[var(--color-destructive)] transition-colors hover:bg-[var(--color-destructive)] hover:text-[var(--color-destructive-foreground)] hover:border-[var(--color-destructive)] disabled:opacity-50"
        >
          {clearing
            ? t("settings.remoteCache.clearing")
            : t("settings.remoteCache.clearButton")}
        </button>
      </div>
    </SettingsSectionCard>
  );
}
