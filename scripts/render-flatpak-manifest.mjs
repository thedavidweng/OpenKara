import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { parseArgs, requireArg, sha256ForUrl } from "./release-metadata.mjs";

const args = parseArgs(process.argv.slice(2));

const owner = args.owner ?? "thedavidweng";
const repo = args.repo ?? "OpenKara";
const version = requireArg(args, "version");
const tag = args.tag ?? `v${version}`;
const outputDir = requireArg(args, "output");
const templateRoot = args["template-root"] ?? "packaging/flatpak";

const sourceUrl =
  args["source-url"] ??
  `https://github.com/${owner}/${repo}/archive/refs/tags/${tag}.tar.gz`;
const sourceSha256 = await sha256ForUrl(sourceUrl);

const outputRoot = join(outputDir, "io.github.thedavidweng.OpenKara");
mkdirSync(outputRoot, { recursive: true });

const cargoSourcesPath = join(templateRoot, "generated", "cargo-sources.json");
const nodeSourceFiles = readdirSync(join(templateRoot, "generated"))
  .filter((file) => file.startsWith("node-sources") && file.endsWith(".json"))
  .sort();

const nodeSourcesYaml = nodeSourceFiles
  .map((file) => `      - ${file}`)
  .join("\n");

const manifestTemplate = readFileSync(
  join(templateRoot, "io.github.thedavidweng.OpenKara.yml.in"),
  "utf8",
);

// ONNX Runtime sources come from the pinned catalog snapshot — the same
// contract fixture the application and prepare-onnx-runtime.mjs consume.
const catalog = JSON.parse(
  readFileSync(join("src-tauri", "catalog", "release-manifest.json"), "utf8"),
);
const catalogRuntime = (target) => {
  // Deprecated (superseded) runtimes stay listed for provenance; only the
  // active delivery per target may be bundled (mirrors resolve_runtime).
  const matches = catalog.artifacts.runtimes.filter(
    (runtime) =>
      runtime.target_triple === target && !runtime.deprecation?.deprecated,
  );
  if (matches.length !== 1) {
    throw new Error(
      `catalog snapshot must list exactly one active runtime for ${target}`,
    );
  }
  return matches[0];
};
const ortX64 = catalogRuntime("x86_64-unknown-linux-gnu");
const ortArm64 = catalogRuntime("aarch64-unknown-linux-gnu");
if (ortX64.runtime.version !== ortArm64.runtime.version) {
  throw new Error("catalog Linux runtimes disagree on the ORT version");
}

const replacements = new Map([
  ["@@SOURCE_URL@@", sourceUrl],
  ["@@SOURCE_SHA256@@", sourceSha256],
  ["@@NODE_SOURCES@@", nodeSourcesYaml],
  ["@@ORT_VERSION@@", ortX64.runtime.version],
  ["@@ORT_X64_URL@@", ortX64.download_url],
  ["@@ORT_X64_SHA256@@", ortX64.archive_digest],
  ["@@ORT_ARM64_URL@@", ortArm64.download_url],
  ["@@ORT_ARM64_SHA256@@", ortArm64.archive_digest],
]);

const replaceTokens = (content) => {
  let next = content;
  for (const [token, value] of replacements.entries()) {
    next = next.replaceAll(token, value);
  }
  return next;
};

writeFileSync(
  join(outputRoot, "io.github.thedavidweng.OpenKara.yml"),
  replaceTokens(manifestTemplate),
);
writeFileSync(
  join(outputRoot, "cargo-sources.json"),
  readFileSync(cargoSourcesPath, "utf8"),
);

for (const file of nodeSourceFiles) {
  writeFileSync(
    join(outputRoot, file),
    readFileSync(join(templateRoot, "generated", file), "utf8"),
  );
}

process.stdout.write(`${outputRoot}\n`);
