import { useLibraryStore } from "@/stores/library-store";
import type { IntegrityReport } from "@/types/ipc";
import type {
  SettingsActionContext,
  SettingsOverlayActions,
} from "./settings-overlay.types";

/**
 * Default selection for a fresh integrity report: all songs with missing or
 * empty primary media are selected by default. Optional/orphan sections are
 * read-only.
 */
function defaultSelection(report: IntegrityReport): Set<string> {
  const hashes = new Set<string>();
  for (const issue of report.missing_primary_media) {
    hashes.add(issue.song_hash);
  }
  for (const issue of report.empty_primary_media) {
    hashes.add(issue.song_hash);
  }
  return hashes;
}

export function createIntegritySettingsActions(
  context: SettingsActionContext,
): Pick<
  SettingsOverlayActions,
  | "checkLibraryIntegrity"
  | "toggleIntegritySelection"
  | "confirmIntegrityCleanup"
  | "openIntegrityCleanupConfirmDialog"
  | "closeIntegrityReport"
> {
  const { dependencies, controls, patchState, patchMeta, closeDialog } =
    context;

  return {
    checkLibraryIntegrity: async () => {
      patchMeta({ integrityCheckInProgress: true });
      patchState({
        libraryError: null,
        integritySkippedCount: null,
      });
      try {
        const report = await dependencies.api.checkLibraryIntegrity();
        const selection = defaultSelection(report);
        patchState({
          integrityReport: report,
          integritySelection: selection,
        });
      } catch (error: unknown) {
        patchState({ integrityReport: null, integritySelection: new Set() });
        dependencies.notifyError(error);
      } finally {
        patchMeta({ integrityCheckInProgress: false });
      }
    },

    toggleIntegritySelection: (hash) => {
      const current = controls.getSnapshot().state;
      const next = new Set(current.integritySelection);
      if (next.has(hash)) {
        next.delete(hash);
      } else {
        next.add(hash);
      }
      patchState({ integritySelection: next });
    },

    openIntegrityCleanupConfirmDialog: () => {
      patchMeta({ dangerDialog: "integrity_cleanup_confirm" });
    },

    confirmIntegrityCleanup: async () => {
      const current = controls.getSnapshot().state;
      const selectedHashes = Array.from(current.integritySelection);
      if (selectedHashes.length === 0) {
        closeDialog();
        return;
      }

      patchMeta({ integrityCleanupInProgress: true });
      patchState({ libraryError: null });

      try {
        const result =
          await dependencies.api.removeMissingLibraryEntries(selectedHashes);

        if (result.deleted_song_hashes.length > 0) {
          dependencies.queueStore.removeSongIds(result.deleted_song_hashes);

          const deletedSet = new Set(result.deleted_song_hashes);
          const libraryState = useLibraryStore.getState();
          const affectedSelection = Array.from(
            libraryState.selectedSongIds,
          ).some((id) => deletedSet.has(id));
          if (affectedSelection) {
            dependencies.libraryStore.clearSelection();
          }
        }

        // Reload the library from the backend (mandatory).
        await dependencies.libraryStore.loadLibrary();
        await dependencies.playerStore.loadState();

        const deletedSet = new Set(result.deleted_song_hashes);
        if (current.integrityReport) {
          const updatedReport: IntegrityReport = {
            ...current.integrityReport,
            missing_primary_media:
              current.integrityReport.missing_primary_media.filter(
                (issue) => !deletedSet.has(issue.song_hash),
              ),
            empty_primary_media:
              current.integrityReport.empty_primary_media.filter(
                (issue) => !deletedSet.has(issue.song_hash),
              ),
            missing_optional_assets:
              current.integrityReport.missing_optional_assets.filter(
                (issue) => !deletedSet.has(issue.song_hash),
              ),
            empty_optional_assets:
              current.integrityReport.empty_optional_assets.filter(
                (issue) => !deletedSet.has(issue.song_hash),
              ),
          };
          const remainingSelection = new Set(
            Array.from(current.integritySelection).filter(
              (hash) => !deletedSet.has(hash),
            ),
          );
          patchState({
            integrityReport: updatedReport,
            integritySelection: remainingSelection,
            integritySkippedCount:
              result.skipped_song_hashes.length > 0
                ? result.skipped_song_hashes.length
                : null,
          });
        }

        closeDialog();
      } catch (error: unknown) {
        await dependencies.libraryStore.loadLibrary();
        dependencies.notifyError(error);
        closeDialog();
      } finally {
        patchMeta({ integrityCleanupInProgress: false });
      }
    },

    closeIntegrityReport: () => {
      patchState({
        integrityReport: null,
        integritySelection: new Set(),
        integritySkippedCount: null,
      });
    },
  };
}
