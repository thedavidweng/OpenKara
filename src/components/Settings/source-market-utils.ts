import { buttonVariants } from "@/components/ui/button-variants";
import type {
  MarketPackage,
  MarketVersion,
  RegistryStatus,
  SourceSnapshot,
} from "@/types/ipc";

export const sourceActionButton = buttonVariants({ variant: "outline" });
export const sourcePrimaryButton = buttonVariants();
export const sourceRow =
  "space-y-2 rounded-xl border border-border bg-card p-4";
export const sourceDetails =
  "text-[12px] [&_summary]:cursor-pointer [&_summary]:text-muted-foreground [&_summary]:select-none";

export function isInstalledSource(source: SourceSnapshot): boolean {
  return (
    !!source.installed_version ||
    source.state === "installing" ||
    source.state === "cancelling" ||
    source.state === "update_required"
  );
}

export function isOfficialRegistry(
  registries: RegistryStatus[],
  registryId: string | null,
): boolean {
  return registries.some(
    (entry) => entry.official && entry.registry_id === registryId,
  );
}

/** The readable part of a failure; host messages start with their error code. */
export function errorMessage(failure: unknown): string {
  const message =
    failure instanceof Error
      ? failure.message
      : typeof failure === "object" && failure !== null && "message" in failure
        ? String(failure.message)
        : String(failure);
  return message.replace(/^[a-z_]+: /, "");
}

function versionParts(version: string): {
  core: number[];
  pre: string[] | null;
} {
  const [core, pre] = version.split("-", 2);
  return {
    core: core.split(".").map((part) => Number(part) || 0),
    pre: pre === undefined ? null : pre.split("."),
  };
}

/** SemVer precedence: negative when `a` is older than `b`. */
export function compareVersions(a: string, b: string): number {
  const left = versionParts(a);
  const right = versionParts(b);
  for (let index = 0; index < 3; index += 1) {
    const difference = (left.core[index] ?? 0) - (right.core[index] ?? 0);
    if (difference !== 0) return difference < 0 ? -1 : 1;
  }
  if (left.pre === null && right.pre === null) return 0;
  if (left.pre === null) return 1;
  if (right.pre === null) return -1;
  const length = Math.max(left.pre.length, right.pre.length);
  for (let index = 0; index < length; index += 1) {
    const l = left.pre[index];
    const r = right.pre[index];
    if (l === undefined) return -1;
    if (r === undefined) return 1;
    if (l === r) continue;
    const lNumeric = /^\d+$/.test(l);
    const rNumeric = /^\d+$/.test(r);
    if (lNumeric && rNumeric) return Number(l) < Number(r) ? -1 : 1;
    if (lNumeric) return -1;
    if (rNumeric) return 1;
    return l < r ? -1 : 1;
  }
  return 0;
}

export function latestVersion(versions: MarketVersion[]): MarketVersion | null {
  let latest: MarketVersion | null = null;
  for (const version of versions)
    if (!latest || compareVersions(version.version, latest.version) > 0)
      latest = version;
  return latest;
}

export function declaredOrigins(version: MarketVersion): string[] {
  return [...new Set([...version.resource_origins, ...version.media_origins])];
}

const RECOGNIZED_HOST_CODES = [
  "consent_required",
  "source_id_taken",
  "invalid_registry",
] as const;
export type MarketErrorCode = (typeof RECOGNIZED_HOST_CODES)[number];

/**
 * Reads the host error code from either the mock shape (`code` carries the
 * host code) or the wire shape (`code` is the generic enum and the message is
 * prefixed with `<host_code>: `).
 */
export function marketErrorCode(error: unknown): MarketErrorCode | null {
  if (typeof error !== "object" || error === null) return null;
  const record = error as Record<string, unknown>;
  const candidates = [
    typeof record.code === "string" ? record.code : "",
    typeof record.message === "string"
      ? record.message.split(":", 1)[0].trim()
      : "",
  ];
  for (const candidate of candidates)
    if ((RECOGNIZED_HOST_CODES as readonly string[]).includes(candidate))
      return candidate as MarketErrorCode;
  return null;
}

/** The newest version of a package that this OpenKara can install. */
export function installableVersion(pkg: MarketPackage): MarketVersion | null {
  return (
    pkg.versions.find(
      (version) => version.version === pkg.installable_version,
    ) ?? null
  );
}
