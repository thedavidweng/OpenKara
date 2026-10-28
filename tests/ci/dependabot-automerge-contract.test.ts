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
import { describe, expect, test } from "vite-plus/test";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const workflowPath = join(
  projectRoot,
  ".github/workflows/dependabot-automerge.yml",
);

const workflow = readFileSync(workflowPath, "utf8");

describe("dependabot automerge contract", () => {
  test.each([
    ["actions/checkout", "version-update:semver-patch", "", "", true],
    ["actions/checkout", "version-update:semver-minor", "", "", true],
    ["actions/checkout", "version-update:semver-major", "", "OPEN", false],
    ["actions/checkout", "unknown", "", "OPEN", false],
    ["actions/checkout", "unknown", "", "FIXED", false],
    // #568: Dependabot reports an empty update-type for SHA-pinned Actions
    // bumps, which is a same-major pin refresh, so it now auto-merges.
    ["actions/checkout", "", "", "OPEN", true],
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
    // #562 narrowed the denylist to the release driver and this gate:
    // patch updates and github-actions minor bumps now auto-merge, so the
    // previous expectations for cla-action and jactionlint are obsolete.
    [
      "contributor-license/cla-action",
      "version-update:semver-patch",
      "",
      "OPEN",
      true,
    ],
    [
      "actions/checkout, jdx/jactionlint",
      "version-update:semver-minor",
      "",
      "",
      true,
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
});
