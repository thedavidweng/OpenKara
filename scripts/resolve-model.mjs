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

const matches = manifest.artifacts.models.filter(
  (model) => model.variant === args.variant,
);
if (matches.length !== 1) {
  throw new Error(
    `catalog snapshot must list exactly one model for variant ${args.variant}, found ${matches.length}`,
  );
}

const model = matches[0];
const resolved = {
  variant: model.variant,
  artifact_id: model.artifact_id,
  filename: model.filename,
  url: model.download_url,
  sha256: model.archive_digest,
  size: model.byte_size,
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
