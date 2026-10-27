// Drift-protection tests for the CI workflow.
//
// These tests parse .github/workflows/ci.yml and verify that:
//   1. Every expected job ID emitted by the classifier has a corresponding
//      workflow job.
//   2. Every optional expensive workflow job is represented in CI Gate's
//      dependency list.
//   3. No broad `workflows` boolean appears in app-job conditions (the old
//      broken pattern from PR #150).
//   4. CI Gate reads the classifier's expected-jobs output.
//   5. Packaging build jobs and aggregate gates agree for every trigger.
//   6. The Playwright config and workflow agree on report generation.
//   7. The Playwright job runs in the official image pinned by digest, at
//      the same version as the lockfile, without the apt-based install.

import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { runInNewContext } from "node:vm";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";
import {
  ALL_JOBS,
  classifyChanges,
} from "../../scripts/ci/classify-changes.mjs";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));

function readProjectFile(path: string) {
  return readFileSync(join(projectRoot, path), "utf8");
}

/** Extract job IDs from a GitHub Actions workflow YAML. */
function extractJobIds(yaml: string): string[] {
  const jobs: string[] = [];
  // Match top-level job keys under `jobs:` — 2-space indent, key: at end of line
  // or followed by value.
  const lines = yaml.split(/\r?\n/);
  let inJobs = false;
  for (const line of lines) {
    if (/^jobs:\s*$/.test(line)) {
      inJobs = true;
      continue;
    }
    if (!inJobs) continue;
    // Job keys are exactly 2 spaces indent, alphanumeric/kebab-case, followed
    // by `:`. Stop at non-indented lines (end of jobs block).
    if (/^[a-zA-Z]/.test(line)) {
      inJobs = false;
      continue;
    }
    const match = line.match(/^  ([a-z0-9-]+):\s*$/);
    if (match) {
      jobs.push(match[1]);
    }
  }
  return jobs;
}

/** Extract the ci-gate `needs:` list from the workflow YAML. */
function extractGateNeeds(yaml: string): string[] {
  const lines = yaml.split(/\r?\n/);
  let inGate = false;
  let inNeeds = false;
  const needs: string[] = [];
  for (const line of lines) {
    if (/^  ci-gate:\s*$/.test(line)) {
      inGate = true;
      continue;
    }
    if (!inGate) continue;
    if (/^[a-z]/.test(line) && !line.startsWith(" ")) {
      break; // left the ci-gate job block
    }
    if (/^    needs:\s*$/.test(line)) {
      inNeeds = true;
      continue;
    }
    if (inNeeds) {
      // Needs entries are 6-space indent list items.
      const match = line.match(/^      - "?([a-z0-9-]+)"?\s*$/);
      if (match) {
        needs.push(match[1]);
      } else if (!line.startsWith("        ") && !line.match(/^\s*-/)) {
        // End of needs list (next key at 4-space indent)
        if (/^    [a-z]/.test(line)) {
          inNeeds = false;
        }
      }
    }
  }
  return needs;
}

// ── Tests ─────────────────────────────────────────────────────────────

