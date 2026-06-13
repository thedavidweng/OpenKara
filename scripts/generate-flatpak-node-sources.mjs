import { readFileSync, writeFileSync } from "node:fs";

const lockfilePath = "pnpm-lock.yaml";
const nodeSourcesPath = "packaging/flatpak/generated/node-sources.0.json";
const PNPM_TARBALL_DEST = "flatpak-node/pnpm-tarballs";
const PNPM_MANIFEST_FILENAME = "pnpm-manifest.json";

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

function tarballUrl(name, version) {
  if (name.startsWith("@")) {
    const [scope, packageName] = name.split("/");
    return `https://registry.npmjs.org/${scope}/${packageName}/-/${packageName}-${version}.tgz`;
  }

  return `https://registry.npmjs.org/${name}/-/${name}-${version}.tgz`;
}

function parsePnpmLockfilePackages(lockfile) {
  const packages = [];
  let inPackagesSection = false;
  let currentKey = null;

  for (const line of lockfile.split(/\r?\n/)) {
    if (line === "packages:") {
      inPackagesSection = true;
      continue;
    }

    if (inPackagesSection && /^[a-zA-Z].*:/.test(line)) {
      break;
    }

    if (!inPackagesSection) {
      continue;
    }

    const packageKeyMatch = line.match(
      /^ {2}(?:(?:"([^"]+)")|([^\s:#][^:#]*)):\s*$/,
    );

    if (packageKeyMatch) {
      currentKey = packageKeyMatch[1] ?? packageKeyMatch[2] ?? null;
      continue;
    }

    if (currentKey === null) {
      continue;
    }

    const integrityMatch = line.match(/integrity:\s*sha512-([^,}\s]+)/);

    if (!integrityMatch) {
      continue;
    }

    const { name, version } = splitPackageKey(currentKey);
    const integrityHex = Buffer.from(integrityMatch[1], "base64").toString(
      "hex",
    );

    packages.push({
      filename: tarballFilename(name, version),
      integrityHex,
      name,
      version,
    });
  }

  return packages;
}

const lockfilePackages = parsePnpmLockfilePackages(
  readFileSync(lockfilePath, "utf8"),
);
const existingSources = JSON.parse(readFileSync(nodeSourcesPath, "utf8"));
const generatedDependencySources = lockfilePackages.map((pkg) => ({
  type: "file",
  url: tarballUrl(pkg.name, pkg.version),
  sha512: pkg.integrityHex,
  "dest-filename": pkg.filename,
  dest: PNPM_TARBALL_DEST,
}));
const manifestSource = {
  type: "inline",
  "dest-filename": PNPM_MANIFEST_FILENAME,
  dest: "flatpak-node",
  contents: JSON.stringify(
    {
      store_version: "v10",
      packages: Object.fromEntries(
        lockfilePackages.map((pkg) => [
          pkg.filename,
          {
            integrity_hex: pkg.integrityHex,
            name: pkg.name,
            version: pkg.version,
          },
        ]),
      ),
    },
    null,
    2,
  ),
};
const preservedSources = existingSources.filter(
  (source) =>
    source.dest !== PNPM_TARBALL_DEST &&
    source.dest !== "flatpak-node/cache/esbuild" &&
    !source.dest?.startsWith("flatpak-node/cache/esbuild/") &&
    source["dest-filename"] !== PNPM_MANIFEST_FILENAME,
);
const nextSources = [
  ...preservedSources.slice(0, 1),
  ...generatedDependencySources,
  manifestSource,
  ...preservedSources.slice(1),
];

writeFileSync(nodeSourcesPath, `${JSON.stringify(nextSources, null, 2)}\n`);
