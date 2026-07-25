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
    "resolves %s to the smallest non-deprecated delivery",
    (variant) => {
      const resolved = resolve(variant);
      const candidates = manifest.artifacts.models.filter(
        (model: { variant: string; deprecation?: { deprecated?: boolean } }) =>
          model.variant === variant && !model.deprecation?.deprecated,
      );
      const catalogModel = candidates.reduce(
        (smallest: { byte_size: number }, candidate: { byte_size: number }) =>
          candidate.byte_size < smallest.byte_size ? candidate : smallest,
      );

      expect(catalogModel).toBeDefined();
      expect(resolved.url).toBe(catalogModel.download_url);
      expect(resolved.sha256).toBe(catalogModel.archive_digest);
      expect(resolved.download_filename).toBe(catalogModel.filename);
      expect(resolved.size).toBe(catalogModel.byte_size);
      expect(resolved.tag).toBe(catalogModel.upstream.tag);
      expect(resolved.artifact_id).toBe(catalogModel.artifact_id);
      expect(resolved.generation).toBe(manifest.generation);

      // The installed file is the extracted .onnx with its own digest.
      const onnxEntries = Object.entries(
        catalogModel.extracted_file_digests,
      ).filter(([path]) => path.endsWith(".onnx"));
      expect(onnxEntries).toHaveLength(1);
      const [file, digest] = onnxEntries[0] as [
        string,
        { sha256: string; size: number },
      ];
      expect(resolved.filename).toBe(file);
      expect(resolved.file_sha256).toBe(digest.sha256);
      expect(resolved.file_size).toBe(digest.size);
      expect(resolved.archived).toBe(
        /\.(tar\.gz|tgz|zip)$/.test(catalogModel.filename),
      );
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

  test("the snapshot pins portable models for both variants with compatibility", () => {
    for (const variant of ["htdemucs", "htdemucs_ft"]) {
      const candidates = manifest.artifacts.models.filter(
        (model: { variant: string; deprecation?: { deprecated?: boolean } }) =>
          model.variant === variant && !model.deprecation?.deprecated,
      );
      expect(candidates.length).toBeGreaterThan(0);
    }
    for (const model of manifest.artifacts.models) {
      expect(model.model.tensor_interface).toBe("waveform");
      expect(model.model.compatible_runtime_ids.length).toBeGreaterThan(0);
    }
    expect(manifest.compatibility.length).toBeGreaterThan(0);
  });
});
