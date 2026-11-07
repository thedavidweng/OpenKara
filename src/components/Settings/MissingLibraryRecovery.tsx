import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import {
  type LucideIcon,
  FolderSearch,
  Library,
  Plus,
  RefreshCw,
  TriangleAlert,
} from "lucide-react";
import { getErrorMessage } from "@/lib/errors";
import { useBackend } from "@/lib/backend";
import { createLibrarySession } from "@/lib/library-session";
import type { LibraryRegistrySnapshot } from "@/types/ipc";

interface MissingLibraryRecoveryProps {
  registry: LibraryRegistrySnapshot;
  onResolved: () => void;
}

interface ActionRowProps {
  icon: LucideIcon;
  title: string;
  description?: string;
  disabled: boolean;
  onClick: () => void;
}

function ActionRow({
  icon: Icon,
  title,
  description,
  disabled,
  onClick,
}: ActionRowProps) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="flex w-full items-center gap-3 rounded-lg border border-[var(--color-border-light)] bg-[var(--color-sidebar)] px-5 py-4 text-left transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50"
    >
      <Icon
        size={20}
        className="shrink-0 text-[var(--color-control-primary)]"
      />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[14px] font-medium text-[var(--color-text)]">
          {title}
        </div>
        {description && (
          <div className="text-[12px] text-[var(--color-text-dim)]">
            {description}
          </div>
        )}
      </div>
    </button>
  );
}

export function MissingLibraryRecovery({
  registry,
  onResolved,
}: MissingLibraryRecoveryProps) {
  const backend = useBackend();
  const { t } = useTranslation();
  const librarySession = useMemo(
    () => createLibrarySession({ backend }),
    [backend],
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const unavailable = registry.active_library_unavailable;
  const missingId = registry.active_library_id;
  if (!unavailable || !missingId) return null;

  const otherLibraries = registry.libraries.filter(
    (library) => library.id !== missingId,
  );

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      onResolved();
    } catch (err: unknown) {
      setError(getErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const pickDirectory = async (title: string) => {
    const selected = await open({ directory: true, title });
    return typeof selected === "string" ? selected : (selected?.[0] ?? null);
  };

  const handleLocate = async () => {
    const directory = await pickDirectory(
      t("libraryRecovery.dialogTitleLocate"),
    );
    if (!directory) return;
    await run(async () => {
      await librarySession.openLocalLibrary(directory);
      const updated = await backend.librarySetup.getLibraryRegistry();
      if (updated.active_library_id !== missingId) {
        await backend.librarySetup.removeLibrary(missingId);
      }
    });
  };

  const handleCreate = async () => {
    const directory = await pickDirectory(t("setup.dialogTitleCreate"));
    if (!directory) return;
    await run(() => librarySession.createLocalLibrary(directory));
  };

  return (
    <div className="flex h-screen w-full items-center justify-center overflow-y-auto bg-[var(--color-surface)]">
      <div className="mx-auto max-h-full w-full max-w-md space-y-6 px-6 py-8 text-center">
        <div className="flex flex-col items-center gap-4">
          <TriangleAlert
            size={32}
            className="text-[var(--color-control-primary)]"
          />
          <h1 className="text-2xl font-bold text-[var(--color-text)]">
            {t("libraryRecovery.title")}
          </h1>
          <p className="break-all rounded-md bg-[var(--color-sidebar)] px-3 py-2 font-mono text-[12px] text-[var(--color-text)]">
            {unavailable.path}
          </p>
          <p className="text-[14px] text-[var(--color-text-dim)]">
            {unavailable.reason === "not_found"
              ? t("libraryRecovery.descriptionNotFound")
              : t("libraryRecovery.descriptionNotOpenable")}
          </p>
        </div>

        {error && (
          <p role="alert" className="text-[13px] text-red-400">
            {error}
          </p>
        )}

        <div className="space-y-3">
          <ActionRow
            icon={RefreshCw}
            title={t("libraryRecovery.retry")}
            description={t("libraryRecovery.retryDescription")}
            disabled={busy}
            onClick={() =>
              void run(() => librarySession.switchLibrary(missingId))
            }
          />
          <ActionRow
            icon={FolderSearch}
            title={t("libraryRecovery.locate")}
            description={t("libraryRecovery.locateDescription")}
            disabled={busy}
            onClick={() => void handleLocate()}
          />
          <ActionRow
            icon={Plus}
            title={t("setup.createNew")}
            description={t("setup.createNewDescription")}
            disabled={busy}
            onClick={() => void handleCreate()}
          />
        </div>

        {otherLibraries.length > 0 && (
          <div className="space-y-3">
            <h2 className="text-[12px] font-medium uppercase tracking-wide text-[var(--color-text-dim)]">
              {t("libraryRecovery.otherLibraries")}
            </h2>
            {otherLibraries.map((library) => (
              <ActionRow
                key={library.id}
                icon={Library}
                title={library.display_name}
                disabled={busy}
                onClick={() =>
                  void run(() => librarySession.switchLibrary(library.id))
                }
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
