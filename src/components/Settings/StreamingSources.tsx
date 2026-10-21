import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useModalDialog } from "@/hooks/use-modal-dialog";
import { useBackend } from "@/lib/backend";
import { useCatalogStore } from "@/stores/catalog-store";
import type { SourceSnapshot } from "@/types/ipc";
import { StreamingSignIn } from "@/components/Catalog/StreamingSignIn";

const button =
  "min-h-9 rounded-full border border-[var(--color-border)] px-4 text-xs focus-visible:outline-2 disabled:opacity-50";
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
  const [busy, setBusy] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [decision, setDecision] = useState<Decision | null>(null);
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
  const run = async (
    source: SourceSnapshot,
    operation: () => Promise<SourceSnapshot | void>,
  ) => {
    setBusy(source.source_id);
    setErrors((old) => ({ ...old, [source.source_id]: "" }));
    try {
      const result = await operation();
      if (result) apply(result);
      return true;
    } catch (failure) {
      setErrors((old) => ({
        ...old,
        [source.source_id]:
          failure instanceof Error
            ? failure.message
            : typeof failure === "object" &&
                failure !== null &&
                "message" in failure
              ? String(failure.message)
              : String(failure),
      }));
      return false;
    } finally {
      setBusy(null);
    }
  };
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
  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold">
          {t("settings.streaming.title")}
        </h3>
        <button
          className={button}
          disabled={status === "loading"}
          onClick={() => void useCatalogStore.getState().loadSources(true)}
        >
          {t("settings.streaming.refresh")}
        </button>
      </div>
      <p className="text-xs text-[var(--color-text-dim)]">
        {t("settings.streaming.trustNotice")}
      </p>
      {error ? (
        <p role="alert" className="text-xs">
          {error}
        </p>
      ) : null}
      {status === "loading" ? <p role="status">{t("common.loading")}</p> : null}
      {status === "ready" && sources.length === 0 ? (
        <p>{t("settings.streaming.empty")}</p>
      ) : null}
      {sources.map((source) => {
        const pending = busy === source.source_id;
        const session = sessions[source.source_id]?.session;
        return (
          <article
            key={source.source_id}
            aria-label={source.name}
            className="space-y-3 rounded-xl border border-[var(--color-border)] p-4"
          >
            <header>
              <h4 className="font-semibold">{source.name}</h4>
              <p className="text-xs text-[var(--color-text-dim)]">
                {source.description}
              </p>
            </header>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
              <dt>{t("settings.streaming.publisher")}</dt>
              <dd>{source.owner_binding}</dd>
              <dt>{t("settings.streaming.license")}</dt>
              <dd>{source.license}</dd>
              <dt>{t("settings.streaming.installed")}</dt>
              <dd>
                {source.installed_version ??
                  t("settings.streaming.notInstalled")}
              </dd>
              <dt>{t("settings.streaming.available")}</dt>
              <dd>{source.available_version ?? "—"}</dd>
              <dt>{t("settings.streaming.runtime")}</dt>
              <dd>
                {t(
                  source.runtime_ready
                    ? "settings.streaming.runtimeReady"
                    : "settings.streaming.runtimeMissing",
                )}
              </dd>
              <dt>{t("settings.streaming.state")}</dt>
              <dd>{t(`settings.streaming.states.${source.state}`)}</dd>
              <dt>{t("settings.streaming.authentication")}</dt>
              <dd>
                {t(
                  session?.expired
                    ? "catalog.streaming.sessionExpired"
                    : session?.signed_in
                      ? "catalog.streaming.signedIn"
                      : "settings.streaming.signedOut",
                )}
                {session?.display_name ? ` · ${session.display_name}` : ""}
              </dd>
            </dl>
            {source.staged_version ? (
              <p role="status">
                {t("settings.streaming.staged", {
                  version: source.staged_version,
                })}
              </p>
            ) : null}
            {source.progress ? (
              <div
                role="progressbar"
                aria-label={source.progress.phase}
                aria-valuemin={0}
                aria-valuemax={source.progress.total ?? undefined}
                aria-valuenow={
                  source.progress.total ? source.progress.completed : undefined
                }
              >
                <span>
                  {source.progress.phase}
                  {source.progress.total
                    ? ` · ${Math.round((source.progress.completed / source.progress.total) * 100)}%`
                    : ""}
                </span>
              </div>
            ) : null}
            {sessions[source.source_id]?.errors.auth ? (
              <p role="alert">{sessions[source.source_id].errors.auth}</p>
            ) : null}
            {source.last_error ? (
              <p role="alert">{source.last_error.message}</p>
            ) : null}
            {errors[source.source_id] ? (
              <p role="alert">{errors[source.source_id]}</p>
            ) : null}
            {source.active_work.length > 0 ? (
              <p className="text-xs">
                {t("settings.streaming.activeWork")}:{" "}
                {source.active_work
                  .map((work) => t(`settings.streaming.work.${work.kind}`))
                  .join(", ")}
              </p>
            ) : null}
            <div className="flex flex-wrap gap-2">
              {source.state === "installing" ||
              source.state === "cancelling" ? (
                <button
                  className={button}
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
                  {!source.installed_version ||
                  source.available_version !== source.installed_version ||
                  source.last_error ? (
                    <button
                      className={button}
                      disabled={pending || !source.available_version}
                      onClick={() =>
                        void run(source, () =>
                          catalog.installStreamingSource(
                            source.source_id,
                            null,
                          ),
                        )
                      }
                    >
                      {t(
                        !source.installed_version
                          ? "settings.streaming.install"
                          : source.last_error
                            ? "common.retry"
                            : "settings.streaming.update",
                      )}
                    </button>
                  ) : null}
                  {source.installed_version ? (
                    <>
                      <button
                        className={button}
                        disabled={pending}
                        onClick={() =>
                          source.enabled
                            ? source.active_work.length
                              ? decide(source, "disable")
                              : void run(source, () =>
                                  catalog.setStreamingSourceEnabled(
                                    source.source_id,
                                    false,
                                    false,
                                  ),
                                )
                            : void run(source, () =>
                                catalog.setStreamingSourceEnabled(
                                  source.source_id,
                                  true,
                                  false,
                                ),
                              )
                        }
                      >
                        {t(
                          source.enabled
                            ? "settings.streaming.disable"
                            : "settings.streaming.enable",
                        )}
                      </button>
                      <button
                        className={button}
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
                        className={button}
                        disabled={pending}
                        onClick={() => decide(source, "uninstall")}
                      >
                        {t("settings.streaming.uninstall")}
                      </button>
                    </>
                  ) : null}
                </>
              )}
            </div>
            {source.enabled &&
            source.runtime_ready &&
            source.state === "ready" ? (
              session?.signed_in && !session.expired ? (
                <button
                  className={button}
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
              ) : !!source.manifest?.authentication &&
                typeof source.manifest.authentication === "object" &&
                "flow" in source.manifest.authentication &&
                source.manifest.authentication.flow !== "none" ? (
                <StreamingSignIn sourceId={source.source_id} compact />
              ) : null
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
                className={button}
                disabled={!!busy}
                onClick={() => setDecision(null)}
              >
                {t("common.cancel")}
              </button>
              <button
                className={button}
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
    </div>
  );
}
