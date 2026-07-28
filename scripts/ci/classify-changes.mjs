// CI change classifier — single source of truth for path-based CI gating.
//
// This module is a pure function over filenames and event type. It maps every
// changed file to one or more known categories, collects unmatched files as
// `unknown`, and derives the expected job set from the category union.
//
// The workflow (.github/workflows/ci.yml) consumes the per-job boolean outputs
// (`run_<job>`) in `if:` conditionals. CI Gate consumes `expected-jobs` to
// verify that every expected job ran and every non-expected job was skipped.
//
// Contract tests live in tests/ci/classify-changes.test.ts. Drift-protection
// tests that parse ci.yml live in tests/ci/ci-workflow-contract.test.ts.
//
// CLI usage:
//   node scripts/ci/classify-changes.mjs --files <newline-separated-paths> --event pull_request
//   node scripts/ci/classify-changes.mjs --json '["src/foo.ts"]' --event pull_request
//
// Outputs JSON to stdout and writes GITHUB_OUTPUT entries when GITHUB_OUTPUT is set.

import { readFileSync, appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import { argv } from "node:process";

// ── Glob matching ─────────────────────────────────────────────────────
// Minimal glob-to-regex converter supporting `**`, `*`, `?`, and literals.
// `**/` matches zero or more path segments. `/**` matches anything under a
// directory. `*` matches anything except `/`.

/** @param {string} pattern */
function globToRegex(pattern) {
  let re = "^";
  let i = 0;
  while (i < pattern.length) {
    const c = pattern[i];
    if (c === "*" && pattern[i + 1] === "*") {
      i += 2;
      if (pattern[i] === "/") {
        re += "(?:.*/)?";
        i += 1;
      } else {
        re += ".*";
      }
    } else if (c === "*") {
      re += "[^/]*";
      i += 1;
    } else if (c === "?") {
      re += "[^/]";
      i += 1;
    } else if (/[.+^${}()|[\]\\]/.test(c)) {
      re += "\\" + c;
      i += 1;
    } else {
      re += c;
      i += 1;
    }
  }
  re += "$";
  return new RegExp(re);
}

// ── Category definitions ──────────────────────────────────────────────
// Each category maps to a list of glob patterns. A file may belong to multiple
// categories. `unknown` is NOT a category with patterns — it is the set
// complement of all known categories.

/** @type {Record<string, string[]>} */
const CATEGORY_PATTERNS = {
  docs: ["**/*.md", "docs/**", "LICENSE"],

  frontend_app: ["src/**", "public/**", "index.html"],

  website: ["website/**"],

  frontend_tooling: [
    "vite.config.ts",
    "tsconfig.json",
    "tsconfig.node.json",
    "tsconfig.app.json",
    "oxlintrc.json",
    ".oxfmtrc.json",
    "knip.json",
    "pnpm-workspace.yaml",
    "lefthook.yml",
    "codecov.yml",
    "scripts/check-i18n.mjs",
    "scripts/generate-db-schema.mjs",
    "scripts/generate-mock-songs.mjs",
    "scripts/prepare-bundled-oauth-client.mjs",
    "tests/contract/**",
  ],

  e2e: ["tests/e2e/**", "playwright.config.ts", "playwright.webkit.config.ts"],

  rust: [
    "src-tauri/src/**",
    "src-tauri/tests/**",
    // Source-tree smoke binaries compile against the production crate and
    // need the same native validation as any other Rust entry point.
    "src-tauri/examples/**",
    "src-tauri/deny.toml",
    "rust-toolchain.toml",
    "patches/**",
    // The pinned catalog snapshot is compiled into the binary and drives
    // model/runtime resolution — a pin bump must run the full Rust suite.
    "src-tauri/catalog/**",
  ],

  model_runtime: [
    "scripts/prepare-onnx-runtime.mjs",
    "scripts/resolve-model.mjs",
    "src-tauri/catalog/**",
  ],

  deps_js: ["package.json", "pnpm-lock.yaml"],

  deps_rust: ["src-tauri/Cargo.toml", "src-tauri/Cargo.lock"],

  ci_workflow: [
    ".github/workflows/ci.yml",
    ".github/labeler.yml",
    "scripts/ci/**",
    "scripts/check-standards.mjs",
    "tests/ci/**",
    "tests/workflow-security.test.ts",
    "scripts/check-patch-coverage.mjs",
  ],

  release_workflow: [
    ".github/workflows/release.yml",
    "tests/release-workflow.test.ts",
    "scripts/setup.sh",
    "scripts/run-local-smoke.sh",
    "scripts/validate-installed-app-smoke.mjs",
  ],

  packaging_workflow: [".github/workflows/packaging.yml"],

  e2e_workflow: [
    // No standalone E2E workflow file exists today; Playwright runs inside
    // ci.yml. This category is reserved for when one is added.
  ],

  other_workflow: [
    ".github/workflows/dependabot-sync.yml",
    ".github/workflows/mirror.yml",
    ".github/workflows/pages.yml",
    ".github/dependabot.yml",
    ".github/ISSUE_TEMPLATE/**",
    ".github/PULL_REQUEST_TEMPLATE.md",
  ],

  packaging_inputs: [
    "packaging/**",
    "src-tauri/tauri.conf.json",
    "src-tauri/tauri.linux.conf.json",
    "src-tauri/tauri.macos.conf.json",
    "src-tauri/tauri.windows.conf.json",
    "scripts/prepare-onnx-runtime.mjs",
    "scripts/render-flatpak-manifest.mjs",
    "scripts/render-winget-manifests.mjs",
    "scripts/generate-flatpak-cargo-sources.mjs",
    "scripts/generate-flatpak-node-sources.mjs",
    "scripts/clean-macos-bundle.mjs",
    "tests/flatpak-packaging.test.ts",
    "tests/onnx-runtime-packaging.test.ts",
    "tests/tauri-config.test.ts",
  ],

  release_metadata: [
    "scripts/release-metadata.mjs",
    "scripts/sync-version.mjs",
    "cliff.toml",
    "tests/release-metadata.test.ts",
  ],
};

// Pre-compile all category regexes for performance.
/** @type {Record<string, RegExp[]>} */
const CATEGORY_REGEXES = Object.fromEntries(
  Object.entries(CATEGORY_PATTERNS).map(([cat, patterns]) => [
    cat,
    patterns.map(globToRegex),
  ]),
);

// ── Classification ────────────────────────────────────────────────────

/**
 * Classify a single file into all matching categories.
 * @param {string} file
 * @returns {string[]}
 */
function classifyFile(file) {
  const cats = [];
  for (const [cat, regexes] of Object.entries(CATEGORY_REGEXES)) {
    if (regexes.some((re) => re.test(file))) {
      cats.push(cat);
    }
  }
  return cats;
}

// ── Job derivation ────────────────────────────────────────────────────
// Maps category sets to expected job IDs. The workflow job `if:` conditions
// consume the per-job boolean outputs (`run_<job>`).

/** @type {Record<string, (cats: Set<string>, event: string) => boolean>} */
const JOB_RULES = {
  triage: () => true,
  "conventional-commits": (_cats, event) => event === "pull_request",
  "standards-reference": (cats) =>
    cats.has("docs") || cats.has("ci_workflow") || cats.has("unknown"),
  "ci-gate": () => true,

  "js-quality": (cats) =>
    cats.has("frontend_app") ||
    cats.has("website") ||
    cats.has("frontend_tooling") ||
    cats.has("e2e") ||
    cats.has("deps_js") ||
    cats.has("ci_workflow") ||
    cats.has("unknown"),

  "app-frontend": (cats) =>
    cats.has("frontend_app") ||
    cats.has("frontend_tooling") ||
    cats.has("deps_js") ||
    cats.has("ci_workflow") ||
    cats.has("unknown"),

  website: (cats) =>
    cats.has("website") ||
    cats.has("deps_js") ||
    cats.has("ci_workflow") ||
    cats.has("unknown"),

  "playwright-ui-smoke": (cats) =>
    cats.has("frontend_app") ||
    cats.has("frontend_tooling") ||
    cats.has("e2e") ||
    cats.has("deps_js") ||
    cats.has("ci_workflow") ||
    cats.has("unknown"),

  "workflow-lint": (cats) =>
    cats.has("ci_workflow") ||
    cats.has("release_workflow") ||
    cats.has("packaging_workflow") ||
    cats.has("e2e_workflow") ||
    cats.has("other_workflow") ||
    cats.has("unknown"),

  "release-validation": (cats) =>
    cats.has("release_workflow") ||
    cats.has("release_metadata") ||
    cats.has("ci_workflow") ||
    cats.has("unknown"),

  "cargo-deny": (cats) =>
    cats.has("rust") ||
    cats.has("model_runtime") ||
    cats.has("deps_rust") ||
    cats.has("ci_workflow") ||
    cats.has("unknown"),

  "dependency-checks": (cats) =>
    cats.has("rust") ||
    cats.has("model_runtime") ||
    cats.has("deps_rust") ||
    cats.has("ci_workflow") ||
    cats.has("unknown"),

  "prepare-model": (cats) =>
    cats.has("rust") ||
    cats.has("model_runtime") ||
    cats.has("deps_rust") ||
    cats.has("ci_workflow") ||
    cats.has("unknown"),

  "rust-test": (cats) =>
    cats.has("rust") ||
    cats.has("model_runtime") ||
    cats.has("deps_rust") ||
    cats.has("ci_workflow") ||
    cats.has("unknown"),

  "rust-test-windows-compile": (cats) =>
    cats.has("rust") ||
    cats.has("model_runtime") ||
    cats.has("deps_rust") ||
    cats.has("ci_workflow") ||
    cats.has("unknown"),

  "tauri-build-smoke": (cats) => {
    const frontend =
      cats.has("frontend_app") ||
      cats.has("frontend_tooling") ||
      cats.has("deps_js");
    const heavy =
      cats.has("rust") ||
      cats.has("model_runtime") ||
      cats.has("deps_rust") ||
      cats.has("ci_workflow") ||
      cats.has("unknown");
    return frontend && !heavy;
  },

  "tauri-build": (cats) =>
    cats.has("rust") ||
    cats.has("model_runtime") ||
    cats.has("deps_rust") ||
    cats.has("ci_workflow") ||
    cats.has("unknown"),
};

// All job IDs in declaration order (used by CI Gate and drift tests).
const ALL_JOBS = Object.keys(JOB_RULES);

// Jobs considered "heavy" for the step summary and gate enforcement.
const HEAVY_JOBS = new Set([
  "app-frontend",
  "website",
  "playwright-ui-smoke",
  "cargo-deny",
  "dependency-checks",
  "prepare-model",
  "rust-test",
  "rust-test-windows-compile",
  "tauri-build-smoke",
  "tauri-build",
]);

// ── Main classify function ────────────────────────────────────────────

/**
 * Classify changed files and derive expected jobs.
 *
 * For `push` (to main) and `workflow_dispatch`, returns full CI (all jobs)
 * as a deliberate safety path — the integration branch always gets full
 * validation regardless of what changed.
 *
 * @param {string[]} files - changed file paths
 * @param {string} event - "pull_request" | "push" | "workflow_dispatch"
 * @returns {{
 *   files: string[],
 *   categories: string[],
 *   categoriesByFile: Record<string, string[]>,
 *   unknownFiles: string[],
 *   expectedJobs: string[],
 *   expectedSkippedHeavyJobs: string[],
 *   run: Record<string, boolean>,
 * }}
 */
export function classifyChanges(files, event) {
  // Always classify each file against all known categories. The category
  // data is consumed by both the main CI workflow (for job gating) and the
  // packaging workflow (for packaging-specific gates). Classifying up front
  // lets downstream consumers inspect categories regardless of event type,
  // while the job derivation below applies the event-based safety path.
  /** @type {Record<string, string[]>} */
  const categoriesByFile = {};
  /** @type {Set<string>} */
  const categorySet = new Set();
  /** @type {string[]} */
  const unknownFiles = [];

  for (const file of files) {
    const cats = classifyFile(file);
    categoriesByFile[file] = cats;
    if (cats.length === 0) {
      unknownFiles.push(file);
      categorySet.add("unknown");
    } else {
      for (const c of cats) {
        categorySet.add(c);
      }
    }
  }

  // Push to main and workflow_dispatch always run full CI as a deliberate
  // safety path — the integration branch always gets full validation
  // regardless of what changed. Categories are still populated above so
  // downstream consumers (e.g. packaging triage) can inspect them.
  if (event === "push" || event === "workflow_dispatch") {
    const allJobs = ALL_JOBS.filter((job) =>
      JOB_RULES[job](new Set(["unknown"]), event),
    );
    return {
      files,
      categories: [...categorySet],
      categoriesByFile,
      unknownFiles,
      expectedJobs: allJobs,
      expectedSkippedHeavyJobs: [],
      run: Object.fromEntries(ALL_JOBS.map((j) => [j, allJobs.includes(j)])),
    };
  }

  // PR: derive expected jobs from the category union.
  const expectedJobs = ALL_JOBS.filter((job) =>
    JOB_RULES[job](categorySet, event),
  );

  const expectedSkippedHeavyJobs = HEAVY_JOBS
    ? [...HEAVY_JOBS].filter((j) => !expectedJobs.includes(j))
    : [];

  const run = Object.fromEntries(
    ALL_JOBS.map((j) => [j, expectedJobs.includes(j)]),
  );

  return {
    files,
    categories: [...categorySet].sort(),
    categoriesByFile,
    unknownFiles,
    expectedJobs,
    expectedSkippedHeavyJobs,
    run,
  };
}

// ── Step summary ──────────────────────────────────────────────────────

/**
 * Build a GITHUB_STEP_SUMMARY markdown table.
 * @param {ReturnType<typeof classifyChanges>} result
 * @param {string} event
 */
export function buildStepSummary(result, event) {
  const lines = [];
  lines.push(`## CI Triage`);
  lines.push("");
  lines.push(`**Event:** \`${event}\``);
  lines.push("");

  lines.push("### Changed files");
  lines.push("");
  if (result.files.length === 0) {
    lines.push("- _(none)_");
  } else {
    for (const file of result.files) {
      const cats = result.categoriesByFile[file] ?? [];
      const label = cats.length > 0 ? cats.join(", ") : "unknown";
      lines.push(`- \`${file}\` → ${label}`);
    }
  }
  lines.push("");

  if (result.unknownFiles.length > 0) {
    lines.push("### Unknown files (full CI safety fallback)");
    lines.push("");
    for (const file of result.unknownFiles) {
      lines.push(`- \`${file}\``);
    }
    lines.push("");
  }

  lines.push("### Expected jobs");
  lines.push("");
  if (result.expectedJobs.length === 0) {
    lines.push("- _(none — no heavy jobs needed)_");
  } else {
    for (const job of result.expectedJobs) {
      lines.push(`- ${job}`);
    }
  }
  lines.push("");

  const skippedHeavy = result.expectedSkippedHeavyJobs;
  if (skippedHeavy.length > 0) {
    lines.push("### Expected skipped heavy jobs");
    lines.push("");
    for (const job of skippedHeavy) {
      lines.push(`- ${job}`);
    }
    lines.push("");
  }

  return lines.join("\n");
}

// ── CLI entry point ───────────────────────────────────────────────────

/** @param {string[]} argv */
export function parseArgs(argv) {
  const args = { files: null, json: null, event: "pull_request" };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--files") {
      args.files = argv[++i];
    } else if (arg === "--json") {
      args.json = argv[++i];
    } else if (arg === "--event") {
      args.event = argv[++i];
    }
  }
  return args;
}