describe("CI workflow drift protection", () => {
  test("keeps CI toolchains aligned with local version pins", () => {
    const node = readProjectFile(".node-version").trim();
    const rust = readProjectFile("rust-toolchain.toml").match(
      /channel = "([^"]+)"/,
    )?.[1];
    expect(rust).toBeTruthy();
    expect(readProjectFile("mise.toml")).toContain(`node = "${node}"`);
    for (const file of readdirSync(join(projectRoot, ".github/workflows"))) {
      if (!file.endsWith(".yml")) continue;
      const workflow = readProjectFile(`.github/workflows/${file}`);
      for (const [, version] of workflow.matchAll(/node-version: ([\d.]+)/g)) {
        expect(version, `${file}: Node.js`).toBe(node);
      }
      for (const [, version] of workflow.matchAll(/toolchain: ([\d.]+)/g)) {
        expect(version, `${file}: Rust`).toBe(rust);
      }
    }
  });

  const ciYaml = readProjectFile(".github/workflows/ci.yml");
  const workflowJobs = extractJobIds(ciYaml);

  test("every classifier ALL_JOBS entry has a matching workflow job", () => {
    for (const job of ALL_JOBS) {
      expect(workflowJobs, `workflow missing job: ${job}`).toContain(job);
    }
  });

  test("every workflow job has a classifier entry (no orphan jobs)", () => {
    for (const job of workflowJobs) {
      expect(ALL_JOBS, `classifier missing job: ${job}`).toContain(job);
    }
  });

  test("ci-gate depends on every other workflow job", () => {
    const gateNeeds = extractGateNeeds(ciYaml);
    const otherJobs = workflowJobs.filter((j) => j !== "ci-gate");
    for (const job of otherJobs) {
      expect(gateNeeds, `ci-gate missing needs entry: ${job}`).toContain(job);
    }
  });

  test("no broad `workflows` boolean in app-job conditions", () => {
    // The old pattern used `needs.triage.outputs.workflows == 'true'` in
    // app-job conditions, which caused any workflow edit to trigger full
    // app CI. The classifier no longer emits a `workflows` output.
    expect(ciYaml).not.toMatch(/outputs\.workflows/);
  });

  test("triage job runs the classifier script", () => {
    expect(ciYaml).toContain("scripts/ci/classify-changes.mjs");
  });

  test("triage outputs expected-jobs for CI Gate", () => {
    expect(ciYaml).toContain("expected-jobs");
  });

  test("triage outputs per-job run_ booleans", () => {
    // At least one run_ output must appear.
    expect(ciYaml).toMatch(/run_/);
  });

  test("app jobs use run_ outputs instead of category booleans", () => {
    // App jobs should check `run_app-frontend == 'true'`, not
    // `frontend == 'true'`.
    expect(ciYaml).toContain("run_app-frontend");
    expect(ciYaml).toContain("run_js-quality");
    expect(ciYaml).toContain("run_rust-test");
    expect(ciYaml).toContain("run_rust-test-macos");
    expect(ciYaml).toContain("run_rust-test-windows-compile");
    expect(ciYaml).toContain("run_tauri-build-smoke");
    expect(ciYaml).toContain("run_tauri-build");
    expect(ciYaml).toContain("run_playwright-ui-smoke");
  });

  test("ci.yml triggers on main push, PR (with label updates), nightly schedule, and manual dispatch", () => {
    expect(ciYaml).toMatch(
      /pull_request:\s*\n\s+types:\s*\[opened,\s*synchronize,\s*reopened,\s*labeled,\s*unlabeled\]/,
    );
    expect(ciYaml).toMatch(/schedule:\s*\n\s+-\s*cron:/);
    expect(ciYaml).toContain("workflow_dispatch:");
    expect(ciYaml).toContain(
      "PR_LABELS: ${{ toJson(github.event.pull_request.labels.*.name) }}",
    );
    expect(ciYaml).toContain('--labels "$PR_LABELS"');
  });

  test("rust-test runs on Linux with Rust caching and rust-test-macos runs on macOS", () => {
    const rustLinux = extractJobSection(ciYaml, "rust-test");
    expect(rustLinux).toContain("runs-on: ubuntu-22.04");
    expect(rustLinux).toContain("swatinem/rust-cache@");
    expect(rustLinux).not.toContain("macos-14");

    const rustMacos = extractJobSection(ciYaml, "rust-test-macos");
    expect(rustMacos).toContain("runs-on: macos-14");
    expect(rustMacos).toContain("swatinem/rust-cache@");
    expect(rustMacos).toContain(
      "needs.triage.outputs.run_rust-test-macos == 'true'",
    );
  });

  test("ci-gate verifies expected jobs against actual results", () => {
    // The gate must read expected-jobs and compare against actual results.
    expect(ciYaml).toContain("EXPECTED_JOBS");
  });

  test("workflow lint runs without a path or triage dependency", () => {
    const lintJob = ciYaml.split("  workflow-lint:\n")[1].split("\n  #")[0];
    expect(lintJob).not.toMatch(/^    (if|needs):/m);
  });

  test("ci-gate rejects skipped workflow lint on a docs-only PR", () => {
    const classification = classifyChanges(["docs/guide.md"], "pull_request");
    const gate = ciYaml.split("  ci-gate:")[1].split("        run: |\n")[1];
    for (const status of ["success", "failure", "cancelled", "skipped"]) {
      const env: NodeJS.ProcessEnv = {
        ...process.env,
        EXPECTED_JOBS: JSON.stringify(classification.expectedJobs),
        UNKNOWN_FLAG: "false",
        WORKFLOW_LINT: status,
      };
      for (const job of ALL_JOBS) {
        if (job === "workflow-lint") continue;
        env[job.replaceAll("-", "_").toUpperCase()] = classification.run[job]
          ? "success"
          : "skipped";
      }
      const result = spawnSync("bash", ["-c", gate], { env });
      expect(
        result.status,
        result.stdout.toString() + result.stderr.toString(),
      ).toBe(status === "success" ? 0 : 1);
    }
  });

  test("ci-gate accepts fast-only PR classification even when unknown files exist", () => {
    const classification = classifyChanges(
      ["new-unmapped-root-config.xyz"],
      "pull_request",
    );
    const gate = ciYaml.split("  ci-gate:")[1].split("        run: |\n")[1];
    const env: NodeJS.ProcessEnv = {
      ...process.env,
      EXPECTED_JOBS: JSON.stringify(classification.expectedJobs),
      UNKNOWN_FLAG: "true",
    };
    for (const job of ALL_JOBS) {
      env[job.replaceAll("-", "_").toUpperCase()] = classification.run[job]
        ? "success"
        : "skipped";
    }
    const result = spawnSync("bash", ["-c", gate], { env });
    expect(
      result.status,
      result.stdout.toString() + result.stderr.toString(),
    ).toBe(0);
  });

  test("prepare-model does not run for frontend-only or docs-only changes", () => {
    // The prepare-model job should use run_prepare-model, not a broad
    // category OR that includes frontend.
    const prepareModelSection = ciYaml.match(
      /prepare-model:[\s\S]*?runs-on:/,
    )?.[0];
    expect(prepareModelSection).toBeDefined();
    expect(prepareModelSection).toContain("run_prepare-model");
  });
});

