#!/usr/bin/env node
// Resolve a separation model artifact from the pinned openkara-models
// catalog snapshot (src-tauri/catalog/release-manifest.json).
//
// This is the same contract fixture the application embeds and the Rust
// catalog client validates, so setup.sh, CI, and the app all resolve model
// URLs, digests, and sizes from one source of truth.
//
// Usage:
//   node scripts/resolve-model.mjs                       # htdemucs, JSON output
//   node scripts/resolve-model.mjs --variant htdemucs_ft
//   node scripts/resolve-model.mjs --field sha256        # print a single field

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const manifestPath = join(
  scriptDir,
  "..",
  "src-tauri",
  "catalog",
  "release-manifest.json",
);

function parseArgs(argv) {
  const args = { variant: "htdemucs", field: null };
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === "--variant") {
      args.variant = argv[index + 1];
      index += 1;
    } else if (argv[index] === "--field") {
      args.field = argv[index + 1];
      index += 1;
    } else {
      throw new Error(`unknown argument: ${argv[index]}`);
    }
  }
  return args;
}

const args = parseArgs(process.argv.slice(2));
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));

if (manifest.schema_version !== "openkara.catalog/release-v1") {
  throw new Error(
    `unsupported release manifest schema: ${manifest.schema_version}`,
  );
}

// Mirror the Rust resolver: among non-deprecated artifacts of the variant
// with a LOADABLE tensor interface, the smallest download wins. The
// spectral session is the sole production path, so waveform deliveries —
// still listed in manifests for compatibility — are never candidates
// (for the ft variant the waveform dual archive is smaller than the
// spectral delivery; a size-only rule would resolve an unloadable model).
const matches = manifest.artifacts.models.filter(
  (model) =>
    model.variant === args.variant &&
    model.model?.tensor_interface === "spectral-core" &&
    !model.deprecation?.deprecated,
);
if (matches.length === 0) {
  throw new Error(
    `catalog snapshot must list exactly one model for variant ${args.variant}, found 0`,
  );
}
const model = matches.reduce((smallest, candidate) =>
  candidate.byte_size < smallest.byte_size ? candidate : smallest,
);

const onnxEntries = Object.entries(model.extracted_file_digests).filter(
  ([path]) => path.endsWith(".onnx"),
);
if (onnxEntries.length !== 1) {
  throw new Error(
    `model ${model.artifact_id} must declare exactly one .onnx file, found ${onnxEntries.length}`,
  );
}
const [modelFile, modelDigest] = onnxEntries[0];

const resolved = {
  variant: model.variant,
  artifact_id: model.artifact_id,
  // The installed .onnx file (extraction target for archived deliveries).
  filename: modelFile,
  file_sha256: modelDigest.sha256,
  file_size: modelDigest.size,
  // The download payload (an archive for compressed deliveries).
  download_filename: model.filename,
  url: model.download_url,
  sha256: model.archive_digest,
  size: model.byte_size,
  archived: /\.(tar\.gz|tgz|zip)$/.test(model.filename),
  tag: model.upstream.tag,
  generation: manifest.generation,
  release_id: manifest.release_id,
};

if (args.field !== null) {
  if (!(args.field in resolved)) {
    throw new Error(`unknown field: ${args.field}`);
  }
  process.stdout.write(`${resolved[args.field]}\n`);
} else {
  process.stdout.write(`${JSON.stringify(resolved, null, 2)}\n`);
}
