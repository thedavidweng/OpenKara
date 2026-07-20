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
//   5. Packaging build jobs use packaging-specific gates (tested in
//      packaging-workflow-contract.test.ts).
//   6. The Playwright config and workflow agree on report generation.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import { ALL_JOBS } from "../../scripts/ci/classify-changes.mjs";

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
});