describe("Playwright report contract", () => {
  const playwrightConfig = readProjectFile("playwright.config.ts");
  const ciYaml = readProjectFile(".github/workflows/ci.yml");

  test("playwright config generates HTML report in CI", () => {
    // The config must produce an HTML report when running in CI so the
    // upload-artifact step has files to upload.
    expect(playwrightConfig).toMatch(/html/);
    expect(playwrightConfig).toMatch(/open:\s*["']never["']/);
  });

  test("workflow uploads playwright-report with if-no-files-found", () => {
    // The upload step must use if-no-files-found to catch contract drift
    // between the reporter and the upload path.
    const uploadSection = ciYaml.match(
      /Upload Playwright report[\s\S]*?retention-days:/,
    )?.[0];
    expect(uploadSection).toBeDefined();
    expect(uploadSection).toContain("if-no-files-found");
  });
});

describe("Packaging workflow contract", () => {
  const packagingYaml = readProjectFile(".github/workflows/packaging.yml");

  test("WinGet validation uses a published stable release", () => {
    expect(packagingYaml).toMatch(
      /gh release list[^\n]*--exclude-drafts[^\n]*--exclude-pre-releases/,
    );
  });

  test("release.yml is not a packaging trigger", () => {
    // A release-notes text edit must not trigger full packaging builds.
    expect(packagingYaml).not.toMatch(/\.github\/workflows\/release\.yml/);
  });

  test("no codex/** in push branches (prevents duplicate runs)", () => {
    // The main CI removed codex/** from push triggers; packaging must do
    // the same to avoid duplicate push + PR runs.
    expect(packagingYaml).not.toMatch(/codex\/\*\*/);
  });

  test("build-flatpak gates on packaging inputs", () => {
    // The flatpak build must not run for release-workflow-only changes.
    const buildFlatpakSection = packagingYaml.match(
      /build-flatpak:[\s\S]*?runs-on:/,
    )?.[0];
    expect(buildFlatpakSection).toBeDefined();
    // build-flatpak should have an if: condition that gates on packaging
    // inputs, not just the workflow trigger.
    expect(buildFlatpakSection).toMatch(/if:/);
  });

  test.each([
    ["pull_request", false, true],
    ["pull_request", false, false],
    ["pull_request", true, true],
    ["pull_request", true, false],
    ["push", true, true],
    ["push", true, false],
    ["schedule", true, false],
    ["workflow_dispatch", true, false],
  ])(
    "%s full_ci=%s packaging changes=%s has matching build and gate requirements",
    (event, fullCi, changed) => {
      const condition = packagingYaml.match(
        /  build-flatpak:[\s\S]*?    if: \|\n([\s\S]*?)    runs-on:/,
      )?.[1];
      expect(condition).toBeDefined();
      const gate = packagingYaml
        .split("  packaging-gate:")[1]
        .split("        run: |\n")[1];
      expect(gate).toBeDefined();
      const required =
        fullCi &&
        (changed || event === "schedule" || event === "workflow_dispatch");
      for (const validation of ["success", "failure"]) {
        const expression = condition!
          .replace(/!cancelled\(\)/g, "true")
          .replace(
            /needs\.validate-flatpak\.result/g,
            JSON.stringify(validation),
          )
          .replace(
            /needs\.triage\.outputs\.full_ci/g,
            JSON.stringify(String(fullCi)),
          )
          .replace(
            /needs\.triage\.outputs\.packaging_(inputs|workflow)/g,
            JSON.stringify(String(changed)),
          )
          .replace(/github\.event_name/g, JSON.stringify(event));
        expect(runInNewContext(expression)).toBe(
          required && validation === "success",
        );
      }
      for (const field of [
        "FLATPAK_BUILD",
        "FLATPAK_VAL",
        "WINGET",
        "TRIAGE",
      ]) {
        for (const status of ["success", "failure", "cancelled", "skipped"]) {
          const result = spawnSync("bash", ["-c", gate], {
            env: {
              ...process.env,
              TRIAGE: "success",
              WINGET: required ? "success" : "skipped",
              FLATPAK_VAL: required ? "success" : "skipped",
              FLATPAK_BUILD: required ? "success" : "skipped",
              FULL_CI: String(fullCi),
              PKG_INPUTS: String(changed),
              PKG_WORKFLOW: "false",
              REL_META: "false",
              EVENT_NAME: event,
              [field]: status,
            },
          });
          expect(result.status).toBe(
            status === (required || field === "TRIAGE" ? "success" : "skipped")
              ? 0
              : 1,
          );
        }
      }
    },
  );

  test("packaging-gate depends on all other packaging workflow jobs", () => {
    const jobs = extractJobIds(packagingYaml);
    expect(jobs).toContain("packaging-gate");
    const gateBlock = packagingYaml.match(
      /packaging-gate:[\s\S]*?runs-on:/,
    )?.[0];
    expect(gateBlock).toBeDefined();
    for (const job of [
      "triage",
      "validate-winget",
      "validate-flatpak",
      "build-flatpak",
    ]) {
      expect(gateBlock).toContain(`- ${job}`);
    }
  });

  test("packaging workflow does not filter pull_request by paths so Packaging Gate is always reporting", () => {
    // Top-level pull_request must not carry paths filter; triage handles gating.
    const prTrigger = packagingYaml.match(/pull_request:\s*\n\s+paths:/);
    expect(prTrigger).toBeNull();
  });
});

describe("Windows installed-app gate workflow contract", () => {
  const winGateYaml = readProjectFile(
    ".github/workflows/windows-installed-app-smoke-test.yml",
  );

  test("runs on main push, nightly schedule, manual dispatch, and PRs labeled ci:full", () => {
    expect(winGateYaml).toMatch(/push:\s*\n\s+branches:\s*\n\s+-\s*main/);
    expect(winGateYaml).toMatch(/schedule:\s*\n\s+-\s*cron:/);
    expect(winGateYaml).toContain("workflow_dispatch:");
    expect(winGateYaml).toContain("pull_request:");
    expect(winGateYaml).toContain(
      "contains(github.event.pull_request.labels.*.name, 'ci:full')",
    );
  });
});

/** Return the text of one top-level job (2-space indented key) in ci.yml. */
function extractJobSection(yaml: string, jobId: string): string {
  const start = yaml.indexOf(`\n  ${jobId}:\n`);
  expect(start, `job ${jobId} not found`).toBeGreaterThanOrEqual(0);
  const rest = yaml.slice(start + 1);
  const next = rest.slice(1).search(/\n {2}[a-z0-9-]+:\n/);
  return next === -1 ? rest : rest.slice(0, next + 1);
}

/** Return a `run: |` block of a named step, de-indented. */
function extractStepScript(jobSection: string, stepName: string): string {
  const step = jobSection.split(`- name: ${stepName}\n`)[1];
  expect(step, `step ${stepName} not found`).toBeDefined();
  const body = step.split("run: |\n")[1].split(/\n {6}- name: /)[0];
  const lines = body.split("\n");
  const indent = lines[0].match(/^ */)![0].length;
  return lines.map((line) => line.slice(indent)).join("\n");
}

describe("Playwright container contract", () => {
  const ciYaml = readProjectFile(".github/workflows/ci.yml");
  const lockfile = readProjectFile("pnpm-lock.yaml");
  const job = extractJobSection(ciYaml, "playwright-ui-smoke");

  test("job runs in the official Playwright image pinned by digest", () => {
    const image = job.match(/^ {6}image: (\S+)(?: # (\S+))?$/m);
    expect(image, "container image line").not.toBeNull();
    expect(image![1]).toMatch(
      /^mcr\.microsoft\.com\/playwright@sha256:[0-9a-f]{64}$/,
    );
    // The comment next to the digest names the tag it was resolved from.
    expect(image![2]).toMatch(/^v\d+\.\d+\.\d+-noble$/);
  });

  test("image tag matches the playwright version in pnpm-lock.yaml", () => {
    const locked = [
      ...lockfile.matchAll(/^ {2}playwright@(\d+\.\d+\.\d+):$/gm),
    ].map((m) => m[1]);
    expect(locked.length, "playwright entry in pnpm-lock.yaml").toBeGreaterThan(
      0,
    );
    expect(new Set(locked).size, "a single locked playwright version").toBe(1);
    const tag = job.match(/^ {6}image: \S+ # v(\d+\.\d+\.\d+)-noble$/m)?.[1];
    expect(tag).toBe(locked[0]);
  });

  test("job does not install browser system deps through apt", () => {
    // Ignore comment lines, which explain why these are gone.
    const code = job
      .split("\n")
      .filter((line) => !/^\s*(#|\/\/)/.test(line))
      .join("\n");
    expect(code).not.toMatch(/--with-deps/);
    expect(code).not.toMatch(/install-deps/);
    expect(code).not.toMatch(/apt-get/);
    // jq is not in the Playwright image.
    expect(code).not.toMatch(/\bjq\b/);
    expect(code).toMatch(/^ {4}timeout-minutes: 30$/m);
    // Container jobs default `run:` to `sh -e`; the steps use bash options.
    expect(code).toMatch(/^ {4}defaults:\n {6}run:\n {8}shell: bash$/m);
  });

  test("flaky report step parses a Playwright JSON report without jq", () => {
    const script = extractStepScript(job, "Report flaky tests");
    const dir = mkdtempSync(join(tmpdir(), "pw-flaky-"));
    mkdirSync(join(dir, "playwright-report"));
    writeFileSync(
      join(dir, "playwright-report", "results.json"),
      JSON.stringify({
        suites: [
          {
            specs: [{ title: "stable", tests: [{ status: "expected" }] }],
            suites: [
              {
                specs: [{ title: "wobbly", tests: [{ status: "flaky" }] }],
              },
            ],
          },
        ],
        stats: { flaky: 1, unexpected: 0 },
      }),
    );
    const summary = join(dir, "summary.md");
    const result = spawnSync("bash", ["-e", "-c", script], {
      cwd: dir,
      env: { ...process.env, GITHUB_STEP_SUMMARY: summary },
    });
    const output = result.stdout.toString() + result.stderr.toString();
    expect(result.status, output).toBe(0);
    const written = readFileSync(summary, "utf8");
    expect(written).toContain("**Flaky (passed on retry):** 1");
    expect(written).toContain("**Failed:** 0");
    expect(written).toContain("- `wobbly`");
    expect(written).not.toContain("`stable`");
    expect(output).toContain("::warning::1 flaky Playwright test(s) detected");
  });
});

function withStubbedGh(
  existingIssueNumber: string,
  runJobsJson: string,
  fn: (ctx: { dir: string; callLog: string; bodyLog: string }) => void,
) {
  const dir = mkdtempSync(join(tmpdir(), "ci-failure-issue-"));
  const callLog = join(dir, "gh-calls.log");
  const bodyLog = join(dir, "gh-body.md");
  const jobsFile = join(dir, "run-jobs.json");
  try {
    if (runJobsJson) {
      writeFileSync(jobsFile, runJobsJson);
    }
    writeFileSync(
      join(dir, "gh"),
      `#!/usr/bin/env bash
set -euo pipefail
printf '%s\\n' "$*" >> "${callLog}"
if [ "\${1:-}" = "run" ] && [ "\${2:-}" = "view" ]; then
  if [ -f "${jobsFile}" ]; then
    jq -r "\${@: -1}" "${jobsFile}"
  fi
  exit 0
fi
if [ "\${1:-}" = "issue" ] && [ "\${2:-}" = "list" ]; then
  printf '%s\\n' "${existingIssueNumber}"
  exit 0
fi
if [ "\${1:-}" = "issue" ] && { [ "\${2:-}" = "create" ] || [ "\${2:-}" = "comment" ]; }; then
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--body" ]; then
      printf '%s\\n' "$2" > "${bodyLog}"
      break
    fi
    shift
  done
  printf 'https://github.com/thedavidweng/OpenKara/issues/99\\n'
  exit 0
fi
`,
      { mode: 0o755 },
    );
    fn({ dir, callLog, bodyLog });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

describe("Main and nightly failure tracking contract", () => {
  const ciYaml = readProjectFile(".github/workflows/ci.yml");

  test("ci.yml scopes issues: write to ci-gate and keeps top-level permissions read-only", () => {
    expect(ciYaml).toMatch(/^permissions:\n  contents: read$/m);
    const gateSection = extractJobSection(ciYaml, "ci-gate");
    expect(gateSection).toContain("issues: write");
  });

  test("ci-gate opens a new tracking issue on main push failure and comments on existing issue on nightly failure", () => {
    const gateSection = extractJobSection(ciYaml, "ci-gate");
    const script = extractStepScript(
      gateSection,
      "Verify expected jobs succeeded and non-expected jobs skipped",
    );
    const classification = classifyChanges(["src/App.tsx"], "push");
    const runJobs = JSON.stringify({
      jobs: [
        {
          name: "Tauri build (macos-14)",
          conclusion: "failure",
          url: "https://github.com/thedavidweng/OpenKara/actions/runs/1001/job/2002",
        },
        {
          name: "Rust tests (ubuntu-22.04)",
          conclusion: "success",
          url: "https://github.com/thedavidweng/OpenKara/actions/runs/1001/job/2003",
        },
      ],
    });

    // 1. Main push failure with no open tracking issue -> creates new issue with run + job links
    withStubbedGh("", runJobs, ({ dir, callLog, bodyLog }) => {
      const env: NodeJS.ProcessEnv = {
        ...process.env,
        PATH: `${dir}:${process.env.PATH}`,
        GH_TOKEN: "test-token",
        GH_REPO: "thedavidweng/OpenKara",
        EVENT_NAME: "push",
        REF_NAME: "main",
        WORKFLOW_NAME: "CI",
        RUN_ID: "1001",
        RUN_URL: "https://github.com/thedavidweng/OpenKara/actions/runs/1001",
        HEAD_SHA: "0123456789abcdef0123456789abcdef01234567",
        EXPECTED_JOBS: JSON.stringify(classification.expectedJobs),
        UNKNOWN_FLAG: "false",
      };
      for (const job of ALL_JOBS) {
        env[job.replaceAll("-", "_").toUpperCase()] =
          job === "tauri-build"
            ? "failure"
            : classification.run[job]
              ? "success"
              : "skipped";
      }
      const result = spawnSync("bash", ["-c", script], {
        env,
        encoding: "utf8",
      });
      expect(result.status).toBe(1);
      expect(existsSync(callLog)).toBe(true);
      const calls = readFileSync(callLog, "utf8");
      expect(calls).toContain(
        "issue list --repo thedavidweng/OpenKara --state open --search in:title CI failure on main/nightly",
      );
      expect(calls).toContain("issue create --repo thedavidweng/OpenKara");
      expect(calls).toContain("--label bug");
      const body = readFileSync(bodyLog, "utf8");
      expect(body).toContain(
        "https://github.com/thedavidweng/OpenKara/actions/runs/1001",
      );
      expect(body).toContain(
        "[Tauri build (macos-14)](https://github.com/thedavidweng/OpenKara/actions/runs/1001/job/2002)",
      );
    });

    // 2. Nightly schedule failure with existing open tracking issue #42 -> comments on #42 instead of opening a new one
    withStubbedGh("42", runJobs, ({ dir, callLog, bodyLog }) => {
      const env: NodeJS.ProcessEnv = {
        ...process.env,
        PATH: `${dir}:${process.env.PATH}`,
        GH_TOKEN: "test-token",
        GH_REPO: "thedavidweng/OpenKara",
        EVENT_NAME: "schedule",
        REF_NAME: "main",
        WORKFLOW_NAME: "CI",
        RUN_ID: "1002",
        RUN_URL: "https://github.com/thedavidweng/OpenKara/actions/runs/1002",
        HEAD_SHA: "0123456789abcdef0123456789abcdef01234567",
        EXPECTED_JOBS: JSON.stringify(classification.expectedJobs),
        UNKNOWN_FLAG: "false",
      };
      for (const job of ALL_JOBS) {
        env[job.replaceAll("-", "_").toUpperCase()] =
          job === "tauri-build"
            ? "failure"
            : classification.run[job]
              ? "success"
              : "skipped";
      }
      const result = spawnSync("bash", ["-c", script], {
        env,
        encoding: "utf8",
      });
      expect(result.status).toBe(1);
      const calls = readFileSync(callLog, "utf8");
      expect(calls).toContain(
        "issue comment 42 --repo thedavidweng/OpenKara --body",
      );
      expect(calls).not.toContain("issue create");
      const body = readFileSync(bodyLog, "utf8");
      expect(body).toContain(
        "https://github.com/thedavidweng/OpenKara/actions/runs/1002",
      );
      expect(body).toContain(
        "[Tauri build (macos-14)](https://github.com/thedavidweng/OpenKara/actions/runs/1001/job/2002)",
      );
    });
  });

  test("ci-gate does not open or update an issue on green runs or PR failures", () => {
    const gateSection = extractJobSection(ciYaml, "ci-gate");
    const script = extractStepScript(
      gateSection,
      "Verify expected jobs succeeded and non-expected jobs skipped",
    );

    // 1. Green main push run -> no issue calls
    const pushClass = classifyChanges(["src/App.tsx"], "push");
    withStubbedGh("", "", ({ dir, callLog }) => {
      const env: NodeJS.ProcessEnv = {
        ...process.env,
        PATH: `${dir}:${process.env.PATH}`,
        GH_TOKEN: "test-token",
        GH_REPO: "thedavidweng/OpenKara",
        EVENT_NAME: "push",
        REF_NAME: "main",
        WORKFLOW_NAME: "CI",
        RUN_ID: "1003",
        RUN_URL: "https://github.com/thedavidweng/OpenKara/actions/runs/1003",
        EXPECTED_JOBS: JSON.stringify(pushClass.expectedJobs),
        UNKNOWN_FLAG: "false",
      };
      for (const job of ALL_JOBS) {
        env[job.replaceAll("-", "_").toUpperCase()] = pushClass.run[job]
          ? "success"
          : "skipped";
      }
      const result = spawnSync("bash", ["-c", script], {
        env,
        encoding: "utf8",
      });
      expect(result.status).toBe(0);
      expect(existsSync(callLog)).toBe(false);
    });

    // 2. Red PR run (even with ci:full) -> no issue calls
    const prFullClass = classifyChanges(["src/App.tsx"], "pull_request", [
      "ci:full",
    ]);
    withStubbedGh("", "", ({ dir, callLog }) => {
      const env: NodeJS.ProcessEnv = {
        ...process.env,
        PATH: `${dir}:${process.env.PATH}`,
        GH_TOKEN: "test-token",
        GH_REPO: "thedavidweng/OpenKara",
        EVENT_NAME: "pull_request",
        REF_NAME: "feature-branch",
        WORKFLOW_NAME: "CI",
        RUN_ID: "1004",
        RUN_URL: "https://github.com/thedavidweng/OpenKara/actions/runs/1004",
        EXPECTED_JOBS: JSON.stringify(prFullClass.expectedJobs),
        UNKNOWN_FLAG: "false",
      };
      for (const job of ALL_JOBS) {
        env[job.replaceAll("-", "_").toUpperCase()] =
          job === "tauri-build"
            ? "failure"
            : prFullClass.run[job]
              ? "success"
              : "skipped";
      }
      const result = spawnSync("bash", ["-c", script], {
        env,
        encoding: "utf8",
      });
      expect(result.status).toBe(1);
      expect(existsSync(callLog)).toBe(false);
    });
  });

  test("packaging-gate opens or updates a tracking issue only on main or nightly failures", () => {
    const packagingYaml = readProjectFile(".github/workflows/packaging.yml");
    expect(packagingYaml).toMatch(/^permissions:\n  contents: read$/m);
    const gateSection = extractJobSection(packagingYaml, "packaging-gate");
    expect(gateSection).toContain("issues: write");
    const script = extractStepScript(gateSection, "Verify packaging status");

    withStubbedGh("", "", ({ dir, callLog, bodyLog }) => {
      const result = spawnSync("bash", ["-c", script], {
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${dir}:${process.env.PATH}`,
          GH_TOKEN: "test-token",
          GH_REPO: "thedavidweng/OpenKara",
          EVENT_NAME: "schedule",
          REF_NAME: "main",
          WORKFLOW_NAME: "Packaging",
          RUN_ID: "2001",
          RUN_URL: "https://github.com/thedavidweng/OpenKara/actions/runs/2001",
          HEAD_SHA: "0123456789abcdef0123456789abcdef01234567",
          TRIAGE: "success",
          WINGET: "success",
          FLATPAK_VAL: "success",
          FLATPAK_BUILD: "failure",
          FULL_CI: "true",
          PKG_INPUTS: "false",
          PKG_WORKFLOW: "false",
          REL_META: "false",
        },
      });
      expect(result.status).toBe(1);
      expect(existsSync(callLog)).toBe(true);
      const calls = readFileSync(callLog, "utf8");
      expect(calls).toContain("issue create --repo thedavidweng/OpenKara");
      const body = readFileSync(bodyLog, "utf8");
      expect(body).toContain(
        "https://github.com/thedavidweng/OpenKara/actions/runs/2001",
      );
      expect(body).toContain(
        "[build-flatpak](https://github.com/thedavidweng/OpenKara/actions/runs/2001)",
      );
    });

    // PR failure must not open an issue
    withStubbedGh("", "", ({ dir, callLog }) => {
      const result = spawnSync("bash", ["-c", script], {
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${dir}:${process.env.PATH}`,
          GH_TOKEN: "test-token",
          GH_REPO: "thedavidweng/OpenKara",
          EVENT_NAME: "pull_request",
          REF_NAME: "feature-branch",
          WORKFLOW_NAME: "Packaging",
          RUN_ID: "2002",
          RUN_URL: "https://github.com/thedavidweng/OpenKara/actions/runs/2002",
          TRIAGE: "success",
          WINGET: "success",
          FLATPAK_VAL: "success",
          FLATPAK_BUILD: "failure",
          FULL_CI: "true",
          PKG_INPUTS: "true",
          PKG_WORKFLOW: "false",
          REL_META: "false",
        },
      });
      expect(result.status).toBe(1);
      expect(existsSync(callLog)).toBe(false);
    });
  });

  test("windows-installed-app-smoke-test and nightly-hardening report main/nightly failures via tracking issues", () => {
    const winGateYaml = readProjectFile(
      ".github/workflows/windows-installed-app-smoke-test.yml",
    );
    expect(winGateYaml).toMatch(/^permissions:\n  contents: read$/m);
    const winReportJob = extractJobSection(winGateYaml, "report-failure");
    expect(winReportJob).toContain("issues: write");
    expect(winReportJob).toContain("github.event_name == 'schedule'");
    expect(winReportJob).toContain("github.event_name == 'push'");
    expect(winReportJob).not.toContain("pull_request");
    const winScript = extractStepScript(
      winReportJob,
      "Open or update tracking issue",
    );

    withStubbedGh("77", "", ({ dir, callLog, bodyLog }) => {
      const result = spawnSync("bash", ["-c", winScript], {
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${dir}:${process.env.PATH}`,
          GH_TOKEN: "test-token",
          GH_REPO: "thedavidweng/OpenKara",
          EVENT_NAME: "push",
          REF_NAME: "main",
          WORKFLOW_NAME: "Branch Windows installed-app gate",
          RUN_ID: "3001",
          RUN_URL: "https://github.com/thedavidweng/OpenKara/actions/runs/3001",
          HEAD_SHA: "0123456789abcdef0123456789abcdef01234567",
          WINDOWS_INSTALLED_APP: "failure",
        },
      });
      expect(result.status).toBe(0);
      const calls = readFileSync(callLog, "utf8");
      expect(calls).toContain(
        "issue comment 77 --repo thedavidweng/OpenKara --body",
      );
      const body = readFileSync(bodyLog, "utf8");
      expect(body).toContain(
        "https://github.com/thedavidweng/OpenKara/actions/runs/3001",
      );
      expect(body).toContain("windows-installed-app");
    });

    const nightlyYaml = readProjectFile(
      ".github/workflows/nightly-hardening.yml",
    );
    expect(nightlyYaml).toMatch(/^permissions:\n  contents: read$/m);
    const nightlyReportJob = extractJobSection(nightlyYaml, "report-failure");
    expect(nightlyReportJob).toContain("issues: write");
    expect(nightlyReportJob).toContain("github.event_name == 'schedule'");
    const nightlyScript = extractStepScript(
      nightlyReportJob,
      "Open or update tracking issue",
    );

    withStubbedGh("", "", ({ dir, callLog, bodyLog }) => {
      const result = spawnSync("bash", ["-c", nightlyScript], {
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${dir}:${process.env.PATH}`,
          GH_TOKEN: "test-token",
          GH_REPO: "thedavidweng/OpenKara",
          EVENT_NAME: "schedule",
          REF_NAME: "main",
          WORKFLOW_NAME: "Nightly hardening",
          RUN_ID: "4001",
          RUN_URL: "https://github.com/thedavidweng/OpenKara/actions/runs/4001",
          HEAD_SHA: "0123456789abcdef0123456789abcdef01234567",
          WINDOWS_MATRIX: "success",
          SEPARATION_SMOKE: "failure",
          WINDOWS_INSTALLED_APP: "success",
          MACOS_INSTALLED_APP: "success",
          LINUX_INSTALLED_APP: "success",
          AGGREGATE_REPORTS: "failure",
        },
      });
      expect(result.status).toBe(0);
      const calls = readFileSync(callLog, "utf8");
      expect(calls).toContain("issue create --repo thedavidweng/OpenKara");
      const body = readFileSync(bodyLog, "utf8");
      expect(body).toContain(
        "https://github.com/thedavidweng/OpenKara/actions/runs/4001",
      );
      expect(body).toContain(
        "[separation-smoke](https://github.com/thedavidweng/OpenKara/actions/runs/4001)",
      );
    });
  });
});
