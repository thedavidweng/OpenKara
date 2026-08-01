import { existsSync, readFileSync } from "node:fs";

const VALID_STATUSES = new Set(["passed", "failed", "skipped"]);
const VALID_ASSERTION_RESULTS = new Set(["pass", "fail", "skip"]);

const reportPath = process.argv[2];
const expectedScenario = process.argv[3];

function fail(message) {
  console.error(`desktop-e2e-report validation failed: ${message}`);
  process.exit(1);
}

function assertString(value, label) {
  if (typeof value !== "string" || value.length === 0) {
    fail(`${label} must be a non-empty string`);
  }
}

if (!reportPath || !expectedScenario) {
  console.error(
    "usage: node scripts/validate-desktop-e2e-report.mjs <report-path> <expected-scenario>",
  );
  process.exit(1);
}

if (!existsSync(reportPath)) {
  fail(`report not found at ${reportPath}`);
}

let report;
try {
  report = JSON.parse(readFileSync(reportPath, "utf8"));
} catch (error) {
  fail(`report at ${reportPath} is not valid JSON: ${error.message}`);
}

if (typeof report !== "object" || report === null) {
  fail("report must be an object");
}

assertString(report.scenario, "report.scenario");
if (report.scenario !== expectedScenario) {
  fail(`expected scenario "${expectedScenario}", found "${report.scenario}"`);
}

assertString(report.status, "report.status");
if (!VALID_STATUSES.has(report.status)) {
  fail(
    `report.status must be one of ${[...VALID_STATUSES].join(", ")}; found "${report.status}"`,
  );
}

if (!Array.isArray(report.assertions) || report.assertions.length === 0) {
  fail("report.assertions must be a non-empty array");
}

let failCount = 0;
for (const [index, assertion] of report.assertions.entries()) {
  if (typeof assertion !== "object" || assertion === null) {
    fail(`assertions[${index}] must be an object`);
  }
  assertString(assertion.id, `assertions[${index}].id`);
  assertString(assertion.expected, `assertions[${index}].expected`);
  assertString(assertion.observed, `assertions[${index}].observed`);
  assertString(assertion.result, `assertions[${index}].result`);
  if (!VALID_ASSERTION_RESULTS.has(assertion.result)) {
    fail(
      `assertions[${index}].result must be one of ${[
        ...VALID_ASSERTION_RESULTS,
      ].join(", ")}; found "${assertion.result}"`,
    );
  }
  if (assertion.result === "fail") {
    failCount += 1;
  }
}

if (failCount > 0) {
  fail(`${failCount} assertion(s) failed`);
}

if (report.status !== "passed") {
  fail(`report.status is "${report.status}"; expected "passed"`);
}

console.log(
  `desktop-e2e-report valid: scenario="${report.scenario}" status="${report.status}" assertions=${report.assertions.length} failed=${failCount}`,
);
