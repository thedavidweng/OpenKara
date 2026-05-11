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
  test("uses the DirectML NuGet runtime for Windows instead of the CPU release zip", () => {
    const prepareScript = readProjectFile("scripts/prepare-onnx-runtime.mjs");

    expect(prepareScript).toContain("Microsoft.ML.OnnxRuntime.DirectML");
    expect(prepareScript).toContain("Microsoft.AI.DirectML");
    expect(prepareScript).toContain("api/v2/package");
    expect(prepareScript).toContain("onnxruntime_providers_shared.dll");
    expect(prepareScript).toContain("DirectML.dll");
    expect(prepareScript).not.toContain(
      "onnxruntime-win-x64-${ORT_VERSION}.zip",
    );
  });

  test("keeps Flatpak ONNX Runtime source aligned with the prepared runtime version", () => {
    const prepareScript = readProjectFile("scripts/prepare-onnx-runtime.mjs");
    const manifestTemplate = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.yml.in",
    );
    const runtimeVersion = prepareScript.match(
      /const ORT_VERSION = "([^"]+)";/,
    )?.[1];

    expect(runtimeVersion).toBeDefined();
    expect(manifestTemplate).toContain(`\\"version\\":\\"${runtimeVersion}\\"`);
    expect(manifestTemplate).toContain(
      `onnxruntime-linux-x64-${runtimeVersion}.tgz`,
    );
    expect(manifestTemplate).toContain(
      `onnxruntime-linux-aarch64-${runtimeVersion}.tgz`,
    );
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
        "dest-filename": "config.toml",
      }),
    );
    expect(cargoSources).not.toContainEqual(
      expect.objectContaining({
        dest: "cargo",
        "dest-filename": "config",
      }),
    );
  });
});
