import { useTranslation } from "react-i18next";
import { ConfirmationDialog } from "./ConfirmationDialog";
import { useSettings } from "./SettingsController.context";
import { formatBytes } from "@/lib/format";

export function SettingsDialogHost() {
  const { t } = useTranslation();
  const { view, maintenance } = useSettings();

  const confirm = () => void maintenance.confirmDialog();

  switch (view.dialog) {
    case "delete_stems":
      return (
        <ConfirmationDialog
          title={t("settings.confirmDeleteStems.title")}
          message={t("settings.confirmDeleteStems.message")}
          detail={
            view.maintenance.stemsSize != null && view.maintenance.stemsSize > 0
              ? t("settings.confirmDeleteStems.detail", {
                  size: formatBytes(view.maintenance.stemsSize),
                })
              : undefined
          }
          confirmLabel={t("settings.confirmDeleteStems.confirm")}
          onConfirm={confirm}
          onCancel={maintenance.closeDialog}
        />
      );

    case "downgrade_stems":
      return (
        <ConfirmationDialog
          title={t("settings.confirmDowngradeStems.title")}
          message={t("settings.confirmDowngradeStems.message")}
          detail={
            view.maintenance.downgradeSavings != null &&
            view.maintenance.downgradeSavings > 0
              ? t("settings.confirmDowngradeStems.detail", {
                  size: formatBytes(view.maintenance.downgradeSavings),
                })
              : undefined
          }
          confirmLabel={t("settings.confirmDowngradeStems.confirm")}
          onConfirm={confirm}
          onCancel={maintenance.closeDialog}
        />
      );

    case "delete_lyrics":
      return (
        <ConfirmationDialog
          title={t("settings.confirmDeleteLyrics.title")}
          message={t("settings.confirmDeleteLyrics.message")}
          confirmLabel={t("settings.confirmDeleteLyrics.confirm")}
          onConfirm={confirm}
          onCancel={maintenance.closeDialog}
        />
      );

    case "ft_warning":
      return (
        <ConfirmationDialog
          title={t("settings.modelVariant.ftWarningTitle")}
          message={t("settings.modelVariant.ftWarningMessage")}
          confirmLabel={t("settings.modelVariant.ftWarningConfirm")}
          onConfirm={confirm}
          onCancel={maintenance.closeDialog}
        />
      );

    case "delete_runtime":
      return (
        <ConfirmationDialog
          title={t("settings.confirmDeleteRuntime.title")}
          message={t("settings.confirmDeleteRuntime.message")}
          confirmLabel={t("settings.confirmDeleteRuntime.confirm")}
          onConfirm={confirm}
          onCancel={maintenance.closeDialog}
        />
      );

    case "integrity_cleanup_confirm":
      return (
        <ConfirmationDialog
          title={t("settings.integrity.confirmCleanupTitle")}
          message={t("settings.integrity.confirmCleanupMessage", {
            count: view.integrity.selection.size,
          })}
          confirmLabel={t("settings.integrity.confirmCleanupButton")}
          onConfirm={confirm}
          onCancel={maintenance.closeDialog}
        />
      );

    default:
      return null;
  }
}
