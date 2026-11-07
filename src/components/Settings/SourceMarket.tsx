import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { formatBytes } from "@/lib/format";
import { useBackend } from "@/lib/backend";
import { useCatalogStore } from "@/stores/catalog-store";
import type { MarketPackage, SourceSnapshot } from "@/types/ipc";
import { SourceActionDialog } from "./SourceActionDialog";
import { OfficialBadge } from "./OfficialBadge";
import {
  compareVersions,
  declaredOrigins,
  errorMessage,
  isOfficialRegistry,
  installableVersion,
  latestVersion,
  marketErrorCode,
  sourceDetails,
  sourcePrimaryButton,
  sourceRow,
} from "./source-market-utils";

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
  const [replaceTarget, setReplaceTarget] = useState<ReplaceTarget | null>(
    null,
  );

  useEffect(() => {
    if (status === "idle") void useCatalogStore.getState().loadMarket();
  }, [status]);

  const registryName = (registryId: string) =>
    registries.find((entry) => entry.registry_id === registryId)
      ?.display_name ?? registryId;

  const holderOf = (pkg: MarketPackage) =>
    sources.find(
      (source) =>
        source.installed_version &&
        source.source_id === pkg.source_id &&
        (source.registry_id !== pkg.registry_id ||
          source.package_id !== pkg.package_id),
    ) ?? null;

  const runInstall = async (pkg: MarketPackage, replace = false) => {
    const key = marketKey(pkg);
    setBusy(key);
    setErrors((old) => ({ ...old, [key]: "" }));
    try {
      await catalog.installMarketPackage(
        pkg.registry_id,
        pkg.package_id,
        null,
        false,
        replace,
      );
      // Confirming the install review is consent to run the source. A
      // replacement stays disabled, as its confirmation states.
      if (!replace)
        await catalog.setStreamingSourceEnabled(pkg.source_id, true, false);
      const store = useCatalogStore.getState();
      store.applySources(await catalog.listStreamingSources());
      store.applyMarket(await catalog.listSourceMarket());
    } catch (failure) {
      const code = marketErrorCode(failure);
      if (code === "source_id_taken" && !replace) {
        setReplaceTarget({ pkg, current: holderOf(pkg) });
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

  const confirmInstall = async () => {
    const target = installTarget;
    if (!target) return;
    await runInstall(target);
    setInstallTarget(null);
  };

  const confirmReplace = async () => {
    const target = replaceTarget;
    if (!target) return;
    await runInstall(target.pkg, true);
    setReplaceTarget(null);
  };

  const available = market.filter((pkg) => !pkg.installed);
  return (
    <div className="space-y-3">
      {loadError ? (
        <p role="alert" className="text-xs">
          {loadError}
        </p>
      ) : null}
      {status === "loading" ? <p role="status">{t("common.loading")}</p> : null}
      {status === "ready" && available.length === 0 ? (
        <p className="text-xs text-[var(--color-text-dim)]">
          {t(
            registriesStatus === "ready" && registries.length === 0
              ? "settings.streaming.market.emptyNoRegistries"
              : "settings.streaming.market.empty",
          )}
        </p>
      ) : null}
      {available.map((pkg) => {
        const key = marketKey(pkg);
        const pending = busy === key;
        const installable = installableVersion(pkg);
        const latest = installable ?? latestVersion(pkg.versions);
        const origins = latest ? declaredOrigins(latest) : [];
        const official = isOfficialRegistry(registries, pkg.registry_id);
        const versions = [...pkg.versions].sort((a, b) =>
          compareVersions(b.version, a.version),
        );
        return (
          <article key={key} aria-label={pkg.name} className={sourceRow}>
            <div className="flex flex-wrap items-start justify-between gap-3">
              <header className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <h4 className="font-semibold">{pkg.name}</h4>
                  {official ? <OfficialBadge /> : null}
                </div>
                <p className="text-xs text-[var(--color-text-dim)]">
                  {pkg.description}
                </p>
                <p className="text-xs text-[var(--color-text-dim)]">
                  {[
                    latest
                      ? `${latest.version} · ${formatBytes(latest.size)}`
                      : null,
                    official ? null : registryName(pkg.registry_id),
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              </header>
              {installable ? (
                <button
                  className={sourcePrimaryButton}
                  disabled={pending}
                  onClick={() => requestInstall(pkg)}
                >
                  {t(
                    holderOf(pkg)
                      ? "settings.streaming.market.replace"
                      : "settings.streaming.market.install",
                  )}
                </button>
              ) : (
                <p className="max-w-48 text-right text-xs text-[var(--color-text-dim)]">
                  {pkg.required_host_version
                    ? t("settings.streaming.market.needsHost", {
                        version: pkg.required_host_version,
                      })
                    : t("settings.streaming.market.incompatible")}
                </p>
              )}
            </div>
            {errors[key] ? (
              <p role="alert" className="text-xs">
                {errors[key]}
              </p>
            ) : null}
            <details className={sourceDetails}>
              <summary>{t("settings.streaming.details")}</summary>
              <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
                <dt>{t("settings.streaming.market.registry")}</dt>
                <dd>{registryName(pkg.registry_id)}</dd>
                <dt>{t("settings.streaming.market.packageId")}</dt>
                <dd className="break-all">{pkg.package_id}</dd>
                <dt>{t("settings.streaming.license")}</dt>
                <dd>{pkg.license}</dd>
                {latest ? (
                  <>
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
                {versions.length > 1 ? (
                  <>
                    <dt>{t("settings.streaming.market.versions")}</dt>
                    <dd>
                      {versions.map((version) => version.version).join(", ")}
                    </dd>
                  </>
                ) : null}
              </dl>
            </details>
          </article>
        );
      })}
      {installTarget
        ? (() => {
            const latest = installableVersion(installTarget);
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
      {replaceTarget ? (
        <SourceActionDialog
          title={t("settings.streaming.market.replaceTitle", {
            name: replaceTarget.pkg.name,
          })}
          confirmLabel={t("settings.streaming.market.replaceConfirm")}
          busy={busy !== null}
          onConfirm={() => void confirmReplace()}
          onCancel={() => setReplaceTarget(null)}
        >
          <p>
            {t("settings.streaming.market.replaceBody", {
              current: replaceTarget.current?.name ?? replaceTarget.pkg.name,
              currentRegistry: replaceTarget.current?.registry_id
                ? registryName(replaceTarget.current.registry_id)
                : "OpenKara",
              registry: registryName(replaceTarget.pkg.registry_id),
            })}
          </p>
        </SourceActionDialog>
      ) : null}
    </div>
  );
}
