import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vite-plus/test";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));
const sha256 = (bytes: Buffer) =>
  createHash("sha256").update(bytes).digest("hex");

test.skipIf(process.platform === "win32")(
  "stages a usable library from an internal symlink and repairs changed bytes",
  () => {
    const root = mkdtempSync(join(tmpdir(), "openkara-stage-test-"));
    try {
      const scripts = join(root, "scripts");
      const catalog = join(root, "src-tauri", "catalog");
      const source = join(root, "source", "onnxruntime-package", "lib");
      for (const dir of [scripts, catalog, source])
        mkdirSync(dir, { recursive: true });
      copyFileSync(
        join(projectRoot, "scripts", "prepare-onnx-runtime.mjs"),
        join(scripts, "prepare-onnx-runtime.mjs"),
      );
      const bytes = Buffer.from("verified runtime");
      writeFileSync(join(source, "libonnxruntime.1.31.1.dylib"), bytes);
      symlinkSync(
        "libonnxruntime.1.31.1.dylib",
        join(source, "libonnxruntime.dylib"),
      );
      const archive = join(root, "runtime.tar.gz");
      execFileSync("tar", [
        "-czf",
        archive,
        "-C",
        join(root, "source"),
        "onnxruntime-package",
      ]);
      const archiveBytes = readFileSync(archive);
      const digest = { size: bytes.length, sha256: sha256(bytes) };
      writeFileSync(
        join(catalog, "release-manifest.json"),
        JSON.stringify({
          schema_version: "openkara.catalog/release-v2",
          artifacts: {
            runtimes: [
              {
                artifact_id: "fixture",
                target_triple: "x86_64-apple-darwin",
                filename: "runtime.tar.gz",
                download_url: "https://example.invalid/runtime.tar.gz",
                byte_size: archiveBytes.length,
                archive_digest: sha256(archiveBytes),
                extracted_file_digests: {
                  "onnxruntime-package/lib/libonnxruntime.1.31.1.dylib": digest,
                  "onnxruntime-package/lib/libonnxruntime.dylib": digest,
                },
                runtime: { version: "1.31.1", execution_providers: ["cpu"] },
              },
            ],
          },
        }),
      );
      const preload = join(root, "fetch-fixture.mjs");
      writeFileSync(
        preload,
        'import { readFileSync } from "node:fs"; globalThis.fetch = async () => new Response(readFileSync(process.env.TEST_ARCHIVE));',
      );
      const run = () =>
        execFileSync(
          process.execPath,
          [
            "--import",
            preload,
            join(scripts, "prepare-onnx-runtime.mjs"),
            "--target",
            "x86_64-apple-darwin",
          ],
          { env: { ...process.env, TEST_ARCHIVE: archive }, stdio: "pipe" },
        );
      const staged = join(
        root,
        "src-tauri",
        "generated",
        "onnxruntime",
        "libonnxruntime.dylib",
      );
      run();
      expect(readFileSync(staged)).toEqual(bytes);
      writeFileSync(staged, "changed");
      run();
      expect(readFileSync(staged)).toEqual(bytes);

      for (const target of [join(root, "outside"), "../../../outside"]) {
        const link = join(source, "libonnxruntime.dylib");
        unlinkSync(link);
        symlinkSync(target, link);
        execFileSync("tar", [
          "-czf",
          archive,
          "-C",
          join(root, "source"),
          "onnxruntime-package",
        ]);
        const updatedArchive = readFileSync(archive);
        const manifestPath = join(catalog, "release-manifest.json");
        const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
        manifest.artifacts.runtimes[0].byte_size = updatedArchive.length;
        manifest.artifacts.runtimes[0].archive_digest = sha256(updatedArchive);
        writeFileSync(manifestPath, JSON.stringify(manifest));
        rmSync(staged, { force: true });
        expect(run).toThrow(/archive link/);
      }
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  },
);
