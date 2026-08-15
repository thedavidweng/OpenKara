import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const lefthook = readFileSync(join(projectRoot, "lefthook.yml"), "utf8");

function topLevelHookBlock(yaml: string, hook: string): string {
  const lines = yaml.split(/\r?\n/);
  const start = lines.findIndex((line) => line === `${hook}:`);
  if (start < 0) {
    throw new Error(`lefthook.yml is missing the ${hook} hook`);
  }
  const body: string[] = [];
  for (const line of lines.slice(start + 1)) {
    if (/^[A-Za-z]/.test(line)) {
      break;
    }
    body.push(line);
  }
  return body.join("\n");
}

function hookCommandBlock(hookBody: string, command: string): string | null {
  const lines = hookBody.split(/\r?\n/);
  const start = lines.findIndex((line) => line === `    ${command}:`);
  if (start < 0) {
    return null;
  }
  const body: string[] = [];
  for (const line of lines.slice(start + 1)) {
    if (/^    [A-Za-z0-9_-]+:\s*$/.test(line)) {
      break;
    }
    body.push(line);
  }
  return body.join("\n");
}

function hookRunsKnip(yaml: string, hook: string): boolean {
  const command = hookCommandBlock(topLevelHookBlock(yaml, hook), "knip");
  return command != null && /run:.*pnpm knip --no-progress/.test(command);
}

describe("lefthook gates", () => {
  test("pre-commit and pre-push both run knip", () => {
    expect(hookRunsKnip(lefthook, "pre-commit")).toBe(true);
    expect(hookRunsKnip(lefthook, "pre-push")).toBe(true);
  });

  test("knip assertions stay inside their own hook", () => {
    const onlyPushRunsKnip = [
      "pre-commit:",
      "  commands:",
      "    oxfmt:",
      "      run: pnpm exec oxfmt",
      "pre-push:",
      "  commands:",
      "    knip:",
      "      run: pnpm knip --no-progress",
    ].join("\n");
    expect(hookRunsKnip(onlyPushRunsKnip, "pre-commit")).toBe(false);
    expect(hookRunsKnip(onlyPushRunsKnip, "pre-push")).toBe(true);
  });
});
