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

  test("auto-merge is limited to patch, non-production minor, and github-actions non-major", () => {
    expect(workflow).toContain('"version-update:semver-patch"');
    expect(workflow).toContain('"version-update:semver-minor"');
    expect(workflow).toContain('dt" != "direct:production"');
    expect(workflow).toContain('ut" != "version-update:semver-major"');
    expect(workflow).toContain("github-actions-non-major");
  });

  test("security updates are always eligible", () => {
    expect(workflow).toContain('alert" = "OPEN"');
    expect(workflow).toContain('alert" = "FIXED"');
    expect(workflow).toContain("security-update");
  });

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
