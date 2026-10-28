import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));

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

describe("Playwright flaky report", () => {
  test("parses a Playwright JSON report without jq", () => {
    const ciYaml = readFileSync(
      join(projectRoot, ".github/workflows/ci.yml"),
      "utf8",
    );
    const script = extractStepScript(
      extractJobSection(ciYaml, "playwright-ui-smoke"),
      "Report flaky tests",
    );
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
    // node -p colorizes numbers when FORCE_COLOR is set, which this shell
    // and the pre-push hook inherit. The report text is what the step writes.
    const env = { ...process.env, GITHUB_STEP_SUMMARY: summary };
    delete env.FORCE_COLOR;
    delete env.NO_COLOR;
    const result = spawnSync("bash", ["-e", "-c", script], {
      cwd: dir,
      env,
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
