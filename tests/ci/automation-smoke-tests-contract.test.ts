// The Linux Rust test job runs the unit tests of every module that
// src-tauri/src/lib.rs compiles only with the automation-smoke feature. The
// default-feature test run never builds those modules, so a failing test in
// one of them would otherwise stay hidden.

import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const workflow = readFileSync(
  join(projectRoot, ".github/workflows/ci.yml"),
  "utf8",
);
const crateSource = join(projectRoot, "src-tauri/src");
const crateRoot = join(crateSource, "lib.rs");

const FEATURE = '"automation-smoke"';
const STEP_NAME = "Run automation-smoke module tests";
const ATTRIBUTE_RUN = String.raw`((?:#\[[^\]]*\]\s*)+)`;
const MOD_ITEM = new RegExp(
  ATTRIBUTE_RUN + String.raw`(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*[;{]`,
  "g",
);
const FN_ITEM = new RegExp(
  ATTRIBUTE_RUN +
    String.raw`(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+(\w+)`,
  "g",
);
const INNER_CFG = /#!\[cfg\s*\([^\]]*\)\s*\]/g;

function jobSection(id: string): string {
  const start = workflow.search(new RegExp(`^ {2}${id}:[ \\t]*$`, "m"));
  expect(start, `job "${id}" exists`).toBeGreaterThan(-1);
  const rest = workflow.slice(start + 1);
  const next = rest.search(/\n {2}[A-Za-z0-9_-]+:[ \t]*\n/);
  return next === -1 ? rest : rest.slice(0, next);
}

function stepSection(job: string, name: string): string {
  const start = job.indexOf(`- name: ${name}\n`);
  expect(start, `step "${name}" exists`).toBeGreaterThan(-1);
  const rest = job.slice(start + 1);
  const next = rest.search(/\n\s*- name: /);
  return next === -1 ? rest : rest.slice(0, next);
}

function stepRun(step: string): string {
  const match = step.match(/^\s*run: (.+)$/m);
  expect(
    match,
    `step "${STEP_NAME}" has a one-line run command`,
  ).not.toBeNull();
  return match![1].trim();
}

/** Blank out comments so doc comments between attributes do not hide a gate. */
function withoutComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, " ").replace(/\/\/[^\n]*/g, "");
}

