import type { MarketVersion } from "@/types/ipc";

export const sourceActionButton =
  "min-h-9 rounded-full border border-[var(--color-border)] px-4 text-xs focus-visible:outline-2 disabled:opacity-50";

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
