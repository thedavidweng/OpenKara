import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { ShieldCheck, X } from "lucide-react";
import { useSettingsOverlay } from "./SettingsOverlay.context";
import type { IntegrityReport, ManagedAssetIssue } from "@/types/ipc";

const ASSET_TYPE_LABEL_KEYS: Record<string, string> = {
  primary_media: "settings.integrity.assetTypePrimaryMedia",
  cdg: "settings.integrity.assetTypeCdg",
  stem_vocals: "settings.integrity.assetTypeStemVocals",
  stem_accomp: "settings.integrity.assetTypeStemAccomp",
  stem_drums: "settings.integrity.assetTypeStemDrums",
  stem_bass: "settings.integrity.assetTypeStemBass",
  stem_other: "settings.integrity.assetTypeStemOther",
  artwork_thumb: "settings.integrity.assetTypeArtworkThumb",
  artwork_preview: "settings.integrity.assetTypeArtworkPreview",
};

function assetTypeLabel(t: TFunction, assetType: string): string {
  const key = ASSET_TYPE_LABEL_KEYS[assetType];
  return key ? t(key as never) : assetType;
}

function IssueRow({
  issue,
  selectable,
  selected,
  onToggle,
  t,
}: {
  issue: ManagedAssetIssue;
  selectable: boolean;
  selected: boolean;
  onToggle: () => void;
  t: TFunction;
}) {
  return (
    <li className="flex items-center gap-2 py-1 text-[12px]">
      {selectable ? (
        <input
          type="checkbox"
          checked={selected}
          onChange={onToggle}
          className="shrink-0"
        />
      ) : null}
      <span className="shrink-0 font-mono text-[var(--color-text-dim)]">
        {issue.song_hash.slice(0, 8)}
      </span>
      <span className="shrink-0 text-[var(--color-text-dim)]">
        {assetTypeLabel(t, issue.asset_type)}
      </span>
      <span className="min-w-0 truncate text-[var(--color-text)]">
        {issue.path || t("settings.integrity.emptyPath")}
      </span>
    </li>
  );
}

function ReportSection({
  title,
  count,
  issues,
  selectable,
  t,
}: {
  title: string;
  count: number;
  issues: ManagedAssetIssue[];
  selectable: boolean;
  t: TFunction;
}) {
  const { state, actions } = useSettingsOverlay();

  if (count === 0) {
    return (
      <div className="space-y-1">
        <h4 className="text-[13px] font-medium text-[var(--color-text)]">
          {title}
        </h4>
        <p className="text-[12px] text-[var(--color-text-dim)]">
          {t("settings.integrity.noIssues")}
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-1">
      <h4 className="text-[13px] font-medium text-[var(--color-text)]">
        {title} <span className="text-[var(--color-text-dim)]">({count})</span>
      </h4>
      <ul className="space-y-0.5">
        {issues.map((issue, index) => (
          <IssueRow
            key={`${issue.song_hash}-${issue.asset_type}-${issue.path}-${index}`}
            issue={issue}
            selectable={selectable}
            selected={state.integritySelection.has(issue.song_hash)}
            onToggle={() => actions.toggleIntegritySelection(issue.song_hash)}
            t={t}
          />
        ))}
      </ul>
    </div>
  );
}

export function IntegrityReportModal({ report }: { report: IntegrityReport }) {
  const { t } = useTranslation();
  const { state, meta, actions } = useSettingsOverlay();

  const hasSelection = state.integritySelection.size > 0;
  const totalIssues =
    report.missing_primary_media.length +
    report.empty_primary_media.length +
    report.missing_optional_assets.length +
    report.empty_optional_assets.length +
    report.orphaned_managed_files.length;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="flex max-h-[80vh] w-full max-w-2xl flex-col rounded-lg border border-[var(--color-border-light)] bg-[var(--color-surface)] shadow-xl">
        <div className="flex items-center justify-between border-b border-[var(--color-border-light)] px-4 py-3">
          <div className="flex items-center gap-2">
            <ShieldCheck size={16} className="text-[var(--color-accent)]" />
            <h3 className="text-[14px] font-medium text-white">
              {t("settings.integrity.reportTitle")}
            </h3>
          </div>
          <button
            type="button"
            onClick={actions.closeIntegrityReport}
            className="rounded-md p-1 text-[var(--color-text-dim)] transition-colors hover:bg-[var(--color-hover)] hover:text-white"
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto space-y-4 px-4 py-3">
          <div className="flex gap-4 text-[12px] text-[var(--color-text-dim)]">
            <span>
              {t("settings.integrity.checkedLocal", {
                count: report.checked_local_songs,
              })}
            </span>
            <span>
              {t("settings.integrity.skippedRemote", {
                count: report.skipped_remote_songs,
              })}
            </span>
            {totalIssues === 0 ? (
              <span className="text-[var(--color-accent)]">
                {t("settings.integrity.allClean")}
              </span>
            ) : null}
          </div>

          {state.integritySkippedCount != null &&
          state.integritySkippedCount > 0 ? (
            <p className="text-[12px] text-[var(--color-text-dim)]">
              {t("settings.integrity.skippedNotice", {
                count: state.integritySkippedCount,
              })}
            </p>
          ) : null}

          <ReportSection
            title={t("settings.integrity.missingPrimary")}
            count={report.missing_primary_media.length}
            issues={report.missing_primary_media}
            selectable
            t={t}
          />
          <ReportSection
            title={t("settings.integrity.emptyPrimary")}
            count={report.empty_primary_media.length}
            issues={report.empty_primary_media}
            selectable
            t={t}
          />
          <ReportSection
            title={t("settings.integrity.missingOptional")}
            count={report.missing_optional_assets.length}
            issues={report.missing_optional_assets}
            selectable={false}
            t={t}
          />
          <ReportSection
            title={t("settings.integrity.emptyOptional")}
            count={report.empty_optional_assets.length}
            issues={report.empty_optional_assets}
            selectable={false}
            t={t}
          />

          <div className="space-y-1">
            <h4 className="text-[13px] font-medium text-[var(--color-text)]">
              {t("settings.integrity.orphanedFiles")}{" "}
              <span className="text-[var(--color-text-dim)]">
                ({report.orphaned_managed_files.length})
              </span>
            </h4>
            {report.orphaned_managed_files.length === 0 ? (
              <p className="text-[12px] text-[var(--color-text-dim)]">
                {t("settings.integrity.noIssues")}
              </p>
            ) : (
              <ul className="space-y-0.5">
                {report.orphaned_managed_files.map((path, index) => (
                  <li
                    key={`${path}-${index}`}
                    className="py-1 text-[12px] text-[var(--color-text)]"
                  >
                    {path}
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>

        <div className="flex items-center justify-between border-t border-[var(--color-border-light)] px-4 py-3">
          <button
            type="button"
            onClick={actions.closeIntegrityReport}
            className="rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] hover:text-white"
          >
            {t("common.close")}
          </button>
          <button
            type="button"
            onClick={actions.openIntegrityCleanupConfirmDialog}
            disabled={!hasSelection || meta.integrityCleanupInProgress}
            className="rounded-md border border-[var(--color-destructive)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] text-[var(--color-destructive)] transition-colors hover:bg-[var(--color-destructive)] hover:text-[var(--color-destructive-foreground)] disabled:opacity-50"
          >
            {meta.integrityCleanupInProgress
              ? t("common.deleting")
              : t("settings.integrity.removeSelected")}
          </button>
        </div>
      </div>
    </div>
  );
}
