import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { formatBytes } from "@/lib/format";
import { useBackend } from "@/lib/backend";
import { useCatalogStore } from "@/stores/catalog-store";
import type { MarketPackage, MarketUpdate, SourceSnapshot } from "@/types/ipc";
import { SourceActionDialog } from "./SourceActionDialog";
import {
  compareVersions,
  declaredOrigins,
  latestVersion,
  marketErrorCode,
  sourceActionButton,
} from "./source-market-utils";

const errorMessage = (failure: unknown) =>
  failure instanceof Error
    ? failure.message
    : typeof failure === "object" && failure !== null && "message" in failure
      ? String(failure.message)
      : String(failure);

const marketKey = (pkg: MarketPackage) =>
  `${pkg.registry_id}/${pkg.package_id}`;

export function AddedAccessReview({
  addedOrigins,
  addedCapabilities,
}: {
  addedOrigins: string[];
  addedCapabilities: string[];
}) {
  const { t } = useTranslation();
  return (
    <>
      {addedOrigins.length ? (
        <div>
          <h4 className="text-xs font-semibold">
            {t("settings.streaming.market.addedOrigins")}
          </h4>
          <ul className="text-xs">
            {addedOrigins.map((origin) => (
              <li key={origin} className="break-all">
                {origin}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      {addedCapabilities.length ? (
        <div>
          <h4 className="text-xs font-semibold">
            {t("settings.streaming.market.addedCapabilities")}
          </h4>
          <ul className="text-xs">
            {addedCapabilities.map((capability) => (
              <li key={capability}>{capability}</li>
            ))}
          </ul>
        </div>
      ) : null}
    </>
  );
}

interface ConsentTarget {
  pkg: MarketPackage;
  update: MarketUpdate;
}
interface ReplaceTarget {
  pkg: MarketPackage;
  current: SourceSnapshot | null;
}

export function SourceMarket() {
  const { t } = useTranslation();
  const { catalog } = useBackend();
  const market = useCatalogStore((state) => state.market);
  const status = useCatalogStore((state) => state.marketStatus);
  const loadError = useCatalogStore((state) => state.marketError);
  const registries = useCatalogStore((state) => state.registries);
  const registriesStatus = useCatalogStore((state) => state.registriesStatus);
  const sources = useCatalogStore((state) => state.sources);
  const [busy, setBusy] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [installTarget, setInstallTarget] = useState<MarketPackage | null>(
    null,
  );
  const [consentTarget, setConsentTarget] = useState<ConsentTarget | null>(
    null,
  );
  const [replaceTarget, setReplaceTarget] = useState<ReplaceTarget | null>(
    null,
  );

  useEffect(() => {
    if (status === "idle") void useCatalogStore.getState().loadMarket();
  }, [status]);

  const registryName = (registryId: string) =>
    registries.find((entry) => entry.registry_id === registryId)
      ?.display_name ?? registryId;

  const installedSnapshot = (pkg: MarketPackage) =>
    sources.find(
      (source) =>
        source.installed_version &&
        source.source_id === pkg.source_id &&
        source.registry_id === pkg.registry_id &&
        source.package_id === pkg.package_id,
    );

  const holderOf = (pkg: MarketPackage) =>
    sources.find(
      (source) =>
        source.installed_version &&
        source.source_id === pkg.source_id &&
        (source.registry_id !== pkg.registry_id ||
          source.package_id !== pkg.package_id),
    ) ?? null;

  const runInstall = async (
    pkg: MarketPackage,
    options: {
      version?: string | null;
      consent?: boolean;
      replace?: boolean;
    } = {},
  ) => {
    const { version = null, consent = false, replace = false } = options;
    const key = marketKey(pkg);
    setBusy(key);
    setErrors((old) => ({ ...old, [key]: "" }));
    try {
      await catalog.installMarketPackage(
        pkg.registry_id,
        pkg.package_id,
        version,
        consent,
        replace,
      );
      const store = useCatalogStore.getState();
      store.applySources(await catalog.listStreamingSources());
      store.applyMarket(await catalog.listSourceMarket());
    } catch (failure) {
      const code = marketErrorCode(failure);
      if (code === "source_id_taken" && !replace) {
        setReplaceTarget({ pkg, current: holderOf(pkg) });
        return;
      }
      if (code === "consent_required" && !consent && pkg.update) {
        setConsentTarget({ pkg, update: pkg.update });
        return;
      }
      setErrors((old) => ({ ...old, [key]: errorMessage(failure) }));
    } finally {
      setBusy(null);
    }
  };

  const requestInstall = (pkg: MarketPackage) => {
    const holder = holderOf(pkg);
    if (holder) setReplaceTarget({ pkg, current: holder });
    else setInstallTarget(pkg);
  };

  const requestUpdate = (pkg: MarketPackage, update: MarketUpdate) => {
    if (update.requires_consent) setConsentTarget({ pkg, update });
    else void runInstall(pkg, { version: update.version });
  };

  const confirmInstall = async () => {
    const target = installTarget;
    if (!target) return;
    await runInstall(target);
    setInstallTarget(null);
  };

  const confirmConsent = async () => {
    const target = consentTarget;
    if (!target) return;
    await runInstall(target.pkg, {
      version: target.update.version,
      consent: true,
    });
    setConsentTarget(null);
  };

  const confirmReplace = async () => {
    const target = replaceTarget;
    if (!target) return;
    await runInstall(target.pkg, { replace: true });
    setReplaceTarget(null);
  };

  return (
    <section aria-labelledby="source-market-title" className="space-y-3">
      <h3 id="source-market-title" className="text-sm font-semibold">
        {t("settings.streaming.market.title")}
      </h3>
      {loadError ? (
        <p role="alert" className="text-xs">
          {loadError}
        </p>
      ) : null}
      {status === "loading" ? <p role="status">{t("common.loading")}</p> : null}
      {status === "ready" && market.length === 0 ? (
        <p className="text-xs text-[var(--color-text-dim)]">
          {t(
            registriesStatus === "ready" && registries.length === 0
              ? "settings.streaming.market.emptyNoRegistries"
              : "settings.streaming.market.empty",
          )}
        </p>
      ) : null}
      {market.map((pkg) => {
        const key = marketKey(pkg);
        const pending = busy === key;
        const latest = latestVersion(pkg.versions);
        const snapshot = installedSnapshot(pkg);
        const origins = latest ? declaredOrigins(latest) : [];
        const versions = [...pkg.versions].sort((a, b) =>
          compareVersions(b.version, a.version),
        );
        return (
          <article
            key={key}
            aria-label={pkg.name}
            className="space-y-3 rounded-xl border border-[var(--color-border)] p-4"
          >
            <header>
              <h4 className="font-semibold">{pkg.name}</h4>
              <p className="text-xs text-[var(--color-text-dim)]">
                {pkg.description}
              </p>
            </header>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
              <dt>{t("settings.streaming.market.registry")}</dt>
              <dd>{registryName(pkg.registry_id)}</dd>
              <dt>{t("settings.streaming.market.packageId")}</dt>
              <dd>{pkg.package_id}</dd>
              <dt>{t("settings.streaming.market.sourceId")}</dt>
              <dd>{pkg.source_id}</dd>
              <dt>{t("settings.streaming.license")}</dt>
              <dd>{pkg.license}</dd>
              {latest ? (
                <>
                  <dt>{t("settings.streaming.market.version")}</dt>
                  <dd>
                    {latest.version} · {formatBytes(latest.size)}
                  </dd>
                  <dt>{t("settings.streaming.market.capabilities")}</dt>
                  <dd>{latest.capabilities.join(", ") || "—"}</dd>
                  <dt>{t("settings.streaming.market.origins")}</dt>
                  <dd>
                    {origins.length ? (
                      <ul>
                        {origins.map((origin) => (
                          <li key={origin} className="break-all">
                            {origin}
                          </li>
                        ))}
                      </ul>
                    ) : (
                      t("settings.streaming.market.noOrigins")
                    )}
                  </dd>
                </>
              ) : null}
              {pkg.installed ? (
                <>
                  <dt>{t("settings.streaming.market.installed")}</dt>
                  <dd>
                    {pkg.installed.version}
                    {snapshot
                      ? ` · ${t(`settings.streaming.states.${snapshot.state}`)}`
                      : ""}
                    {pkg.installed.pinned_version
                      ? ` · ${t("settings.streaming.market.pinned", { version: pkg.installed.pinned_version })}`
                      : ""}
                  </dd>
                </>
              ) : null}
            </dl>
            {versions.length > 1 ? (
              <details className="text-xs">
                <summary>{t("settings.streaming.market.versions")}</summary>
                <ul>
                  {versions.map((version) => (
                    <li key={version.version}>
                      {version.version} · {formatBytes(version.size)}
                    </li>
                  ))}
                </ul>
              </details>
            ) : null}
            {errors[key] ? (
              <p role="alert" className="text-xs">
                {errors[key]}
              </p>
            ) : null}
            <div className="flex flex-wrap gap-2">
              {!pkg.installed ? (
                <button
                  className={sourceActionButton}
                  disabled={pending}
                  onClick={() => requestInstall(pkg)}
                >
                  {t("settings.streaming.market.install")}
                </button>
              ) : null}
              {pkg.update ? (
                <button
                  className={sourceActionButton}
                  disabled={pending}
                  onClick={() => pkg.update && requestUpdate(pkg, pkg.update)}
                >
                  {t("settings.streaming.market.updateTo", {
                    version: pkg.update.version,
                  })}
                </button>
              ) : null}
            </div>
          </article>
        );
      })}
      {installTarget
        ? (() => {
            const latest = latestVersion(installTarget.versions);
            const origins = latest ? declaredOrigins(latest) : [];
            return (
              <SourceActionDialog
                title={t("settings.streaming.market.installTitle", {
                  name: installTarget.name,
                })}
                confirmLabel={t("settings.streaming.market.installConfirm")}
                busy={busy !== null}
                onConfirm={() => void confirmInstall()}
                onCancel={() => setInstallTarget(null)}
              >
                <p>{t("settings.streaming.market.installBody")}</p>
                {latest ? (
                  <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
                    <dt>{t("settings.streaming.market.version")}</dt>
                    <dd>
                      {latest.version} · {formatBytes(latest.size)}
                    </dd>
                    <dt>{t("settings.streaming.market.capabilities")}</dt>
                    <dd>{latest.capabilities.join(", ") || "—"}</dd>
                    <dt>{t("settings.streaming.market.origins")}</dt>
                    <dd>
                      {origins.length ? (
                        <ul>
                          {origins.map((origin) => (
                            <li key={origin} className="break-all">
                              {origin}
                            </li>
                          ))}
                        </ul>
                      ) : (
                        t("settings.streaming.market.noOrigins")
                      )}
                    </dd>
                  </dl>
                ) : null}
              </SourceActionDialog>
            );
          })()
        : null}
      {consentTarget ? (
        <SourceActionDialog
          title={t("settings.streaming.market.consentTitle")}
          confirmLabel={t("settings.streaming.market.consentConfirm")}
          busy={busy !== null}
          onConfirm={() => void confirmConsent()}
          onCancel={() => setConsentTarget(null)}
        >
          <p>
            {t("settings.streaming.market.consentBody", {
              name: consentTarget.pkg.name,
              version: consentTarget.update.version,
            })}
          </p>
          <AddedAccessReview
            addedOrigins={consentTarget.update.added_origins}
            addedCapabilities={consentTarget.update.added_capabilities}
          />
        </SourceActionDialog>
      ) : null}
      {replaceTarget ? (
        <SourceActionDialog
          title={t("settings.streaming.market.replaceTitle", {
            sourceId: replaceTarget.pkg.source_id,
          })}
          confirmLabel={t("settings.streaming.market.replaceConfirm")}
          busy={busy !== null}
          onConfirm={() => void confirmReplace()}
          onCancel={() => setReplaceTarget(null)}
        >
          <p>
            {t("settings.streaming.market.replaceBody", {
              current:
                replaceTarget.current?.name ?? replaceTarget.pkg.source_id,
              name: replaceTarget.pkg.name,
              sourceId: replaceTarget.pkg.source_id,
            })}
          </p>
        </SourceActionDialog>
      ) : null}
    </section>
  );
}
