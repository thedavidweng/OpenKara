// The Linux Rust test job runs the unit tests of every module that
// src-tauri/src/lib.rs compiles only with the automation-smoke feature. The
// default-feature test run never builds those modules, so a failing test in
// one of them would otherwise stay hidden.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const workflow = readFileSync(
  join(projectRoot, ".github/workflows/ci.yml"),
  "utf8",
);
const libRs = readFileSync(join(projectRoot, "src-tauri/src/lib.rs"), "utf8");

const STEP_NAME = "Run automation-smoke module tests";

function jobSection(id: string): string {
  const start = workflow.search(new RegExp(`^ {2}${id}:[ \\t]*$`, "m"));
  expect(start, `job "${id}" exists`).toBeGreaterThan(-1);
  const rest = workflow.slice(start + 1);
  const next = rest.search(/\n {2}[A-Za-z0-9_-]+:[ \t]*\n/);
  return next === -1 ? rest : rest.slice(0, next);
}

function stepRun(job: string, name: string): string {
  const start = job.indexOf(`- name: ${name}\n`);
  expect(start, `step "${name}" exists`).toBeGreaterThan(-1);
  const rest = job.slice(start + 1);
  const next = rest.search(/\n\s*- name: /);
  const step = next === -1 ? rest : rest.slice(0, next);
  const match = step.match(/^\s*run: (.+)$/m);
  expect(match, `step "${name}" has a one-line run command`).not.toBeNull();
  return match![1].trim();
}

function automationSmokeModules(): string[] {
  const gated =
    /#\[cfg\(feature = "automation-smoke"\)\]\s*\n\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+);/g;
  return [...libRs.matchAll(gated)].map((match) => match[1]);
}

function nextestFilter(command: string): RegExp {
  const expression = command.match(/-E '([^']+)'/);
  expect(expression, "the command has a nextest filter").not.toBeNull();
  const pattern = expression![1].match(/^test\(\/(.+)\/\)$/);
  expect(pattern, "the filter is a single test-name regex").not.toBeNull();
  return new RegExp(pattern![1]);
}

describe("automation-smoke module tests in CI", () => {
  const command = stepRun(jobSection("rust-test"), STEP_NAME);

  test("lib.rs gates modules behind automation-smoke", () => {
    expect(automationSmokeModules()).toContain("runtime_bootstrap_regression");
  });

  test("the Linux Rust job builds the lib tests with the feature", () => {
    expect(command).toMatch(/^cargo nextest run /);
    expect(command).toContain(" --lib ");
    expect(command).toContain(" --features automation-smoke ");
  });

  test("the filter selects every gated module", () => {
    const filter = nextestFilter(command);
    for (const module of automationSmokeModules()) {
      expect(filter.test(`${module}::tests::example`), module).toBe(true);
    }
  });

  test("the filter leaves default-feature modules to the main test run", () => {
    const filter = nextestFilter(command);
    for (const module of [
      "remote",
      "separator",
      "commands::runtime_bootstrap",
    ]) {
      expect(filter.test(`${module}::tests::example`), module).toBe(false);
    }
  });
});
