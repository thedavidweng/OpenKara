import { useTranslation } from "react-i18next";
import { ConfirmationDialog } from "./ConfirmationDialog";
import { useSettingsOverlay } from "./SettingsOverlay.context";
import { formatBytes } from "@/lib/format";

export function SettingsDialogHost() {
  const { t } = useTranslation();
  const { state, meta, actions } = useSettingsOverlay();

  switch (meta.dangerDialog) {
    case "delete_stems":
      return (
        <ConfirmationDialog
          title={t("settings.confirmDeleteStems.title")}
          message={t("settings.confirmDeleteStems.message")}
          detail={
            meta.stemsSize != null && meta.stemsSize > 0
              ? t("settings.confirmDeleteStems.detail", {
                  size: formatBytes(meta.stemsSize),
                })
              : undefined
          }
          confirmLabel={t("settings.confirmDeleteStems.confirm")}
          onConfirm={() => void actions.confirmDeleteStems()}
          onCancel={actions.closeDialog}
        />
      );

    case "downgrade_stems":
      return (
        <ConfirmationDialog
          title={t("settings.confirmDowngradeStems.title")}
          message={t("settings.confirmDowngradeStems.message")}
          detail={
            meta.downgradeSavings != null && meta.downgradeSavings > 0
              ? t("settings.confirmDowngradeStems.detail", {
                  size: formatBytes(meta.downgradeSavings),
                })
              : undefined
          }
          confirmLabel={t("settings.confirmDowngradeStems.confirm")}
          onConfirm={() => void actions.confirmDowngrade()}
          onCancel={actions.closeDialog}
        />
      );

    case "delete_lyrics":
      return (
        <ConfirmationDialog
          title={t("settings.confirmDeleteLyrics.title")}
          message={t("settings.confirmDeleteLyrics.message")}
          confirmLabel={t("settings.confirmDeleteLyrics.confirm")}
          onConfirm={() => void actions.confirmDeleteLyrics()}
          onCancel={actions.closeDialog}
        />
      );

    case "ft_warning":
      return (
        <ConfirmationDialog
          title={t("settings.modelVariant.ftWarningTitle")}
          message={t("settings.modelVariant.ftWarningMessage")}
          confirmLabel={t("settings.modelVariant.ftWarningConfirm")}
          onConfirm={() => void actions.confirmFtModel()}
          onCancel={actions.closeDialog}
        />
      );

    case "delete_runtime":
      return (
        <ConfirmationDialog
          title={t("settings.confirmDeleteRuntime.title")}
          message={t("settings.confirmDeleteRuntime.message")}
          confirmLabel={t("settings.confirmDeleteRuntime.confirm")}
          onConfirm={() => void actions.deleteRuntime()}
          onCancel={actions.closeDialog}
        />
      );

    case "integrity_cleanup_confirm": {
      const selectedCount = state.integritySelection.size;
      return (
        <ConfirmationDialog
          title={t("settings.integrity.confirmCleanupTitle")}
          message={t("settings.integrity.confirmCleanupMessage", {
            count: selectedCount,
          })}
          confirmLabel={t("settings.integrity.confirmCleanupButton")}
          onConfirm={() => void actions.confirmIntegrityCleanup()}
          onCancel={actions.closeDialog}
        />
      );
    }

    default:
      return null;
  }
}
