import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));

function readProjectFile(path: string) {
  return readFileSync(join(projectRoot, path), "utf8");
}

describe("nightly installers workflow", () => {
  const workflow = readProjectFile(".github/workflows/nightly-installers.yml");

  test("runs on a daily schedule with a force-capable manual dispatch", () => {
    expect(workflow).toContain("schedule:");
    // Any daily time is fine (the exact hour is an operational choice, not a
    // contract); what must hold is that the schedule stays daily.
    expect(workflow).toMatch(/cron: "\d+ \d+ \* \* \*"/);
    expect(workflow).toContain("workflow_dispatch:");
    expect(workflow).toContain("force:");
    expect(workflow).toContain("type: boolean");
    expect(workflow).toContain('if [ "${FORCE}" = "true" ]; then');
    // Scheduled runs skip when main has not moved since the last nightly.
    expect(workflow).toContain('gh api "repos/${GH_REPO}/commits/nightly"');
    expect(workflow).toContain("should_build=false");
  });

  test("reuses the installed-app build workflows instead of a bespoke build", () => {
    expect(workflow).toContain(
      "uses: ./.github/workflows/reusable-windows-installed-app.yml",
    );
    expect(workflow).toContain(
      "uses: ./.github/workflows/reusable-macos-installed-app-smoke.yml",
    );
    expect(workflow).toContain(
      "uses: ./.github/workflows/reusable-linux-installed-app-smoke.yml",
    );
    // Nightly never runs the release build path: release_build stays off, no
    // signing secrets, and therefore no tauri.release.conf.json overlay.
    expect(workflow).not.toMatch(/release_build:/);
    expect(workflow).not.toContain("TAURI_SIGNING");
    expect(workflow).not.toContain("updater_target");
    // Clean-install only; the upgrade leg needs a published previous version.
    expect(workflow).toMatch(/previous_version: none/);
  });

  test("publishes a rolling prerelease isolated from the updater endpoint", () => {
    // GitHub resolves releases/latest (the production updater endpoint) only
    // to non-prerelease releases, so the prerelease flag is load-bearing.
    expect(workflow).toContain("--prerelease");
    expect(workflow).toContain("gh release create nightly");
    expect(workflow).toContain("gh release delete nightly");
    expect(workflow).toContain("--cleanup-tag");
    // Updater assets must never ship on the nightly release.
    expect(workflow).toContain("latest.json");
    expect(workflow).toMatch(/-name '\*\.sig'/);
    expect(workflow).toContain('endswith(".sig")');
    // The publish gate exists for the Windows installer specifically.
    expect(workflow).toContain("OpenKara_nightly_x64-setup.exe");
    expect(workflow).toContain("needs.windows.result == 'success'");
  });

  test("scopes write permissions to the publish job", () => {
    expect(workflow).toMatch(/^permissions:\n  contents: read$/m);
    const writeGrants = workflow.match(/contents: write/g) ?? [];
    expect(writeGrants).toHaveLength(1);
  });
});
