import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const profiles = [
  "lifecycle-quality-and-testing.md",
  "interaction-and-accessibility.md",
  "language-terminology-and-data.md",
  "interfaces-and-compatibility.md",
  "security-privacy-and-release.md",
  "models-media-and-operations.md",
];

const requiredFiles = [
  "AGENTS.md",
  "CONTRIBUTING.md",
  ".github/PULL_REQUEST_TEMPLATE.md",
  "docs/references/product-standards.md",
  "docs/references/standards/README.md",
  "docs/adr/0014-route-product-standards-by-changed-surface.md",
  ...profiles.map((profile) => `docs/references/standards/${profile}`),
];

const failures = [];

function read(path) {
  const absolutePath = resolve(root, path);
  if (!existsSync(absolutePath)) {
    failures.push(`Missing required standards file: ${path}`);
    return "";
  }
  return readFileSync(absolutePath, "utf8");
}

for (const path of requiredFiles) {
  read(path);
}

const agents = read("AGENTS.md");
if (!agents.includes("docs/references/product-standards.md")) {
  failures.push("AGENTS.md does not route agents to product standards.");
}

const contributing = read("CONTRIBUTING.md");
if (!contributing.includes("docs/references/product-standards.md")) {
  failures.push(
    "CONTRIBUTING.md does not route contributors to product standards.",
  );
}

const template = read(".github/PULL_REQUEST_TEMPLATE.md");
if (!template.includes("product-standard profile")) {
  failures.push(
    "The pull request template has no product-standards evidence check.",
  );
}

const index = read("docs/references/product-standards.md");
for (const profile of profiles) {
  if (!index.includes(`standards/${profile}`)) {
    failures.push(`Product standards index does not link to ${profile}.`);
  }
}

for (const profile of profiles) {
  const path = `docs/references/standards/${profile}`;
  const contents = read(path);
  if (!contents.includes("## Authorities")) {
    failures.push(`${path} has no authorities section.`);
  }
  if (!contents.includes("## Constraints")) {
    failures.push(`${path} has no constraints section.`);
  }
  if (!contents.includes("## Required evidence")) {
    failures.push(`${path} has no required-evidence section.`);
  }
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.error(`Standards check failed: ${failure}`);
  }
  process.exit(1);
}

console.log("Standards route is valid.");
