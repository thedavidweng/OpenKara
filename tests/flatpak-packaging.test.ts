import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

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
  test("targets current supported Flathub runtimes and both default architectures", () => {
    const manifestTemplate = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.yml.in",
    );

    expect(manifestTemplate).toContain('runtime-version: "50"');
    expect(manifestTemplate).toContain("org.freedesktop.Sdk.Extension.node24");
    expect(manifestTemplate).not.toContain("node20");
    expect(
      existsSync(join(projectRoot, "packaging/flatpak/flathub.json")),
    ).toBe(false);

    expect(manifestTemplate).toContain("x86_64");
    expect(manifestTemplate).toContain("aarch64");
  });

  test("uses pre-built ONNX Runtime binaries with architecture-specific sources for the Flathub submission", () => {
    const manifestTemplate = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.yml.in",
    );

    expect(manifestTemplate).toContain("onnxruntime-source");
    expect(manifestTemplate).toContain("onnxruntime-linux-x64");
    expect(manifestTemplate).toContain("onnxruntime-linux-aarch64");
    expect(manifestTemplate).toContain("${FLATPAK_DEST}/share/licenses");
    expect(manifestTemplate).not.toContain("build_shared_lib");
  });

  test("builds the Flatpak app binary without asking Tauri to create Linux bundles", () => {
    const manifestTemplate = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.yml.in",
    );

    expect(manifestTemplate).toContain("pnpm tauri build");
    expect(manifestTemplate).toContain("--no-bundle");
    expect(manifestTemplate).not.toContain("--bundles none");
  });

  test("ships a pnpm tarball matching the packageManager pin in package.json", () => {
    // The Flatpak build installs pnpm from a tarball archive source in the
    // manifest template. If this tarball drifts from the packageManager version
    // in package.json, the offline Flatpak build will install a pnpm that
    // cannot read the migrated pnpm-workspace.yaml or the lockfile.
    const manifestTemplate = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.yml.in",
    );
    const packageJson = JSON.parse(readProjectFile("package.json")) as {
      packageManager: string;
    };
    const pinnedVersion = packageJson.packageManager.replace(/^pnpm@/, "");

    expect(manifestTemplate).toContain(`pnpm/-/pnpm-${pinnedVersion}.tgz`);
  });

  test("uses the Flathub container image and official flatpak-builder action for Flatpak builds", () => {
    const packagingWorkflow = readProjectFile(
      ".github/workflows/packaging.yml",
    );

    // The build job must run inside the Flathub-maintained container image.
    // Running flatpak-builder directly on a GitHub hosted runner, or wrapping
    // `org.flatpak.Builder` manually, led to successive failures:
    //   1. bwrap could not find appstream-compose (missing from GNOME SDK 50).
    //   2. flatpak install aborted with "Cannot autolaunch D-Bus without X11
    //      $DISPLAY" on the session-bus-less runner.
    //   3. --install-deps-from=flathub failed because the sandbox's per-app
    //      flatpak dir did not have the flathub remote.
    //   4. Dependencies resolved for the Update phase were then reported as
    //      "not installed" during build-init due to nested sandbox visibility.
    // The supported Flatpak CI pattern is to run in the Flathub container and
    // use the official flatpak-builder action. Do not revert without reading
    // the history in commits touching this file.
    expect(packagingWorkflow).toContain(
      "image: ghcr.io/flathub-infra/flatpak-github-actions:gnome-50@sha256:",
    );
    expect(packagingWorkflow).toContain("options: --privileged");
    expect(packagingWorkflow).toMatch(
      /uses: flatpak\/flatpak-github-actions\/flatpak-builder@[a-f0-9]{40}/,
    );
    // Without this, `flatpak-builder-lint builddir/repo` fails with
    // "appstream-external-screenshot-url: Screenshots are not mirrored to
    // https://dl.flathub.org/media". The action forwards it as
    // --mirror-screenshots-url=<value> --compose-url-policy=full to
    // flatpak-builder and also commits the screenshots/<arch> OSTree ref,
    // which is what the linter's check_repo() looks for.
    expect(packagingWorkflow).toContain(
      "mirror-screenshots-url: https://dl.flathub.org/media",
    );
  });

  test("renders WinGet manifests with PowerShell environment syntax on Windows", () => {
    const packagingWorkflow = readProjectFile(
      ".github/workflows/packaging.yml",
    );

    expect(packagingWorkflow).toContain(
      'node scripts/render-winget-manifests.mjs --version "$env:RELEASE_VERSION" --output "$env:GITHUB_WORKSPACE/dist/winget"',
    );
    expect(packagingWorkflow).not.toContain(
      'node scripts/render-winget-manifests.mjs --version "${RELEASE_VERSION}"',
    );
  });

  test("includes generated dependency manifests instead of copying them as files", () => {
    const manifestTemplate = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.yml.in",
    );
    const renderScript = readProjectFile("scripts/render-flatpak-manifest.mjs");

    expect(manifestTemplate).toMatch(/\n\s+- cargo-sources\.json\n/);
    expect(manifestTemplate).not.toMatch(
      /type:\s*file\s*\n\s*path:\s*cargo-sources\.json/,
    );

    expect(renderScript).toMatch(/` {6}- \${file}`/);
    expect(renderScript).not.toContain(
      "`      - type: file\\n        path: ${file}`",
    );
  });

  test("keeps app metadata and Flatpak-only Tauri config in the upstream source archive", () => {
    const renderScript = readProjectFile("scripts/render-flatpak-manifest.mjs");
    const metainfo = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.metainfo.xml",
    );

    expect(
      existsSync(
        join(
          projectRoot,
          "packaging/flatpak/io.github.thedavidweng.OpenKara.metainfo.xml",
        ),
      ),
    ).toBe(true);
    expect(
      existsSync(
        join(
          projectRoot,
          "packaging/flatpak/io.github.thedavidweng.OpenKara.metainfo.xml.in",
        ),
      ),
    ).toBe(false);

    expect(renderScript).not.toContain(
      "io.github.thedavidweng.OpenKara.desktop",
    );
    expect(renderScript).not.toContain(
      "io.github.thedavidweng.OpenKara.metainfo.xml",
    );
    expect(renderScript).not.toContain("tauri.flatpak.conf.json");
    expect(renderScript).not.toContain("flathub.json");
    expect(metainfo).not.toContain("/main/packaging/flatpak/screenshots/");
    expect(metainfo).toContain("/v0.9.0/packaging/flatpak/screenshots/");
  });

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

    expect(manifest.store_version).toBe("v10");
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

  test("release automation never opens initial Flathub submission PRs automatically", () => {
    const releaseWorkflow = readProjectFile(".github/workflows/release.yml");

    expect(releaseWorkflow).toContain("GITHUB_TOKEN: ${{ github.token }}");
    expect(releaseWorkflow).toContain("Ensure release source tag exists");
    expect(releaseWorkflow).toContain("persist-credentials: false");
    expect(releaseWorkflow).toContain(
      'git -c "http.https://github.com/.extraheader=AUTHORIZATION: basic ${auth_header}"',
    );
    expect(releaseWorkflow).toContain('push origin "refs/tags/${tag}"');
    expect(releaseWorkflow).toContain(
      '--title "New version: ${WINGET_PACKAGE_IDENTIFIER} version ${VERSION}"',
    );
    expect(releaseWorkflow).not.toContain(
      "WinGet PR could not be created automatically.",
    );
    expect(releaseWorkflow).not.toContain("skipping WinGet PR automation");
    expect(releaseWorkflow).toContain("manual_submission_url=");
    expect(releaseWorkflow).toContain(
      "Initial Flathub submissions must be opened or updated manually",
    );
    expect(releaseWorkflow).not.toContain("--draft");
    expect(releaseWorkflow).not.toContain(
      "Open this prefilled GitHub URL to create the Flathub submission PR",
    );
    expect(releaseWorkflow).not.toContain("skipping Flathub PR automation");
  });
});
