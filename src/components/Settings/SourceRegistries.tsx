import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useBackend } from "@/lib/backend";
import { useCatalogStore } from "@/stores/catalog-store";
import type { RegistryStatus } from "@/types/ipc";
import { SourceActionDialog } from "./SourceActionDialog";
import { sourceActionButton } from "./source-market-utils";

const errorMessage = (failure: unknown) =>
  failure instanceof Error
    ? failure.message
    : typeof failure === "object" && failure !== null && "message" in failure
      ? String(failure.message)
      : String(failure);

export function SourceRegistries() {
  const { t, i18n } = useTranslation();
  const { catalog } = useBackend();
  const registries = useCatalogStore((state) => state.registries);
  const status = useCatalogStore((state) => state.registriesStatus);
  const loadError = useCatalogStore((state) => state.registriesError);
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [addTarget, setAddTarget] = useState<string | null>(null);
  const [removeTarget, setRemoveTarget] = useState<RegistryStatus | null>(null);

  useEffect(() => {
    if (status === "idle") void useCatalogStore.getState().loadRegistries();
  }, [status]);

  const refreshMarket = async () => {
    useCatalogStore.getState().applyMarket(await catalog.listSourceMarket());
  };

  const confirmAdd = async () => {
    if (!addTarget) return;
    setBusy(true);
    setError(null);
    try {
      const created = await catalog.addSourceRegistry(addTarget);
      useCatalogStore.getState().applyRegistries([...registries, created]);
      await refreshMarket();
      setAddTarget(null);
      setUrl("");
    } catch (failure) {
      setError(errorMessage(failure));
      setAddTarget(null);
    } finally {
      setBusy(false);
    }
  };

  const confirmRemove = async () => {
    if (!removeTarget) return;
    setBusy(true);
    setError(null);
    try {
      await catalog.removeSourceRegistry(removeTarget.registry_id);
      useCatalogStore
        .getState()
        .applyRegistries(
          registries.filter(
            (entry) => entry.registry_id !== removeTarget.registry_id,
          ),
        );
      await refreshMarket();
      setRemoveTarget(null);
    } catch (failure) {
      setError(errorMessage(failure));
      setRemoveTarget(null);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section aria-labelledby="source-registries-title" className="space-y-3">
      <h3 id="source-registries-title" className="text-sm font-semibold">
        {t("settings.streaming.registries.title")}
      </h3>
      <p className="text-xs text-[var(--color-text-dim)]">
        {t("settings.streaming.registries.description")}
      </p>
      <form
        className="flex items-end gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          const trimmed = url.trim();
          if (trimmed) setAddTarget(trimmed);
        }}
      >
        <label className="flex-1 text-xs">
          <span className="mb-1 block">
            {t("settings.streaming.registries.urlLabel")}
          </span>
          <input
            type="text"
            value={url}
            placeholder={t("settings.streaming.registries.urlPlaceholder")}
            onChange={(event) => setUrl(event.target.value)}
            className="min-h-9 w-full rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-xs"
          />
        </label>
        <button
          type="submit"
          className={sourceActionButton}
          disabled={busy || !url.trim()}
        >
          {t("settings.streaming.registries.add")}
        </button>
      </form>
      {error ? (
        <p role="alert" className="text-xs">
          {error}
        </p>
      ) : null}
      {loadError ? (
        <p role="alert" className="text-xs">
          {loadError}
        </p>
      ) : null}
      {status === "loading" ? <p role="status">{t("common.loading")}</p> : null}
      {status === "ready" && registries.length === 0 ? (
        <p className="text-xs text-[var(--color-text-dim)]">
          {t("settings.streaming.registries.empty")}
        </p>
      ) : null}
      {registries.map((entry) => (
        <article
          key={entry.registry_id}
          aria-label={entry.display_name}
          className="space-y-2 rounded-xl border border-[var(--color-border)] p-4"
        >
          <header className="flex items-start justify-between gap-3">
            <div>
              <h4 className="font-semibold">{entry.display_name}</h4>
              <p className="break-all text-xs text-[var(--color-text-dim)]">
                {entry.url}
              </p>
            </div>
            <button
              className={sourceActionButton}
              disabled={busy}
              onClick={() => setRemoveTarget(entry)}
            >
              {t("settings.streaming.registries.remove")}
            </button>
          </header>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
            <dt>{t("settings.streaming.registries.homepage")}</dt>
            <dd className="break-all">{entry.homepage}</dd>
            <dt>{t("settings.streaming.registries.refreshed")}</dt>
            <dd>
              {entry.refreshed_at
                ? new Intl.DateTimeFormat(i18n.language, {
                    dateStyle: "medium",
                    timeStyle: "short",
                  }).format(entry.refreshed_at)
                : t("settings.streaming.registries.never")}
            </dd>
          </dl>
          {entry.last_error ? (
            <p role="alert" className="text-xs">
              {entry.last_error.message}
            </p>
          ) : null}
        </article>
      ))}
      {addTarget ? (
        <SourceActionDialog
          title={t("settings.streaming.registries.addTitle")}
          confirmLabel={t("settings.streaming.registries.addConfirm")}
          busy={busy}
          onConfirm={() => void confirmAdd()}
          onCancel={() => setAddTarget(null)}
        >
          <p className="break-all">
            {t("settings.streaming.registries.addBody", { url: addTarget })}
          </p>
        </SourceActionDialog>
      ) : null}
      {removeTarget ? (
        <SourceActionDialog
          title={t("settings.streaming.registries.removeTitle")}
          confirmLabel={t("settings.streaming.registries.removeConfirm")}
          busy={busy}
          onConfirm={() => void confirmRemove()}
          onCancel={() => setRemoveTarget(null)}
        >
          <p>
            {t("settings.streaming.registries.removeBody", {
              name: removeTarget.display_name,
            })}
          </p>
        </SourceActionDialog>
      ) : null}
    </section>
  );
}
