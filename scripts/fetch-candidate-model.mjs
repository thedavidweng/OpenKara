#!/usr/bin/env node
// Resolve a spectral-core model artifact from an openkara-models catalog
// CHANNEL pointer (candidate by default) instead of the embedded snapshot.
//
// The candidate channel carries pre-promotion generations (openkara-models
// issue #23 PR 3): cross-target validation (issue #172 PR 4) runs against
// it BEFORE the stable pointer — and therefore the embedded snapshot every
// consumer trusts — ever references a spectral-core artifact. The manifest
// bytes are verified against the pointer's SHA-256 and size before parsing,
// mirroring the Rust catalog client.
//
// Usage:
//   node scripts/fetch-candidate-model.mjs                        # htdemucs, JSON
//   node scripts/fetch-candidate-model.mjs --variant htdemucs_ft
//   node scripts/fetch-candidate-model.mjs --channel stable
//   node scripts/fetch-candidate-model.mjs --field url            # single field

import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";

const POINTER_BASE =
  "https://raw.githubusercontent.com/thedavidweng/openkara-models/main/catalog/channels";

function parseArgs(argv) {
  const args = {
    variant: "htdemucs",
    field: null,
    channel: "candidate",
    interface: "spectral-core",
  };
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === "--variant") {
      args.variant = argv[index + 1];
      index += 1;
    } else if (argv[index] === "--field") {
      args.field = argv[index + 1];
      index += 1;
    } else if (argv[index] === "--channel") {
      args.channel = argv[index + 1];
      index += 1;
    } else if (argv[index] === "--interface") {
      args.interface = argv[index + 1];
      index += 1;
    } else {
      throw new Error(`unknown argument: ${argv[index]}`);
    }
  }
  if (!["candidate", "stable"].includes(args.channel)) {
    throw new Error(`unknown channel: ${args.channel}`);
  }
  return args;
}

/**
 * Pick the artifact for (variant, tensor interface) from a verified release
 * manifest: smallest non-deprecated download wins, mirroring the Rust
 * resolver. Exported for tests.
 */
export function selectModel(manifest, variant, tensorInterface) {
  if (manifest.schema_version !== "openkara.catalog/release-v1") {
    throw new Error(
      `unsupported release manifest schema: ${manifest.schema_version}`,
    );
  }
  const matches = manifest.artifacts.models.filter(
    (model) =>
      model.variant === variant &&
      model.model?.tensor_interface === tensorInterface &&
      !model.deprecation?.deprecated,
  );
  if (matches.length === 0) {
    throw new Error(
      `manifest ${manifest.release_id} lists no ${tensorInterface} model for variant ${variant}`,
    );
  }
  return matches.reduce((smallest, candidate) =>
    candidate.byte_size < smallest.byte_size ? candidate : smallest,
  );
}

/** Verify manifest bytes against the channel pointer before parsing. */
export function verifyManifestBytes(pointer, bytes) {
  if (pointer.schema_version !== "openkara.catalog/channel-v1") {
    throw new Error(`unsupported channel schema: ${pointer.schema_version}`);
  }
  if (bytes.byteLength !== pointer.release_manifest_size) {
    throw new Error(
      `manifest size ${bytes.byteLength} does not match the pointer's ${pointer.release_manifest_size}`,
    );
  }
  const digest = createHash("sha256").update(bytes).digest("hex");
  if (digest !== pointer.release_manifest_sha256) {
    throw new Error(
      `manifest sha256 ${digest} does not match the pointer's ${pointer.release_manifest_sha256}`,
    );
  }
  return JSON.parse(new TextDecoder().decode(bytes));
}

async function fetchBytes(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`fetch failed (${response.status}): ${url}`);
  }
  return new Uint8Array(await response.arrayBuffer());
}

async function main() {
  const args = parseArgs(process.argv.slice(2));

  const pointerBytes = await fetchBytes(`${POINTER_BASE}/${args.channel}.json`);
  const pointer = JSON.parse(new TextDecoder().decode(pointerBytes));
  const manifest = verifyManifestBytes(
    pointer,
    await fetchBytes(pointer.release_manifest_url),
  );

  const model = selectModel(manifest, args.variant, args.interface);
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
    channel: args.channel,
    variant: model.variant,
    tensor_interface: model.model.tensor_interface,
    artifact_id: model.artifact_id,
    filename: modelFile,
    file_sha256: modelDigest.sha256,
    file_size: modelDigest.size,
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
}

const invokedDirectly =
  process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (invokedDirectly) {
  await main();
}
