import { readFileSync, writeFileSync } from "node:fs";

const REQUIRED_JOBS = [
  "windows-matrix",
  "separation-smoke",
  "windows-installed-app",
  "macos-installed-app",
  "linux-installed-app",
];

function parseArguments(values) {
  const [command, ...rest] = values;
  const options = {};
  for (let index = 0; index < rest.length; index += 2) {
    const key = rest[index];
    const value = rest[index + 1];
    if (!key?.startsWith("--") || value === undefined) {
      throw new Error(`invalid argument ${key ?? ""}`.trim());
    }
    options[key.slice(2)] = value;
  }
  return { command, options };
}

function requireOption(options, name) {
  const value = options[name];
  if (!value) {
    throw new Error(`missing --${name}`);
  }
  return value;
}

function requireCommit(value) {
  if (!/^[0-9a-f]{40}$/u.test(value)) {
    throw new Error("commit must be a 40-character lowercase SHA");
  }
  return value;
}

function requireDate(value, name) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    throw new Error(`${name} must be an ISO timestamp`);
  }
  return date;
}

function validateJobs(jobs) {
  for (const job of REQUIRED_JOBS) {
    if (!(job in jobs)) {
      throw new Error(`missing required Nightly job ${job}`);
    }
    if (jobs[job] !== "passed") {
      throw new Error(`Nightly job ${job} did not pass`);
    }
  }
}

function createEvidence(options) {
  const commit = requireCommit(requireOption(options, "commit"));
  const runId = Number(requireOption(options, "run-id"));
  if (!Number.isSafeInteger(runId) || runId <= 0) {
    throw new Error("run-id must be a positive integer");
  }
  const createdAt = options["created-at"] ?? new Date().toISOString();
  requireDate(createdAt, "created-at");
  const needs = JSON.parse(requireOption(options, "needs-json"));
  const jobs = {};

  for (const job of REQUIRED_JOBS) {
    if (!(job in needs)) {
      throw new Error(`missing required Nightly job ${job}`);
    }
    if (needs[job].result !== "success") {
      throw new Error(`Nightly job ${job} did not pass: ${needs[job].result}`);
    }
    jobs[job] = "passed";
  }

  const evidence = {
    schema_version: 1,
    status: "passed",
    commit_sha: commit,
    workflow_run_id: runId,
    created_at: createdAt,
    jobs,
  };
  writeFileSync(
    requireOption(options, "output"),
    `${JSON.stringify(evidence, null, 2)}\n`,
  );
}

function verifyEvidence(options) {
  const expectedCommit = requireCommit(requireOption(options, "commit"));
  const evidence = JSON.parse(
    readFileSync(requireOption(options, "input"), "utf8"),
  );
  if (evidence.schema_version !== 1 || evidence.status !== "passed") {
    throw new Error(
      "Nightly evidence is not a passed schema version 1 manifest",
    );
  }
  if (evidence.commit_sha !== expectedCommit) {
    throw new Error(
      `Nightly evidence commit mismatch: expected ${expectedCommit}, found ${evidence.commit_sha}`,
    );
  }
  validateJobs(evidence.jobs ?? {});

  const maxAgeHours = Number(requireOption(options, "max-age-hours"));
  if (!Number.isFinite(maxAgeHours) || maxAgeHours <= 0) {
    throw new Error("max-age-hours must be positive");
  }
  const createdAt = requireDate(evidence.created_at, "evidence created_at");
  const now = requireDate(options.now ?? new Date().toISOString(), "now");
  const ageMs = now.getTime() - createdAt.getTime();
  if (ageMs < 0) {
    throw new Error("Nightly evidence was created in the future");
  }
  if (ageMs > maxAgeHours * 60 * 60 * 1000) {
    throw new Error(`Nightly evidence is older than ${maxAgeHours} hours`);
  }
}

try {
  const { command, options } = parseArguments(process.argv.slice(2));
  if (command === "create") {
    createEvidence(options);
  } else if (command === "verify") {
    verifyEvidence(options);
  } else {
    throw new Error("command must be create or verify");
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
