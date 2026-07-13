#!/usr/bin/env node
/**
 * Local gate mirroring Codecov patch status (codecov.yml → coverage.status.patch).
 *
 * Compares executable lines touched by the branch (vs origin/main by default)
 * against coverage/lcov.info and fails if patch coverage is below the target.
 *
 * Usage:
 *   node scripts/check-patch-coverage.mjs
 *   node scripts/check-patch-coverage.mjs --base origin/main
 *   node scripts/check-patch-coverage.mjs --run-tests   # pnpm test:coverage first
 */
import { execFileSync, execSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");

// Keep in sync with codecov.yml coverage.status.patch.default.target
const PATCH_TARGET_PERCENT = 80;

// Keep in sync with codecov.yml coverage.ignore (and vitest coverage.exclude)
const IGNORE_PREFIXES = [
  "src/lib/tauri/",
  "src/main.tsx",
  "src/workers/romanize.worker.ts",
  "src/runtime/window-shell-runtime.ts",
  "src/runtime/theme-runtime.ts",
  "src/lib/native-context-menu.ts",
  "src/components/Library/ImportCdgChoiceDialog.tsx",
  "src/components/Library/SongEditDialog.tsx",
  "src/components/Library/SongPropertiesDialog.tsx",
  "src/components/Library/ImportButton.tsx",
  "src/components/Bootstrap/ModelBootstrapBanner.tsx",
  "src/components/Cdg/CdgCanvas.tsx",
];

function parseArgs(argv) {
  let base = "origin/main";
  let runTests = false;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--base" && argv[i + 1]) {
      base = argv[++i];
    } else if (argv[i] === "--run-tests") {
      runTests = true;
    } else if (argv[i] === "--help" || argv[i] === "-h") {
      console.log(`Usage: node scripts/check-patch-coverage.mjs [--base <ref>] [--run-tests]
Mirrors Codecov patch target (${PATCH_TARGET_PERCENT}%).`);
      process.exit(0);
    }
  }
  return { base, runTests };
}

function isIgnored(relPath) {
  const normalized = relPath.replaceAll("\\", "/");
  return IGNORE_PREFIXES.some(
    (p) => normalized === p || normalized.startsWith(p),
  );
}

function git(args) {
  return execFileSync("git", args, {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
}

/**
 * Parse unified diff for added executable-ish source lines.
 * Returns Map<relPath, Set<lineNumber>> of added line numbers in the new file.
 */
function parseAddedLines(diffText) {
  const result = new Map();
  let file = null;
  let newLine = 0;

  for (const line of diffText.split("\n")) {
    if (line.startsWith("+++ b/")) {
      file = line.slice(6).trim();
      if (!result.has(file)) {
        result.set(file, new Set());
      }
      continue;
    }
    if (line.startsWith("@@")) {
      // @@ -a,b +c,d @@
      const m = line.match(/\+(\d+)(?:,(\d+))?/);
      newLine = m ? Number(m[1]) : 0;
      continue;
    }
    if (!file) {
      continue;
    }
    if (line.startsWith("+") && !line.startsWith("+++")) {
      result.get(file)?.add(newLine);
      newLine += 1;
    } else if (line.startsWith("-") && !line.startsWith("---")) {
      // deleted line in old file — do not advance newLine
    } else {
      // context line
      newLine += 1;
    }
  }
  return result;
}

function parseLcov(lcovText) {
  /** @type {Map<string, Map<number, number>>} */
  const hits = new Map();
  let current = null;
  for (const line of lcovText.split("\n")) {
    if (line.startsWith("SF:")) {
      let sf = line.slice(3).trim();
      // Normalize to repo-relative path
      if (path.isAbsolute(sf)) {
        sf = path.relative(root, sf);
      }
      sf = sf.replaceAll("\\", "/");
      current = sf;
      if (!hits.has(current)) {
        hits.set(current, new Map());
      }
    } else if (current && line.startsWith("DA:")) {
      const [n, h] = line.slice(3).split(",");
      hits.get(current)?.set(Number(n), Number(h));
    }
  }
  return hits;
}

function main() {
  const { base, runTests } = parseArgs(process.argv.slice(2));

  if (runTests) {
    console.log("Running pnpm test:coverage …");
    execSync("pnpm test:coverage", { cwd: root, stdio: "inherit" });
  }

  const lcovPath = path.join(root, "coverage", "lcov.info");
  if (!existsSync(lcovPath)) {
    console.error(
      "Missing coverage/lcov.info. Run with --run-tests or pnpm test:coverage first.",
    );
    process.exit(2);
  }

  let mergeBase;
  try {
    mergeBase = git(["merge-base", "HEAD", base]).trim();
  } catch {
    console.error(`Could not resolve merge-base with ${base}. Fetch it first.`);
    process.exit(2);
  }

  const diff = git(["diff", `${mergeBase}...HEAD`, "--", "src/"]);
  const added = parseAddedLines(diff);
  const lcov = parseLcov(readFileSync(lcovPath, "utf8"));

  let total = 0;
  let covered = 0;
  let missing = 0;
  const missingDetails = [];

  for (const [file, lines] of added) {
    if (!file.startsWith("src/")) {
      continue;
    }
    if (!/\.(ts|tsx|js|jsx)$/.test(file)) {
      continue;
    }
    if (file.endsWith(".test.ts") || file.endsWith(".test.tsx")) {
      continue;
    }
    if (isIgnored(file)) {
      continue;
    }

    const fileHits = lcov.get(file);
    if (!fileHits) {
      // File not in LCOV — treat added lines as missed if any look executable.
      // Prefer only counting lines that appear in LCOV for other files; if the
      // whole file is absent, skip (likely excluded or not instrumented).
      continue;
    }

    for (const lineNo of lines) {
      if (!fileHits.has(lineNo)) {
        // Not an instrumented line (comment, type-only, blank).
        continue;
      }
      total += 1;
      const hit = fileHits.get(lineNo) ?? 0;
      if (hit > 0) {
        covered += 1;
      } else {
        missing += 1;
        missingDetails.push(`${file}:${lineNo}`);
      }
    }
  }

  if (total === 0) {
    console.log(
      "Patch coverage: no instrumented src lines in diff (nothing to check).",
    );
    process.exit(0);
  }

  const pct = (covered / total) * 100;
  const pctRounded = Math.floor(pct * 10) / 10; // codecov precision: 1, round: down
  console.log(
    `Patch coverage: ${pctRounded}% (${covered}/${total} instrumented lines); target ${PATCH_TARGET_PERCENT}%`,
  );
  if (missingDetails.length > 0) {
    console.log(`Missing (${missing}):`);
    for (const d of missingDetails.slice(0, 40)) {
      console.log(`  ${d}`);
    }
    if (missingDetails.length > 40) {
      console.log(`  …and ${missingDetails.length - 40} more`);
    }
  }

  if (pctRounded < PATCH_TARGET_PERCENT) {
    console.error(
      `FAIL: patch coverage ${pctRounded}% is below Codecov target ${PATCH_TARGET_PERCENT}%.`,
    );
    process.exit(1);
  }
  console.log("PASS: patch coverage meets Codecov target.");
}

main();
