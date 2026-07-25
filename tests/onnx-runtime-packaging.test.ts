import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));

function readProjectFile(path: string) {
  return readFileSync(join(projectRoot, path), "utf8");
}

function readCargoLockPackages() {
  const cargoLock = readProjectFile("src-tauri/Cargo.lock");
  return [
    ...cargoLock.matchAll(
      /\[\[package\]\]\n([\s\S]*?)(?=\n\[\[package\]\]|\s*$)/g,
    ),
  ]
    .map(([, packageBlock]) => {
      const name = packageBlock.match(/^name = "([^"]+)"/m)?.[1];
      const version = packageBlock.match(/^version = "([^"]+)"/m)?.[1];
      const checksum = packageBlock.match(/^checksum = "([^"]+)"/m)?.[1];

      return name && version ? { name, version, checksum } : null;
    })
    .filter((cargoPackage) => cargoPackage !== null);
}

describe("ONNX Runtime packaging", () => {
  test("stages runtimes from the catalog snapshot without hardcoded pins", () => {
    const prepareScript = readProjectFile("scripts/prepare-onnx-runtime.mjs");

    // The catalog snapshot is the single source for runtime identity — the
    // prepare script must carry no version, URL, or digest constants.
    expect(prepareScript).toContain("src-tauri");
    expect(prepareScript).toContain("release-manifest.json");
    expect(prepareScript).not.toContain("microsoft/onnxruntime/releases");
    expect(prepareScript).not.toContain("api/v2/package");
    expect(prepareScript).not.toMatch(/const ORT_VERSION =/);

    // Windows ships the DirectML companion inside the catalog artifact.
    const catalog = JSON.parse(
      readProjectFile("src-tauri/catalog/release-manifest.json"),
    );
    const windowsRuntime = catalog.artifacts.runtimes.find(
      (runtime: { target_triple: string | null }) =>
        runtime.target_triple === "x86_64-pc-windows-msvc",
    );
    expect(windowsRuntime).toBeDefined();
    expect(windowsRuntime.runtime.execution_providers).toContain("directml");
    expect(Object.keys(windowsRuntime.extracted_file_digests)).toContain(
      "DirectML.dll",
    );
    expect(windowsRuntime.runtime.companion_files).toContain("DirectML.dll");
  });

  test("keeps Flatpak ONNX Runtime sources aligned with the catalog snapshot", () => {
    const manifestTemplate = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.yml.in",
    );
    const renderer = readProjectFile("scripts/render-flatpak-manifest.mjs");

    // The template must reference runtimes only through renderer
    // placeholders, and the renderer must resolve them from the catalog.
    for (const token of [
      "@@ORT_VERSION@@",
      "@@ORT_X64_URL@@",
      "@@ORT_X64_SHA256@@",
      "@@ORT_ARM64_URL@@",
      "@@ORT_ARM64_SHA256@@",
    ]) {
      expect(manifestTemplate).toContain(token);
      expect(renderer).toContain(token);
    }
    expect(manifestTemplate).not.toContain("microsoft/onnxruntime/releases");
    expect(renderer).toContain("release-manifest.json");

    const catalog = JSON.parse(
      readProjectFile("src-tauri/catalog/release-manifest.json"),
    );
    const linuxTargets = [
      "x86_64-unknown-linux-gnu",
      "aarch64-unknown-linux-gnu",
    ];
    for (const target of linuxTargets) {
      const runtime = catalog.artifacts.runtimes.find(
        (candidate: { target_triple: string | null }) =>
          candidate.target_triple === target,
      );
      expect(runtime).toBeDefined();
      expect(Object.keys(runtime.extracted_file_digests)).toContain(
        "libonnxruntime.so",
      );
    }
  });

  test("keeps Flatpak Cargo vendor sources aligned with Cargo.lock", () => {
    const cargoSources = JSON.parse(
      readProjectFile("packaging/flatpak/generated/cargo-sources.json"),
    );
    const vendorDestinations = new Map(
      cargoSources
        .map(
          (source: { dest?: string; sha256?: string; contents?: string }) => [
            source.dest?.replace(/^cargo\/vendor\//, ""),
            source,
          ],
        )
        .filter(
          ([destination, source]: [string | undefined, { sha256?: string }]) =>
            destination && source.sha256,
        ),
    );

    for (const cargoPackage of readCargoLockPackages()) {
      if (!cargoPackage.checksum) {
        continue;
      }

      const destination = `${cargoPackage.name}-${cargoPackage.version}`;
      const archiveSource = vendorDestinations.get(destination) as
        | { sha256?: string }
        | undefined;

      expect(archiveSource, destination).toBeDefined();
      expect(archiveSource?.sha256, destination).toBe(cargoPackage.checksum);
    }
  });

  test("uses Cargo config.toml for Flatpak vendor configuration", () => {
    const cargoSources = JSON.parse(
      readProjectFile("packaging/flatpak/generated/cargo-sources.json"),
    );

    expect(cargoSources).toContainEqual(
      expect.objectContaining({
        dest: "cargo",
        "dest-filename": "config",
      }),
    );
    expect(cargoSources).not.toContainEqual(
      expect.objectContaining({
        dest: "cargo",
        "dest-filename": "config.toml",
      }),
    );
  });
});
