/**
 * Point every packages: resolution at a local file: tarball so
 * `pnpm install --offline` uses the localTarball fetcher instead of
 * registry.npmjs.org (which is unreachable inside the Flatpak sandbox).
 *
 * flatpak-builder already downloads those tarballs into
 * flatpak-node/pnpm-tarballs/ via node-sources; this only rewrites the
 * lockfile in the build directory (not the committed lockfile).
 */
import fs from "node:fs";

const lockPath = process.argv[2] ?? "pnpm-lock.yaml";
const tarballDir = process.argv[3] ?? "flatpak-node/pnpm-tarballs";

function splitPackageKey(key) {
  const unquoted = key.replace(/^'|'$/g, "");
  const versionSeparator = unquoted.lastIndexOf("@");
  if (versionSeparator <= 0) {
    throw new Error(`Invalid pnpm lockfile package key: ${key}`);
  }
  return {
    name: unquoted.slice(0, versionSeparator),
    version: unquoted.slice(versionSeparator + 1),
  };
}

function tarballFilename(name, version) {
  return `${name.replace("/", "__")}-${version}.tgz`;
}

const lockfile = fs.readFileSync(lockPath, "utf8");
const lines = lockfile.split(/\r?\n/);
let inPackages = false;
let currentKey = null;
let rewritten = 0;
const missing = [];

const out = lines.map((line) => {
  if (line === "packages:") {
    inPackages = true;
    currentKey = null;
    return line;
  }
  if (inPackages && /^[a-zA-Z]/.test(line)) {
    inPackages = false;
    currentKey = null;
    return line;
  }
  if (!inPackages) return line;

  const packageKeyMatch = line.match(
    /^ {2}(?:(?:"([^"]+)")|('([^']+)')|([^\s:#][^:#]*)):\s*$/,
  );
  if (packageKeyMatch) {
    currentKey =
      packageKeyMatch[1] ?? packageKeyMatch[3] ?? packageKeyMatch[4] ?? null;
    return line;
  }

  if (currentKey == null) return line;

  const resolutionMatch = line.match(
    /^( {4}resolution: \{integrity: (sha512-[A-Za-z0-9+/=]+)\})\s*$/,
  );
  if (!resolutionMatch) return line;

  const { name, version } = splitPackageKey(currentKey);
  const filename = tarballFilename(name, version);
  const rel = `${tarballDir}/${filename}`;
  if (!fs.existsSync(rel)) {
    missing.push(rel);
  }
  rewritten += 1;
  currentKey = null;
  // file: is resolved relative to the lockfile directory (project root).
  return `    resolution: {integrity: ${resolutionMatch[2]}, tarball: file:${rel}}`;
});

if (missing.length > 0) {
  console.error(
    `rewrite_lockfile_local_tarballs: missing ${missing.length} tarball(s), e.g. ${missing[0]}`,
  );
  process.exit(1);
}

fs.writeFileSync(lockPath, `${out.join("\n")}`);
console.log(
  `rewrote ${rewritten} package resolutions to file:${tarballDir}/… tarballs`,
);
