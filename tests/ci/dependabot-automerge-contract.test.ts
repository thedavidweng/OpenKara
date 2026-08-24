import { readFileSync } from "node:fs";
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

  test("pins fetch-metadata by commit SHA with version comment", () => {
    expect(workflow).toMatch(
      /uses: dependabot\/fetch-metadata@[0-9a-f]{40} # v\d+\.\d+\.\d+/,
    );
  });

  test("auto-merge is limited to semver patch and minor updates", () => {
    expect(workflow).toContain('"version-update:semver-patch"');
    expect(workflow).toContain('"version-update:semver-minor"');
    // Majors require manual migration review; unknown update types must stay
    // ineligible, so no other acceptance string may exist.
    expect(workflow.match(/version-update:semver-\w+/g)).toEqual([
      "version-update:semver-patch",
      "version-update:semver-minor",
    ]);
  });

  test("denylists native audio, model, and windowing crates", () => {
    const denylist = workflow.match(/DENYLIST:\s*"([^"]+)"/);
    expect(denylist).not.toBeNull();
    for (const crate of [
      "ort",
      "tauri",
      "wry",
      "tao",
      "cpal",
      "rubato",
      "audioadapter",
    ]) {
      expect(denylist?.[1]).toContain(crate);
    }
  });

  test("merges via squash auto-merge behind required checks", () => {
    expect(workflow).toContain("--auto");
    expect(workflow).toContain("--squash");
  });

  test("keeps top-level permissions read-only", () => {
    expect(workflow).toMatch(/permissions:\n  contents: read/);
  });
});
