import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));
const measureScript = join(projectRoot, "scripts/ci/measure-bundle-size.mjs");
const mergeScript = join(projectRoot, "scripts/ci/merge-size-reports.mjs");

function run(script: string, args: string[]) {
  return spawnSync(process.execPath, [script, ...args], {
    cwd: projectRoot,
    encoding: "utf8",
  });
}

describe("bundle size reports", () => {
  test("measures build inputs and installer files without changing them", () => {
    const root = mkdtempSync(join(tmpdir(), "openkara-size-report-"));
    const frontend = join(root, "dist");
    const runtime = join(root, "runtime");
    const bundle = join(root, "bundle");
    const output = join(root, "report.json");
    mkdirSync(join(frontend, "dict"), { recursive: true });
    mkdirSync(runtime, { recursive: true });
    mkdirSync(join(bundle, "macos", "OpenKara.app"), { recursive: true });
    writeFileSync(join(frontend, "index.js"), "12345");
    writeFileSync(join(frontend, "dict", "data.bin"), "123");
    writeFileSync(join(runtime, "libonnxruntime.dylib"), "12");
    writeFileSync(join(bundle, "macos", "OpenKara.app", "openkara"), "123456");
    writeFileSync(join(bundle, "OpenKara_0.16.2_aarch64.dmg"), "123456789");
    writeFileSync(join(bundle, "OpenKara_0.16.2_aarch64.dmg.sig"), "signature");
    const binary = join(root, "openkara");
    writeFileSync(binary, "1234567");
    const inputPaths = [
      join(frontend, "index.js"),
      join(frontend, "dict", "data.bin"),
      join(runtime, "libonnxruntime.dylib"),
      join(bundle, "macos", "OpenKara.app", "openkara"),
      join(bundle, "OpenKara_0.16.2_aarch64.dmg"),
      join(bundle, "OpenKara_0.16.2_aarch64.dmg.sig"),
      binary,
    ];
    const before = inputPaths.map((inputPath) => readFileSync(inputPath));

    const result = run(measureScript, [
      "--platform",
      "darwin-aarch64",
      "--output",
      output,
      "--bundle-root",
      bundle,
      "--binary",
      binary,
      "--frontend-root",
      frontend,
      "--frontend-dict-root",
      join(frontend, "dict"),
      "--runtime-root",
      runtime,
    ]);

    expect(result.status).toBe(0);
    const report = JSON.parse(readFileSync(output, "utf8"));
    expect(report.status).toBe("measured");
    expect(report.platform).toBe("darwin-aarch64");
    expect(report.installers).toEqual([
      { path: "OpenKara_0.16.2_aarch64.dmg", byte_size: 9 },
    ]);
    expect(inputPaths.map((inputPath) => readFileSync(inputPath))).toEqual(
      before,
    );
    expect(report.components).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ name: "frontend_dist", byte_size: 5 }),
        expect.objectContaining({ name: "frontend_dict", byte_size: 3 }),
        expect.objectContaining({ name: "rust_binary", byte_size: 7 }),
        expect.objectContaining({ name: "onnx_runtime_input", byte_size: 2 }),
      ]),
    );
  });

  test("merges available platform reports deterministically", () => {
    const root = mkdtempSync(join(tmpdir(), "openkara-size-report-"));
    const reports = join(root, "reports");
    const output = join(root, "merged.json");
    mkdirSync(reports);
    writeFileSync(
      join(reports, "linux-x86_64.json"),
      JSON.stringify({
        schema_version: 1,
        status: "measured",
        platform: "linux-x86_64",
      }),
    );
    writeFileSync(
      join(reports, "windows-x86_64.json"),
      JSON.stringify({
        schema_version: 1,
        status: "measured",
        platform: "windows-x86_64",
      }),
    );

    const result = run(mergeScript, [reports, output]);
    expect(result.status).toBe(0);
    expect(JSON.parse(readFileSync(output, "utf8"))).toEqual({
      schema_version: 1,
      status: "measured",
      reports: [
        { schema_version: 1, status: "measured", platform: "linux-x86_64" },
        { schema_version: 1, status: "measured", platform: "windows-x86_64" },
      ],
    });
  });
});
