import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));
const workflowDir = join(projectRoot, ".github/workflows");

function workflowFiles() {
  return readdirSync(workflowDir)
    .filter((name) => name.endsWith(".yml") || name.endsWith(".yaml"))
    .sort();
}

function checkoutSteps(workflow: string) {
  const lines = workflow.split(/\r?\n/);
  const steps: string[] = [];

  for (let index = 0; index < lines.length; index += 1) {
    if (!lines[index].includes("uses: actions/checkout@")) {
      continue;
    }

    const stepLines = [lines[index]];
    for (let next = index + 1; next < lines.length; next += 1) {
      if (/^\s*-\s+name:/.test(lines[next])) {
        break;
      }
      stepLines.push(lines[next]);
    }
    steps.push(stepLines.join("\n"));
  }

  return steps;
}

describe("workflow security", () => {
  test("does not persist checkout credentials in workflow worktrees", () => {
    for (const filename of workflowFiles()) {
      const workflow = readFileSync(join(workflowDir, filename), "utf8");

      for (const step of checkoutSteps(workflow)) {
        expect(step, filename).toContain("persist-credentials: false");
      }
    }
  });
});
