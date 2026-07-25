import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

// scripts/resolve-model.mjs, scripts/setup.sh, CI, and the Rust catalog
// client must all resolve model artifacts from the same pinned catalog
// snapshot. This test pins the resolver's output to that snapshot so the
// contract cannot drift between consumers.

const projectRoot = fileURLToPath(new URL("..", import.meta.url));
const resolverPath = join(projectRoot, "scripts", "resolve-model.mjs");
const manifestPath = join(
  projectRoot,
  "src-tauri",
  "catalog",
  "release-manifest.json",
);

function resolve(variant: string) {
  const output = execFileSync("node", [resolverPath, "--variant", variant], {
    encoding: "utf8",
  });
  return JSON.parse(output);
}

describe("model resolver parity", () => {
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));

  test.each(["htdemucs", "htdemucs_ft"])(
    "resolves %s exactly as the catalog snapshot declares",
    (variant) => {
      const resolved = resolve(variant);
      const catalogModel = manifest.artifacts.models.find(
        (model: { variant: string }) => model.variant === variant,
      );

      expect(catalogModel).toBeDefined();
      expect(resolved.url).toBe(catalogModel.download_url);
      expect(resolved.sha256).toBe(catalogModel.archive_digest);
      expect(resolved.filename).toBe(catalogModel.filename);
      expect(resolved.size).toBe(catalogModel.byte_size);
      expect(resolved.tag).toBe(catalogModel.upstream.tag);
      expect(resolved.artifact_id).toBe(catalogModel.artifact_id);
      expect(resolved.generation).toBe(manifest.generation);
    },
  );

  test("single-field output prints the raw value for shell consumers", () => {
    const sha = execFileSync(
      "node",
      [resolverPath, "--variant", "htdemucs", "--field", "sha256"],
      { encoding: "utf8" },
    ).trim();

    expect(sha).toMatch(/^[0-9a-f]{64}$/);
  });

  test("rejects unknown variants", () => {
    expect(() => resolve("htdemucs_v5")).toThrow();
  });

  test("the snapshot pins exactly two portable models with compatibility", () => {
    expect(manifest.artifacts.models).toHaveLength(2);
    for (const model of manifest.artifacts.models) {
      expect(model.model.tensor_interface).toBe("waveform");
      expect(model.model.compatible_runtime_ids.length).toBeGreaterThan(0);
    }
    expect(manifest.compatibility.length).toBeGreaterThan(0);
  });
});
