import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingsSectionCard } from "./SettingsSectionCard";
import { formatBytes } from "@/lib/format";
import { notifyError } from "@/lib/errors";
import * as api from "@/lib/tauri";
import type { CacheUsage } from "@/types/ipc";

// Remote streaming cache settings section (PR #8, issue #151).
export function SettingsRemoteCacheSection() {
  const { t } = useTranslation();
  const [usage, setUsage] = useState<CacheUsage | null>(null);
  const [clearing, setClearing] = useState(false);
  const [evictedCount, setEvictedCount] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    try {
      const u = await api.getRemoteCacheUsage();
      setUsage(u);
    } catch (err) {
      notifyError(err);
    }
  }, []);

  useEffect(() => {
    setEvictedCount(null);
    void refresh();
  }, [refresh]);

  const handleClear = useCallback(async () => {
    setClearing(true);
    try {
      const count = await api.clearRemoteCache();
      setEvictedCount(count);
      await refresh();
    } catch (err) {
      notifyError(err);
    } finally {
      setClearing(false);
    }
  }, [refresh]);

  return (
    <SettingsSectionCard
      title={t("settings.remoteCache.label", {
        defaultValue: "Remote Streaming Cache",
      })}
      description={t("settings.remoteCache.description", {
        defaultValue:
          "Byte-range downloads of remote media files are cached for playback resume.",
      })}
    >
      <div className="space-y-3">
        {usage && (
          <div className="space-y-1 text-[12px] text-[var(--color-text-dim)]">
            <div className="flex justify-between">
              <span>
                {t("settings.remoteCache.used", { defaultValue: "Used" })}
              </span>
              <span className="text-[var(--color-text)]">
                {formatBytes(usage.used_bytes)} /{" "}
                {formatBytes(usage.limit_bytes)}
              </span>
            </div>
            <div className="flex justify-between">
              <span>
                {t("settings.remoteCache.entries", {
                  defaultValue: "Entries",
                })}
              </span>
              <span className="text-[var(--color-text)]">
                {usage.entry_count}
                {usage.pinned_count > 0 && (
                  <span className="text-[var(--color-text-dim)]">
                    {" "}
                    ({usage.pinned_count}{" "}
                    {t("settings.remoteCache.pinned", {
                      defaultValue: "pinned",
                    })}
                    )
                  </span>
                )}
              </span>
            </div>
          </div>
        )}

        {evictedCount !== null && evictedCount > 0 && (
          <p className="text-[11px] text-[var(--color-text-dim)]">
            {t("settings.remoteCache.evicted", {
              count: evictedCount,
              defaultValue: "Evicted {{count}} entries.",
            })}
          </p>
        )}

        <button
          type="button"
          onClick={() => void handleClear()}
          disabled={clearing || !usage || usage.used_bytes === 0}
          className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] text-[var(--color-destructive)] transition-colors hover:bg-[var(--color-destructive)] hover:text-[var(--color-destructive-foreground)] hover:border-[var(--color-destructive)] disabled:opacity-50"
        >
          {clearing
            ? t("common.deleting", { defaultValue: "Clearing…" })
            : t("settings.remoteCache.clearButton", {
                defaultValue: "Clear Cache",
              })}
        </button>
      </div>
    </SettingsSectionCard>
  );
}
