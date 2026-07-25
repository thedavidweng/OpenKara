// Verify the Rust `ort` binding's declared ONNX Runtime C API level is
// compatible with every runtime artifact in the pinned catalog snapshot.
//
// The binding requests `OrtApi` version N (its highest `api-N` cargo
// feature); a native runtime answers requests up to its own C API level.
// Compatibility therefore requires: crate level <= runtime level for every
// catalog runtime. This is the guard that lets Cargo automation update the
// `ort` binding while openkara-models owns native runtime versions.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT_DIR = fileURLToPath(new URL("../..", import.meta.url));
const cargoToml = readFileSync(
  join(ROOT_DIR, "src-tauri", "Cargo.toml"),
  "utf8",
);
const manifest = JSON.parse(
  readFileSync(
    join(ROOT_DIR, "src-tauri", "catalog", "release-manifest.json"),
    "utf8",
  ),
);

const ortLine = cargoToml
  .split("\n")
  .find((line) => line.trimStart().startsWith("ort = "));
if (!ortLine) {
  throw new Error(
    "failed to find the `ort` dependency in src-tauri/Cargo.toml",
  );
}

const apiFeatures = [...ortLine.matchAll(/"api-(\d+)"/g)].map((match) =>
  Number(match[1]),
);
if (apiFeatures.length === 0) {
  throw new Error(
    "the `ort` dependency declares no api-N feature; the required C API level is unknown",
  );
}
const crateApiLevel = Math.max(...apiFeatures);

const failures = [];
for (const runtime of manifest.artifacts.runtimes) {
  const runtimeLevel = Number(runtime.runtime.ort_c_api_level);
  if (!Number.isInteger(runtimeLevel)) {
    failures.push(
      `${runtime.artifact_id}: ort_c_api_level ${runtime.runtime.ort_c_api_level} is not an integer`,
    );
    continue;
  }
  if (crateApiLevel > runtimeLevel) {
    failures.push(
      `${runtime.artifact_id}: crate requires C API ${crateApiLevel} but the runtime only provides ${runtimeLevel}`,
    );
  }
}

if (failures.length > 0) {
  console.error("ort C API level check failed:");
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}

console.log(
  `ort C API level check passed: crate api-${crateApiLevel} vs ${manifest.artifacts.runtimes.length} catalog runtimes (level >= ${crateApiLevel}).`,
);