function main() {
  const args = parseArgs(process.argv.slice(2));

  /** @type {string[]} */
  let files;
  if (args.json) {
    files = JSON.parse(args.json);
  } else if (args.files) {
    files = args.files.split("\n").filter(Boolean);
  } else {
    // Read from stdin if no files provided.
    const input = readFileSync(0, "utf8").trim();
    files = input ? input.split("\n").filter(Boolean) : [];
  }

  const result = classifyChanges(files, args.event);
  const json = JSON.stringify(result, null, 2);
  console.log(json);

  // Write GITHUB_OUTPUT entries when available.
  const ghOutput = process.env.GITHUB_OUTPUT;
  if (ghOutput) {
    const lines = [
      `categories=${JSON.stringify(result.categories)}`,
      `unknown=${result.unknownFiles.length > 0}`,
      `unknown-files=${JSON.stringify(result.unknownFiles)}`,
      `expected-jobs=${JSON.stringify(result.expectedJobs)}`,
      `expected-skipped-heavy=${JSON.stringify(result.expectedSkippedHeavyJobs)}`,
    ];
    for (const job of ALL_JOBS) {
      lines.push(`run_${job}=${result.run[job]}`);
    }
    appendFileSync(ghOutput, lines.join("\n") + "\n");
  }

  // Write step summary when available.
  const ghSummary = process.env.GITHUB_STEP_SUMMARY;
  if (ghSummary) {
    appendFileSync(ghSummary, buildStepSummary(result, args.event) + "\n");
  }
}

// Export internals for tests.
export {
  CATEGORY_PATTERNS,
  CATEGORY_REGEXES,
  ALL_JOBS,
  HEAVY_JOBS,
  JOB_RULES,
  globToRegex,
  classifyFile,
};

// Run CLI when invoked directly (not when imported by tests).
const _isMain = argv[1] && fileURLToPath(import.meta.url) === resolve(argv[1]);
if (_isMain) {
  main();
}
