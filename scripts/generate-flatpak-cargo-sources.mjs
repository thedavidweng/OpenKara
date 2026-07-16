import { readFileSync, writeFileSync } from "node:fs";

/**
 * Regenerates Flatpak offline Cargo dependency sources from `src-tauri/Cargo.lock`.
 *
 * Mirrors the output of flatpak-builder-tools' `flatpak-cargo-generator.py` so
 * the Flatpak build can vendor all registry crates without network access.
 *
 * - **Input:** `src-tauri/Cargo.lock`
 * - **Output:** `packaging/flatpak/generated/cargo-sources.json`
 * - **Run:** `node scripts/generate-flatpak-cargo-sources.mjs` or
 *   `pnpm generate:flatpak-cargo-sources`
 * - **When to run:** after changing Rust dependencies or `Cargo.lock` entries
 *   used by Flatpak packaging
 * - **Idempotent:** two consecutive runs produce zero diff in the output file
 */
const lockfilePath = "src-tauri/Cargo.lock";
const cargoSourcesPath = "packaging/flatpak/generated/cargo-sources.json";

/**
 * @typedef {Object} CargoLockPackage
 * @property {string} name
 * @property {string} version
 * @property {string} checksum
 */

/**
 * @param {string} lockfile
 * @returns {CargoLockPackage[]}
 */
function parseCargoLockfile(lockfile) {
  const packages = [];

  for (const packageBlock of lockfile.split(/\n\[\[package\]\]\n/)) {
    const name = packageBlock.match(/(?:^|\n)name = "([^"]+)"/)?.[1];
    const version = packageBlock.match(/(?:^|\n)version = "([^"]+)"/)?.[1];
    const source = packageBlock.match(/(?:^|\n)source = "([^"]+)"/)?.[1];
    const checksum = packageBlock.match(/(?:^|\n)checksum = "([^"]+)"/)?.[1];

    if (
      name === undefined ||
      version === undefined ||
      checksum === undefined ||
      !source?.startsWith("registry+")
    ) {
      continue;
    }

    packages.push({ name, version, checksum });
  }

  // Deterministic order: sort by name, then version.
  packages.sort((a, b) =>
    a.name === b.name
      ? a.version.localeCompare(b.version, undefined, { numeric: true })
      : a.name.localeCompare(b.name),
  );

  return packages;
}

/**
 * @param {CargoLockPackage[]} packages
 * @returns {unknown[]}
 */
function generateCargoSources(packages) {
  const sources = [];

  for (const pkg of packages) {
    const dest = `cargo/vendor/${pkg.name}-${pkg.version}`;
    const url = `https://static.crates.io/crates/${pkg.name}/${pkg.name}-${pkg.version}.crate`;

    sources.push({
      type: "archive",
      "archive-type": "tar-gzip",
      url,
      sha256: pkg.checksum,
      dest,
    });

    sources.push({
      type: "inline",
      contents: JSON.stringify({
        package: pkg.checksum,
        files: {},
      }),
      dest,
      "dest-filename": ".cargo-checksum.json",
    });
  }

  // Vendored sources config — must be the last entry.
  sources.push({
    type: "inline",
    contents:
      '[source.vendored-sources]\ndirectory = "cargo/vendor"\n\n[source.crates-io]\nreplace-with = "vendored-sources"\n',
    dest: "cargo",
    "dest-filename": "config",
  });

  return sources;
}

const lockfile = readFileSync(lockfilePath, "utf8");
const packages = parseCargoLockfile(lockfile);
const sources = generateCargoSources(packages);

writeFileSync(cargoSourcesPath, `${JSON.stringify(sources, null, 2)}\n`);
console.log(`Generated ${cargoSourcesPath} with ${packages.length} crates`);
