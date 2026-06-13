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

function runBlocks(workflow: string) {
  const lines = workflow.split(/\r?\n/);
  const blocks: string[] = [];

  for (let index = 0; index < lines.length; index += 1) {
    const runMatch = lines[index].match(/^(\s*)run:\s*(.*)$/);
    if (runMatch === null) {
      continue;
    }

    const indentation = runMatch[1].length;
    const blockLines = [lines[index]];
    for (let next = index + 1; next < lines.length; next += 1) {
      const nextLine = lines[next];
      if (
        nextLine.trim().length > 0 &&
        nextLine.search(/\S/) <= indentation &&
        !nextLine.trimStart().startsWith("#")
      ) {
        break;
      }
      blockLines.push(nextLine);
    }
    blocks.push(blockLines.join("\n"));
  }

  return blocks;
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

  test("passes GitHub expressions through env before shell execution", () => {
    for (const filename of workflowFiles()) {
      const workflow = readFileSync(join(workflowDir, filename), "utf8");

      for (const runBlock of runBlocks(workflow)) {
        expect(runBlock, filename).not.toContain("${{");
      }
    }
  });

  test("pins job containers by digest", () => {
    for (const filename of workflowFiles()) {
      const workflow = readFileSync(join(workflowDir, filename), "utf8");
      const imageLines = workflow
        .split(/\r?\n/)
        .filter((line) => line.trimStart().startsWith("image: "));

      for (const line of imageLines) {
        expect(line, filename).toContain("@sha256:");
      }
    }
  });
});
