import { execFileSync } from "node:child_process";
import {
  chmodSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const workflowPath = join(
  projectRoot,
  ".github/workflows/dependabot-automerge.yml",
);

const workflow = readFileSync(workflowPath, "utf8");

describe("dependabot automerge contract", () => {
  test("only touches pull requests authored by dependabot", () => {
    expect(workflow).toContain(
      "github.event.pull_request.user.login == 'dependabot[bot]'",
    );
  });

  test("only runs against same-repo PR heads (not forks)", () => {
    expect(workflow).toContain(
      "github.event.pull_request.head.repo.full_name == github.repository",
    );
  });

  test("never checks out or executes pull request code", () => {
    expect(workflow).not.toContain("actions/checkout");
  });

  test("pins fetch-metadata by commit SHA with version comment", () => {
    expect(workflow).toMatch(
      /uses: dependabot\/fetch-metadata@[0-9a-f]{40} # v\d+\.\d+\.\d+/,
    );
  });

  test.each([
    ["actions/checkout", "version-update:semver-patch", "", "", true],
    ["actions/checkout", "version-update:semver-minor", "", "", true],
    ["actions/checkout", "version-update:semver-major", "", "OPEN", false],
    ["actions/checkout", "unknown", "", "OPEN", false],
    ["actions/checkout", "unknown", "", "FIXED", false],
    ["actions/checkout", "", "", "OPEN", false],
    [
      "example/pkg",
      "version-update:semver-minor",
      "direct:production",
      "",
      false,
    ],
    [
      "example/pkg",
      "version-update:semver-minor",
      "direct:development",
      "",
      true,
    ],
    [
      "example/pkg",
      "version-update:semver-minor",
      "direct:production",
      "OPEN",
      true,
    ],
    [
      "contributor-license/cla-action",
      "version-update:semver-patch",
      "",
      "OPEN",
      false,
    ],
    [
      "actions/checkout, jdx/jactionlint",
      "version-update:semver-minor",
      "",
      "",
      false,
    ],
  ])(
    "classifies %s %s %s %s as merge=%s",
    (names, update, kind, alert, expected) => {
      const directory = mkdtempSync(join(tmpdir(), "openkara-automerge-"));
      const gh = join(directory, "gh");
      writeFileSync(gh, '#!/bin/sh\nprintf "%s\\n" AUTO_MERGE_ENABLED\n');
      chmodSync(gh, 0o755);
      const script = workflow
        .split("        run: |", 2)[1]
        .split("\n")
        .map((line) => line.replace(/^ {10}/, ""))
        .join("\n");
      try {
        const output = execFileSync("bash", ["-c", script], {
          encoding: "utf8",
          env: {
            ...process.env,
            PATH: `${directory}:${process.env.PATH}`,
            GH_TOKEN: "test-only",
            PR_URL: "https://github.com/example/repo/pull/1",
            DEP_NAMES: names,
            UPDATE_TYPE: update,
            DEP_TYPE: kind,
            ALERT_STATE: alert,
            PKG_ECOSYSTEM:
              names === "example/pkg" ? "npm_and_yarn" : "github-actions",
          },
        });
        expect(output.includes("AUTO_MERGE_ENABLED")).toBe(expected);
      } finally {
        rmSync(directory, { recursive: true });
      }
    },
  );

  test("denylists release and review tooling", () => {
    expect(workflow).toContain("googleapis/release-please-action");
    expect(workflow).toContain("dependabot/fetch-metadata");
    expect(workflow).toContain("skip auto-merge (denylist)");
  });

  test("merges via squash auto-merge behind required checks", () => {
    expect(workflow).toContain("--auto");
    expect(workflow).toContain("--squash");
  });

  test("keeps top-level permissions empty and scopes write to the job", () => {
    expect(workflow).toMatch(/^permissions: \{\}$/m);
    expect(workflow).toContain("contents: write");
    expect(workflow).toContain("pull-requests: write");
  });
});