/** Remove every `not(...)` group, so only positive cfg predicates remain. */
function withoutNegations(predicate: string): string {
  let kept = "";
  let index = 0;
  while (index < predicate.length) {
    const negation = /^not\s*\(/.exec(predicate.slice(index));
    if (negation && !/\w/.test(predicate[index - 1] ?? "")) {
      let depth = 0;
      let end = index + negation[0].length - 1;
      for (; end < predicate.length; end += 1) {
        if (predicate[end] === "(") depth += 1;
        if (predicate[end] === ")" && --depth === 0) break;
      }
      index = end + 1;
      continue;
    }
    kept += predicate[index];
    index += 1;
  }
  return kept;
}

/** True when an attribute compiles its item only with the feature enabled. */
function requiresFeature(attribute: string): boolean {
  const cfg = attribute.match(/^#!?\[cfg\s*\(([\s\S]*)\)\s*\]$/);
  return cfg !== null && withoutNegations(cfg[1]).includes(FEATURE);
}

function attributes(run: string): string[] {
  return run.match(/#\[[^\]]*\]/g) ?? [];
}

function gatedModules(source: string): string[] {
  return [...withoutComments(source).matchAll(MOD_ITEM)]
    .filter((match) => attributes(match[1]).some(requiresFeature))
    .map((match) => match[2]);
}

function gatedTestFunctions(source: string): string[] {
  return [...withoutComments(source).matchAll(FN_ITEM)]
    .filter((match) => {
      const list = attributes(match[1]);
      return (
        list.some(requiresFeature) &&
        list.some((attribute) => /^#\[(?:\w+::)*test\b/.test(attribute))
      );
    })
    .map((match) => match[2]);
}

function fileGates(source: string): boolean {
  return (withoutComments(source).match(INNER_CFG) ?? []).some(requiresFeature);
}

function rustFilesBelowCrateRoot(): string[] {
  return readdirSync(crateSource, { recursive: true, encoding: "utf8" })
    .filter((path) => path.endsWith(".rs"))
    .map((path) => join(crateSource, path))
    .filter((path) => path !== crateRoot)
    .sort();
}

function nextestFilter(command: string): RegExp {
  const expression = command.match(/-E '([^']+)'/);
  expect(expression, "the command has a nextest filter").not.toBeNull();
  const pattern = expression![1].match(/^test\(\/(.+)\/\)$/);
  expect(pattern, "the filter is a single test-name regex").not.toBeNull();
  return new RegExp(pattern![1]);
}

describe("automation-smoke module tests in CI", () => {
  const job = jobSection("rust-test");
  const step = stepSection(job, STEP_NAME);
  const command = stepRun(step);
  const gatedAtCrateRoot = gatedModules(readFileSync(crateRoot, "utf8"));

  test("lib.rs gates modules behind automation-smoke", () => {
    expect(gatedAtCrateRoot).toContain("runtime_bootstrap_regression");
  });

  test("the module scan reads every gate form and ignores negated gates", () => {
    const source = [
      '#[cfg(feature = "automation-smoke")] pub mod plain;',
      '#[cfg(all(feature = "automation-smoke", unix))] mod combined;',
      '#[cfg(all(feature = "automation-smoke", not(windows)))] mod partly_negated;',
      '#[cfg(feature = "automation-smoke")]\n#[path = "x.rs"]\npub(crate) mod with_path;',
      '#[cfg(feature = "automation-smoke")]\n/// Docs.\npub(in crate::x) mod documented;',
      '#[cfg(feature = "automation-smoke")] pub mod inline { fn f() {} }',
      '#[cfg(all(test, feature = "automation-smoke"))] mod tests;',
      '#[cfg(not(feature = "automation-smoke"))] mod negated;',
      '#[cfg(not(all(unix, feature = "automation-smoke")))] mod negated_all;',
      '#[cfg_attr(feature = "automation-smoke", path = "y.rs")] mod attr_only;',
      '// #[cfg(feature = "automation-smoke")] mod commented;',
      "#[cfg(test)] mod ungated;",
    ].join("\n");
    expect(gatedModules(source)).toEqual([
      "plain",
      "combined",
      "partly_negated",
      "with_path",
      "documented",
      "inline",
      "tests",
    ]);
  });

  test("the Linux Rust job builds the lib tests with the feature", () => {
    expect(command).toMatch(/^cargo nextest run /);
    expect(command).toContain(" --lib ");
    expect(command).toContain(" --features automation-smoke ");
  });

  test("the step always runs and its failure fails the job", () => {
    expect(step).not.toMatch(/^\s*if:/m);
    expect(step).not.toMatch(/^\s*continue-on-error:/m);
    expect(job).not.toMatch(/^ {4}continue-on-error:/m);
  });

  test("the filter selects every module gated in lib.rs", () => {
    const filter = nextestFilter(command);
    for (const module of gatedAtCrateRoot) {
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

  test("no module or test is gated below the crate root", () => {
    const findings = rustFilesBelowCrateRoot().flatMap((path) => {
      const source = readFileSync(path, "utf8");
      const file = relative(projectRoot, path);
      return [
        ...gatedModules(source).map((name) => `${file}: mod ${name}`),
        ...gatedTestFunctions(source).map((name) => `${file}: test fn ${name}`),
        ...(fileGates(source) ? [`${file}: #![cfg] on the whole file`] : []),
      ];
    });
    expect(
      findings,
      "The CI filter in the step " +
        `"${STEP_NAME}" anchors on modules at the crate root, so an item gated ` +
        "behind automation-smoke below src-tauri/src/lib.rs never runs in CI. " +
        "Give it its own entry in that filter, then update this test to " +
        "expect it.",
    ).toEqual([]);
  });
});
