import { buttonVariants } from "@/components/ui/button-variants";
import { visibleName } from "@/components/Catalog/streaming-utils";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useModalDialog } from "@/hooks/use-modal-dialog";
import { useBackend } from "@/lib/backend";
import { useCatalogStore } from "@/stores/catalog-store";
import type { MarketUpdate, SourceSnapshot } from "@/types/ipc";
import { useSettingsStore } from "@/stores/settings-store";
import { SourceActionDialog } from "./SourceActionDialog";
import { AddedAccessReview } from "./SourceMarket";
import { OfficialBadge } from "./OfficialBadge";
import {
  errorMessage,
  isInstalledSource,
  isOfficialRegistry,
  sourceActionButton,
  sourceDetails,
  sourcePrimaryButton,
  sourceRow,
} from "./source-market-utils";

const rowButton = buttonVariants({ variant: "outline", className: "min-w-20" });
const rowPrimaryButton = buttonVariants({ className: "min-w-20" });

const hasSignIn = (source: SourceSnapshot) => {
  const auth = source.manifest?.authentication;
  return (
    !!auth && typeof auth === "object" && "flow" in auth && auth.flow !== "none"
  );
};

interface Decision {
  source: SourceSnapshot;
  kind: "disable" | "uninstall" | "signOut";
}
export function StreamingSources() {
  const { t } = useTranslation();
  const { catalog } = useBackend();
  const sources = useCatalogStore((state) => state.sources);
  const status = useCatalogStore((state) => state.sourcesStatus);
  const error = useCatalogStore((state) => state.sourcesError);
  const sessions = useCatalogStore((state) => state.sourceStates);
  const apply = useCatalogStore((state) => state.applySource);
  const market = useCatalogStore((state) => state.market);
  const registries = useCatalogStore((state) => state.registries);
  const registriesStatus = useCatalogStore((state) => state.registriesStatus);
  const [busy, setBusy] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [decision, setDecision] = useState<Decision | null>(null);
  const [consentUpdate, setConsentUpdate] = useState<{
    source: SourceSnapshot;
    update: MarketUpdate;
  } | null>(null);
  const [removeCredentials, setRemoveCredentials] = useState<boolean | null>(
    null,
  );
  const [stopWork, setStopWork] = useState(false);
  const dialog = useRef<HTMLDivElement>(null);
  useModalDialog({
    dialogRef: dialog,
    enabled: !!decision,
    canDismiss: busy === null,
    onDismiss: () => setDecision(null),
  });
  useEffect(() => {
    if (status === "idle") void useCatalogStore.getState().loadSources();
  }, [status]);
  // Official badges and update offers need registries and the market even
  // when the Discover tab, which also loads them, has never been opened.
  const marketStatus = useCatalogStore((s) => s.marketStatus);
  useEffect(() => {
    if (registriesStatus === "idle")
      void useCatalogStore.getState().loadRegistries();
  }, [registriesStatus]);
  useEffect(() => {
    if (marketStatus === "idle") void useCatalogStore.getState().loadMarket();
  }, [marketStatus]);
  const run = async (
    source: SourceSnapshot,
    operation: () => Promise<SourceSnapshot | void>,
  ) => {
    setBusy(source.source_id);
    setErrors((old) => ({ ...old, [source.source_id]: "" }));
    try {
      const result = await operation();
      if (result) apply(result);
      useCatalogStore.getState().applyMarket(await catalog.listSourceMarket());
      return true;
    } catch (failure) {
      setErrors((old) => ({
        ...old,
        [source.source_id]: errorMessage(failure),
      }));
      return false;
    } finally {
      setBusy(null);
    }
  };
  const marketUpdate = (source: SourceSnapshot) =>
    source.registry_id && source.package_id
      ? market.find(
          (pkg) =>
            pkg.registry_id === source.registry_id &&
            pkg.package_id === source.package_id,
        )?.update
      : undefined;
  const installOrUpdate = (source: SourceSnapshot) => {
    const update = marketUpdate(source);
    if (source.installed_version && update?.requires_consent) {
      setConsentUpdate({ source, update });
      return;
    }
    void run(source, () =>
      catalog.installStreamingSource(source.source_id, null, false),
    );
  };
  const confirmConsentUpdate = async () => {
    const target = consentUpdate;
    if (!target) return;
    const succeeded = await run(target.source, () =>
      catalog.installStreamingSource(target.source.source_id, null, true),
    );
    if (succeeded) setConsentUpdate(null);
  };
  const registryRemoved = (source: SourceSnapshot) =>
    source.registry_id !== null &&
    registriesStatus === "ready" &&
    !registries.some((entry) => entry.registry_id === source.registry_id);
  const decide = (source: SourceSnapshot, kind: Decision["kind"]) => {
    setRemoveCredentials(null);
    setStopWork(false);
    setDecision({ source, kind });
  };
  const confirm = async () => {
    if (!decision) return;
    const { source, kind } = decision;
    if (
      (source.active_work.length > 0 && !stopWork) ||
      (kind === "uninstall" && removeCredentials === null)
    )
      return;
    const succeeded = await run(source, () =>
      kind === "uninstall"
        ? catalog.uninstallStreamingSource(
            source.source_id,
            stopWork,
            removeCredentials === true,
          )
        : kind === "signOut"
          ? useCatalogStore.getState().signOut(source.source_id, stopWork)
          : catalog.setStreamingSourceEnabled(
              source.source_id,
              false,
              stopWork,
            ),
    );
    if (succeeded) setDecision(null);
  };
  const updatable = sources.filter((source) => {
    const update = marketUpdate(source);
    return (
      source.installed_version &&
      update &&
      !update.requires_consent &&
      source.state !== "installing" &&
      source.state !== "cancelling"
    );
  });
  const updateAll = async () => {
    for (const source of updatable)
      await run(source, () =>
        catalog.installStreamingSource(source.source_id, null, false),
      );
  };
  const installed = sources.filter(isInstalledSource);
  const openSource = (source: SourceSnapshot, signIn: boolean) => {
    useSettingsStore.getState().close();
    const store = useCatalogStore.getState();
    store.setActiveSource(source.source_id);
    if (signIn) void store.openSignIn(source.source_id);
  };
  const toggleEnabled = (source: SourceSnapshot) =>
    source.enabled
      ? source.active_work.length
        ? decide(source, "disable")
        : void run(source, () =>
            catalog.setStreamingSourceEnabled(source.source_id, false, false),
          )
      : void run(source, () =>
          catalog.setStreamingSourceEnabled(source.source_id, true, false),
        );
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="min-w-0 flex-1 basis-56 text-[12px] text-[var(--color-text-dim)]">
          {t("settings.streaming.autoUpdateNotice")}
        </p>
        <div className="flex flex-wrap gap-2">
          {updatable.length > 1 ? (
            <button
              className={sourcePrimaryButton}
              disabled={busy !== null}
              onClick={() => void updateAll()}
            >
              {t("settings.streaming.updateAll", { count: updatable.length })}
            </button>
          ) : null}
          <button
            className={sourceActionButton}
            disabled={status === "loading"}
            onClick={() =>
              void (async () => {
                const store = useCatalogStore.getState();
                await store.loadSources(true);
                await Promise.all([store.loadRegistries(), store.loadMarket()]);
              })()
            }
          >
            {t("settings.streaming.refresh")}
          </button>
        </div>
      </div>
      {error ? (
        <p role="alert" className="text-[12px]">
          {error}
        </p>
      ) : null}
      {status === "loading" ? <p role="status">{t("common.loading")}</p> : null}
      {status === "ready" && installed.length === 0 ? (
        <p className="text-[12px] text-[var(--color-text-dim)]">
          {t("settings.streaming.empty")}
        </p>
      ) : null}
      {installed.map((source) => {
        const pending = busy === source.source_id;
        const session = sessions[source.source_id]?.session;
        const update = marketUpdate(source);
        const working =
          source.state === "installing" || source.state === "cancelling";
        const signedIn = !!session?.signed_in && !session.expired;
        const summary = [
          t(`settings.streaming.states.${source.state}`),
          source.installed_version,
          update ? t("settings.streaming.updateAvailable") : null,
          session?.expired
            ? t("catalog.streaming.sessionExpired")
            : session?.signed_in
              ? visibleName(session.display_name)
                ? t("settings.streaming.signedInAs", {
                    name: visibleName(session.display_name),
                  })
                : t("catalog.streaming.signedIn")
              : null,
        ].filter(Boolean);
        return (
          <article
            key={source.source_id}
            aria-label={source.name}
            className={sourceRow}
          >
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <header className="min-w-0 flex-1 basis-48">
                <div className="flex items-center gap-2">
                  <h4 className="min-w-0 truncate font-semibold">
                    {source.name}
                  </h4>
                  {isOfficialRegistry(registries, source.registry_id) ? (
                    <OfficialBadge />
                  ) : null}
                </div>
                <p className="truncate text-[12px] text-[var(--color-text-dim)]">
                  {summary.join(" · ")}
                </p>
              </header>
              <div className="ml-auto flex flex-wrap justify-end gap-2">
                {working ? (
                  <button
                    className={rowButton}
                    disabled={source.state === "cancelling"}
                    onClick={() =>
                      void run(source, () =>
                        catalog.cancelStreamingSourceInstall(source.source_id),
                      )
                    }
                  >
                    {t("common.cancel")}
                  </button>
                ) : (
                  <>
                    {update ||
                    source.last_error ||
                    !source.installed_version ? (
                      <button
                        className={rowPrimaryButton}
                        disabled={pending || !source.available_version}
                        onClick={() => installOrUpdate(source)}
                      >
                        {!source.installed_version
                          ? t("settings.streaming.install")
                          : source.last_error && !update
                            ? t("common.retry")
                            : t("settings.streaming.market.updateTo", {
                                version:
                                  update?.version ?? source.available_version,
                              })}
                      </button>
                    ) : null}
                    {source.enabled &&
                    source.loaded &&
                    source.state === "ready" ? (
                      <button
                        className={rowButton}
                        onClick={() =>
                          openSource(source, !signedIn && hasSignIn(source))
                        }
                      >
                        {signedIn || !hasSignIn(source)
                          ? t("settings.streaming.open")
                          : t("catalog.streaming.connect")}
                      </button>
                    ) : null}
                    {source.installed_version ? (
                      <button
                        className={rowButton}
                        disabled={pending}
                        onClick={() => toggleEnabled(source)}
                      >
                        {t(
                          source.enabled
                            ? "settings.streaming.disable"
                            : "settings.streaming.enable",
                        )}
                      </button>
                    ) : null}
                  </>
                )}
              </div>
            </div>
            {source.progress ? (
              <div
                role="progressbar"
                aria-label={source.progress.phase}
                aria-valuemin={0}
                aria-valuemax={source.progress.total ?? undefined}
                aria-valuenow={
                  source.progress.total ? source.progress.completed : undefined
                }
                className="text-[12px] text-[var(--color-text-dim)]"
              >
                {source.progress.phase}
                {source.progress.total
                  ? ` · ${Math.round((source.progress.completed / source.progress.total) * 100)}%`
                  : ""}
              </div>
            ) : null}
            {source.staged_version ? (
              <p role="status" className="text-[12px]">
                {t("settings.streaming.staged", {
                  version: source.staged_version,
                })}
              </p>
            ) : null}
            {sessions[source.source_id]?.errors.auth ? (
              <p role="alert" className="text-[12px]">
                {sessions[source.source_id].errors.auth}
              </p>
            ) : null}
            {source.last_error ? (
              <p role="alert" className="text-[12px]">
                {source.last_error.message}
              </p>
            ) : null}
            {registryRemoved(source) ? (
              <p className="text-[12px] text-[var(--color-text-dim)]">
                {t("settings.streaming.registryRemoved")}
              </p>
            ) : null}
            {errors[source.source_id] ? (
              <p role="alert" className="text-[12px]">
                {errors[source.source_id]}
              </p>
            ) : null}
            {source.active_work.length > 0 ? (
              <p className="text-[12px]">
                {t("settings.streaming.activeWork")}:{" "}
                {source.active_work
                  .map((work) => t(`settings.streaming.work.${work.kind}`))
                  .join(", ")}
              </p>
            ) : null}
            {source.installed_version ? (
              <details className={sourceDetails}>
                <summary>{t("settings.streaming.details")}</summary>
                <div className="mt-2 space-y-3">
                  {source.description ? (
                    <p className="text-[var(--color-text-dim)]">
                      {source.description}
                    </p>
                  ) : null}
                  <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
                    <dt>{t("settings.streaming.market.registry")}</dt>
                    <dd className="break-all">
                      {registries.find(
                        (entry) => entry.registry_id === source.registry_id,
                      )?.display_name ?? source.owner_binding}
                    </dd>
                    <dt>{t("settings.streaming.license")}</dt>
                    <dd>{source.license}</dd>
                    <dt>{t("settings.streaming.installed")}</dt>
                    <dd>{source.installed_version}</dd>
                    <dt>{t("settings.streaming.available")}</dt>
                    <dd>{source.available_version ?? "—"}</dd>
                    <dt>{t("settings.streaming.loaded")}</dt>
                    <dd>
                      {t(
                        source.loaded
                          ? "settings.streaming.loadedReady"
                          : "settings.streaming.loadedNotReady",
                      )}
                    </dd>
                  </dl>
                  <div className="flex flex-wrap gap-2">
                    {signedIn ? (
                      <button
                        className={sourceActionButton}
                        onClick={() =>
                          source.active_work.length
                            ? decide(source, "signOut")
                            : void useCatalogStore
                                .getState()
                                .signOut(source.source_id)
                        }
                      >
                        {t("catalog.streaming.signOut")}
                      </button>
                    ) : null}
                    <button
                      className={sourceActionButton}
                      disabled={pending}
                      onClick={() =>
                        void run(source, () =>
                          catalog.pinStreamingSource(
                            source.source_id,
                            source.pinned_version
                              ? null
                              : source.installed_version,
                          ),
                        )
                      }
                    >
                      {t(
                        source.pinned_version
                          ? "settings.streaming.unpin"
                          : "settings.streaming.pin",
                      )}
                      {source.pinned_version
                        ? ` · ${source.pinned_version}`
                        : ""}
                    </button>
                    <button
                      className={`${sourceActionButton} text-[var(--color-destructive)]`}
                      disabled={pending || working}
                      onClick={() => decide(source, "uninstall")}
                    >
                      {t("settings.streaming.uninstall")}
                    </button>
                  </div>
                </div>
              </details>
            ) : null}
          </article>
        );
      })}
      {decision ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
          <div
            ref={dialog}
            role="dialog"
            aria-modal="true"
            tabIndex={-1}
            aria-labelledby="source-decision-title"
            className="max-w-lg space-y-4 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-6 text-[var(--color-text)] backdrop:bg-black/50"
          >
            <h3 id="source-decision-title" className="font-semibold">
              {decision.source.name} ·{" "}
              {t(
                decision.kind === "signOut"
                  ? "catalog.streaming.signOut"
                  : `settings.streaming.${decision.kind}`,
              )}
            </h3>
            <p>{t("settings.streaming.libraryRetained")}</p>
            {errors[decision.source.source_id] ? (
              <p role="alert">{errors[decision.source.source_id]}</p>
            ) : null}
            {decision.source.active_work.length ? (
              <>
                <p>
                  {t("settings.streaming.stopWorkBody")}:{" "}
                  {decision.source.active_work
                    .map((work) => t(`settings.streaming.work.${work.kind}`))
                    .join(", ")}
                </p>
                <label className="flex gap-2">
                  <input
                    type="checkbox"
                    checked={stopWork}
                    onChange={(event) => setStopWork(event.target.checked)}
                  />
                  {t("settings.streaming.stopWork")}
                </label>
              </>
            ) : null}
            {decision.kind === "uninstall" ? (
              <fieldset className="space-y-2">
                <legend>{t("settings.streaming.credentialsChoice")}</legend>
                <label className="flex gap-2">
                  <input
                    type="radio"
                    name="credentials"
                    checked={removeCredentials === false}
                    onChange={() => setRemoveCredentials(false)}
                  />
                  {t("settings.streaming.retainCredentials")}
                </label>
                <label className="flex gap-2">
                  <input
                    type="radio"
                    name="credentials"
                    checked={removeCredentials === true}
                    onChange={() => setRemoveCredentials(true)}
                  />
                  {t("settings.streaming.removeCredentials")}
                </label>
              </fieldset>
            ) : null}
            <div className="flex gap-2">
              <button
                className={sourceActionButton}
                disabled={!!busy}
                onClick={() => setDecision(null)}
              >
                {t("common.cancel")}
              </button>
              <button
                className={sourceActionButton}
                disabled={
                  !!busy ||
                  (decision.source.active_work.length > 0 && !stopWork) ||
                  (decision.kind === "uninstall" && removeCredentials === null)
                }
                onClick={() => void confirm()}
              >
                {t(
                  decision.kind === "uninstall"
                    ? "settings.streaming.confirmUninstall"
                    : decision.kind === "signOut"
                      ? "catalog.streaming.signOut"
                      : "settings.streaming.confirmDisable",
                )}
              </button>
            </div>
          </div>
        </div>
      ) : null}
      {consentUpdate ? (
        <SourceActionDialog
          title={t("settings.streaming.market.consentTitle")}
          confirmLabel={t("settings.streaming.market.consentConfirm")}
          busy={busy !== null}
          onConfirm={() => void confirmConsentUpdate()}
          onCancel={() => setConsentUpdate(null)}
        >
          <p>
            {t("settings.streaming.market.consentBody", {
              name: consentUpdate.source.name,
              version: consentUpdate.update.version,
            })}
          </p>
          <AddedAccessReview
            addedOrigins={consentUpdate.update.added_origins}
            addedCapabilities={consentUpdate.update.added_capabilities}
          />
          {errors[consentUpdate.source.source_id] ? (
            <p role="alert">{errors[consentUpdate.source.source_id]}</p>
          ) : null}
        </SourceActionDialog>
      ) : null}
    </div>
  );
}
