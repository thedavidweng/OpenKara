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

function stepsNamed(workflow: string, stepName: string) {
  const lines = workflow.split(/\r?\n/);
  const steps: string[] = [];

  for (let index = 0; index < lines.length; index += 1) {
    if (lines[index].trim() !== `- name: ${stepName}`) {
      continue;
    }

    const indentation = lines[index].search(/\S/);
    const stepLines = [lines[index]];
    for (let next = index + 1; next < lines.length; next += 1) {
      const nextLine = lines[next];
      if (nextLine.trim().length > 0 && nextLine.search(/\S/) <= indentation) {
        break;
      }
      stepLines.push(nextLine);
    }
    steps.push(stepLines.join("\n"));
  }

  return steps;
}

describe("workflow security", () => {
  test("does not persist checkout credentials in workflow worktrees", () => {
    // dependabot-sync.yml needs persist-credentials: true to push commits back to PR branches
    const allowlist = new Set(["dependabot-sync.yml"]);

    for (const filename of workflowFiles()) {
      if (allowlist.has(filename)) continue;
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

  test("uses bash for matrix ONNX Runtime environment expansion", () => {
    const ciWorkflow = readFileSync(join(workflowDir, "ci.yml"), "utf8");
    const matrixPrepareSteps = stepsNamed(
      ciWorkflow,
      "Prepare ONNX Runtime",
    ).filter((step) => step.includes("ORT_TARGET: ${{ matrix.ort_target }}"));

    expect(matrixPrepareSteps).toHaveLength(2);

    for (const step of matrixPrepareSteps) {
      expect(step).toContain("shell: bash");
      expect(step).toContain(
        'node scripts/prepare-onnx-runtime.mjs --target "${ORT_TARGET}"',
      );
    }
  });
});
