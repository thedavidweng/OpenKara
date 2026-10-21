import { spawnSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import { renderCargoSources } from "../scripts/generate-flatpak-cargo-sources.mjs";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));

function readProjectFile(path: string) {
  return readFileSync(join(projectRoot, path), "utf8");
}

type LockfilePackage = {
  filename: string;
  integrityHex: string;
  name: string;
  version: string;
};

type CargoLockfilePackage = {
  checksum: string;
  dest: string;
  name: string;
  version: string;
};

function splitPackageKey(key: string) {
  // pnpm wraps scoped keys in single quotes: '@scope/pkg@version'
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

function tarballFilename(name: string, version: string) {
  return `${name.replace("/", "__")}-${version}.tgz`;
}

function parsePnpmLockfilePackages(lockfile: string): LockfilePackage[] {
  const packages: LockfilePackage[] = [];
  let inPackagesSection = false;
  let currentKey: string | null = null;

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

function parseCargoLockfilePackages(lockfile: string): CargoLockfilePackage[] {
  const packages: CargoLockfilePackage[] = [];

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

    packages.push({
      checksum,
      dest: `cargo/vendor/${name}-${version}`,
      name,
      version,
    });
  }

  return packages;
}

describe("Flatpak packaging", () => {
  test.skipIf(process.platform === "win32")(
    "renders pinned channel runtimes and installs the executable and license",
    () => {
      const directory = mkdtempSync(
        join(tmpdir(), "openkara-flatpak-runtime-"),
      );
      try {
        const result = spawnSync(
          process.execPath,
          [
            "scripts/render-flatpak-manifest.mjs",
            "--version",
            "0.17.0",
            "--source-url",
            "data:application/octet-stream,source-fixture",
            "--output",
            directory,
          ],
          { cwd: projectRoot, encoding: "utf8" },
        );
        expect(result.status, result.stderr).toBe(0);
        const manifest = readFileSync(
          join(
            directory,
            "io.github.thedavidweng.OpenKara/io.github.thedavidweng.OpenKara.yml",
          ),
          "utf8",
        );
        const archives = Array.from(
          manifest.matchAll(
            /url: (https:\/\/nodejs\.org\/\S+)\n\s+sha256: (\w+)\n\s+dest: source-runtime\n\s+only-arches:\n\s+- (\S+)/g,
          ),
          (match) => ({ url: match[1], sha256: match[2], arch: match[3] }),
        );
        expect(archives).toEqual([
          {
            url: "https://nodejs.org/dist/v24.21.0/node-v24.21.0-linux-x64.tar.gz",
            sha256:
              "6e1db87ef58b8819e5d5402eff1536491b18edd8eb7bee5ef7897876e88dc5ff",
            arch: "x86_64",
          },
          {
            url: "https://nodejs.org/dist/v24.21.0/node-v24.21.0-linux-arm64.tar.gz",
            sha256:
              "724282c3b43aec998aa9527380465b45d229e021b58035f5f4f63095eabfe5d5",
            arch: "aarch64",
          },
        ]);
        mkdirSync(join(directory, "source-runtime/bin"), { recursive: true });
        writeFileSync(
          join(directory, "source-runtime/bin/node"),
          "runtime-fixture",
        );
        writeFileSync(
          join(directory, "source-runtime/LICENSE"),
          "license-fixture",
        );
        const commands = Array.from(
          manifest.matchAll(
            /^\s+- ((?:mkdir|install) .*source-runtime\/.*)$/gm,
          ),
          (match) => match[1],
        );
        expect(commands).toHaveLength(2);
        const destination = join(directory, "app");
        const install = spawnSync("sh", ["-ec", commands.join("\n")], {
          cwd: directory,
          env: { ...process.env, FLATPAK_DEST: destination },
          encoding: "utf8",
        });
        expect(install.status, install.stderr).toBe(0);
        expect(
          readFileSync(join(destination, "bin/openkara-source-node"), "utf8"),
        ).toBe("runtime-fixture");
        expect(
          readFileSync(
            join(
              destination,
              "share/licenses/io.github.thedavidweng.OpenKara/source-runtime/LICENSE",
            ),
            "utf8",
          ),
        ).toBe("license-fixture");
      } finally {
        rmSync(directory, { recursive: true, force: true });
      }
    },
  );

  test("keeps pnpm dependency sources in sync with the lockfile packages used by the app", () => {
    const lockfilePackages = parsePnpmLockfilePackages(
      readProjectFile("pnpm-lock.yaml"),
    );
    const nodeSources = JSON.parse(
      readProjectFile("packaging/flatpak/generated/node-sources.0.json"),
    ) as Array<{
      dest?: string;
      "dest-filename"?: string;
      sha512?: string;
      type: string;
      contents?: string;
    }>;
    const manifestSource = nodeSources.find(
      (source) => source["dest-filename"] === "pnpm-manifest.json",
    );

    expect(manifestSource).toBeDefined();
    expect(manifestSource?.dest).toBe("flatpak-node");

    const manifest = JSON.parse(manifestSource?.contents ?? "") as {
      packages: Record<
        string,
        { integrity_hex: string; name: string; version: string }
      >;
      store_version: string;
    };
    const sourceTarballs = new Map(
      nodeSources
        .filter(
          (source) =>
            source.type === "file" &&
            source.dest === "flatpak-node/pnpm-tarballs",
        )
        .map((source) => [source["dest-filename"], source.sha512]),
    );

    // pnpm 11 resolves from store-dir/v11; v10 left offline installs blind.
    expect(manifest.store_version).toBe("v11");
    expect(
      nodeSources.filter(
        (source) =>
          source.dest === "flatpak-node/cache/esbuild" ||
          source.dest?.startsWith("flatpak-node/cache/esbuild/"),
      ),
    ).toEqual([]);
    expect(Object.keys(manifest.packages).sort()).toEqual(
      lockfilePackages.map((pkg) => pkg.filename).sort(),
    );
    expect(Array.from(sourceTarballs.keys()).sort()).toEqual(
      lockfilePackages.map((pkg) => pkg.filename).sort(),
    );

    for (const pkg of lockfilePackages) {
      const manifestPackage = manifest.packages[pkg.filename];

      expect(manifestPackage).toEqual({
        integrity_hex: pkg.integrityHex,
        name: pkg.name,
        version: pkg.version,
      });
      expect(sourceTarballs.get(pkg.filename)).toBe(pkg.integrityHex);
    }
  });

  test("keeps Cargo dependency sources in sync with the lockfile packages used by the app", () => {
    const lockfilePackages = parseCargoLockfilePackages(
      readProjectFile("src-tauri/Cargo.lock"),
    );
    const cargoSources = JSON.parse(
      readProjectFile("packaging/flatpak/generated/cargo-sources.json"),
    ) as Array<{
      dest?: string;
      "dest-filename"?: string;
      sha256?: string;
      type: string;
      contents?: string;
    }>;
    const archives = new Map(
      cargoSources
        .filter(
          (source) =>
            source.type === "archive" &&
            source.dest?.startsWith("cargo/vendor/"),
        )
        .map((source) => [source.dest, source.sha256]),
    );
    const checksumFiles = new Map(
      cargoSources
        .filter(
          (source) =>
            source.type === "inline" &&
            source.dest?.startsWith("cargo/vendor/") &&
            source["dest-filename"] === ".cargo-checksum.json",
        )
        .map((source) => [
          source.dest,
          (JSON.parse(source.contents ?? "{}") as { package?: string }).package,
        ]),
    );

    expect(Array.from(archives.keys()).sort()).toEqual(
      lockfilePackages.map((pkg) => pkg.dest).sort(),
    );
    expect(Array.from(checksumFiles.keys()).sort()).toEqual(
      lockfilePackages.map((pkg) => pkg.dest).sort(),
    );

    for (const pkg of lockfilePackages) {
      expect(archives.get(pkg.dest)).toBe(pkg.checksum);
      expect(checksumFiles.get(pkg.dest)).toBe(pkg.checksum);
    }
  });

  test("regenerating Cargo sources produces a clean diff (no stale manifest)", () => {
    // Render the Cargo sources from the committed Cargo.lock using the pure
    // generator function and compare the text against the committed manifest.
    // This catches stale manifests after Cargo.lock changes.
    //
    // Non-destructive: we never write into the real checkout, so a mismatch
    // cannot discard a developer's intentional uncommitted generated-manifest
    // edit (the old test ran `git checkout -- cargo-sources.json` on failure).
    const lockfile = readProjectFile("src-tauri/Cargo.lock");
    const committed = readProjectFile(
      "packaging/flatpak/generated/cargo-sources.json",
    );

    const rendered = renderCargoSources(lockfile);

    expect(rendered).toBe(committed);
  });
});
