import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));
const script = join(projectRoot, "scripts/ci/nightly-evidence.mjs");
const commit = "a".repeat(40);
const requiredJobs = [
  "windows-matrix",
  "separation-smoke",
  "windows-installed-app",
  "macos-installed-app",
  "linux-installed-app",
];

function run(args: string[]) {
  return spawnSync(process.execPath, [script, ...args], {
    cwd: projectRoot,
    encoding: "utf8",
  });
}

function successfulNeeds() {
  return Object.fromEntries(
    requiredJobs.map((job) => [job, { result: "success" }]),
  );
}

describe("Nightly evidence", () => {
  test("creates commit-bound evidence from every required successful job", () => {
    const root = mkdtempSync(join(tmpdir(), "openkara-nightly-evidence-"));
    const output = join(root, "nightly-evidence.json");
    const result = run([
      "create",
      "--commit",
      commit,
      "--run-id",
      "42",
      "--created-at",
      "2026-08-02T12:00:00.000Z",
      "--needs-json",
      JSON.stringify(successfulNeeds()),
      "--output",
      output,
    ]);

    expect(result.stderr).toBe("");
    expect(result.status).toBe(0);
    expect(JSON.parse(readFileSync(output, "utf8"))).toEqual({
      schema_version: 1,
      status: "passed",
      commit_sha: commit,
      workflow_run_id: 42,
      created_at: "2026-08-02T12:00:00.000Z",
      jobs: Object.fromEntries(requiredJobs.map((job) => [job, "passed"])),
    });
  });

  test("rejects missing and failed required jobs", () => {
    const root = mkdtempSync(join(tmpdir(), "openkara-nightly-evidence-"));
    const missing = successfulNeeds();
    delete missing["windows-matrix"];
    const missingResult = run([
      "create",
      "--commit",
      commit,
      "--run-id",
      "42",
      "--needs-json",
      JSON.stringify(missing),
      "--output",
      join(root, "missing.json"),
    ]);
    expect(missingResult.status).toBe(1);
    expect(missingResult.stderr).toContain(
      "missing required Nightly job windows-matrix",
    );

    const failed = successfulNeeds();
    failed["separation-smoke"] = { result: "failure" };
    const failedResult = run([
      "create",
      "--commit",
      commit,
      "--run-id",
      "42",
      "--needs-json",
      JSON.stringify(failed),
      "--output",
      join(root, "failed.json"),
    ]);
    expect(failedResult.status).toBe(1);
    expect(failedResult.stderr).toContain(
      "Nightly job separation-smoke did not pass",
    );
  });

  test("verification rejects a different commit and stale evidence", () => {
    const root = mkdtempSync(join(tmpdir(), "openkara-nightly-evidence-"));
    const evidencePath = join(root, "nightly-evidence.json");
    writeFileSync(
      evidencePath,
      JSON.stringify({
        schema_version: 1,
        status: "passed",
        commit_sha: commit,
        workflow_run_id: 42,
        created_at: "2026-08-02T12:00:00.000Z",
        jobs: Object.fromEntries(requiredJobs.map((job) => [job, "passed"])),
      }),
    );

    const wrongCommit = run([
      "verify",
      "--input",
      evidencePath,
      "--commit",
      "b".repeat(40),
      "--now",
      "2026-08-02T12:30:00.000Z",
      "--max-age-hours",
      "24",
    ]);
    expect(wrongCommit.status).toBe(1);
    expect(wrongCommit.stderr).toContain("Nightly evidence commit mismatch");

    const stale = run([
      "verify",
      "--input",
      evidencePath,
      "--commit",
      commit,
      "--now",
      "2026-08-03T13:00:00.000Z",
      "--max-age-hours",
      "24",
    ]);
    expect(stale.status).toBe(1);
    expect(stale.stderr).toContain("Nightly evidence is older than 24 hours");
  });
});
