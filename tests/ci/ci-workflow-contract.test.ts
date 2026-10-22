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

import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { runInNewContext } from "node:vm";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
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
    expect(ciYaml).toContain("run_tauri-build-smoke");
    expect(ciYaml).toContain("run_tauri-build");
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
    ["pull_request", true],
    ["pull_request", false],
    ["push", true],
    ["push", false],
    ["workflow_dispatch", false],
  ])(
    "%s packaging changes=%s has matching build and gate requirements",
    (event, changed) => {
      const condition = packagingYaml.match(
        /  build-flatpak:[\s\S]*?    if: \|\n([\s\S]*?)    runs-on:/,
      )?.[1];
      expect(condition).toBeDefined();
      const gate = packagingYaml
        .split("  packaging-gate:")[1]
        .split("        run: |\n")[1];
      expect(gate).toBeDefined();
      const required = changed || event === "workflow_dispatch";
      for (const validation of ["success", "failure"]) {
        const expression = condition!
          .replace(/!cancelled\(\)/g, "true")
          .replace(
            /needs\.validate-flatpak\.result/g,
            JSON.stringify(validation),
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
