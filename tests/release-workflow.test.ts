import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));

function readProjectFile(path: string) {
  return readFileSync(join(projectRoot, path), "utf8");
}

describe("release workflow", () => {
  test("gates release publishing on the real separation smoke without adding it to per-commit CI", () => {
    const releaseWorkflow = readProjectFile(".github/workflows/release.yml");
    const ciWorkflow = readProjectFile(".github/workflows/ci.yml");

    expect(releaseWorkflow).toContain("release-separation-smoke:");
    expect(releaseWorkflow).toContain("name: Release separation smoke");
    expect(releaseWorkflow).toContain("./scripts/setup.sh");
    expect(releaseWorkflow).toContain("./scripts/run-local-smoke.sh");
    expect(releaseWorkflow).toContain(
      "src-tauri/tests/fixtures/audio/fixture.wav",
    );
    expect(releaseWorkflow).toContain("separation_passed");
    expect(releaseWorkflow).toContain("separation_failed");
    expect(releaseWorkflow).toContain("separation_skipped");
    expect(releaseWorkflow).toMatch(
      /publish:\n(?: {2}.+\n)* {4}needs:.*release-separation-smoke/,
    );

    expect(ciWorkflow).not.toContain("Release separation smoke");
    expect(ciWorkflow).not.toContain("./scripts/run-local-smoke.sh");
  });

  test("fails fast when the release tag and package.json version disagree", () => {
    // The bundle version comes from package.json (scripts/sync-version.mjs),
    // while the tag drives asset naming and the winget/flatpak manifests. If
    // they drift, the release ships misnamed assets and the distribution
    // manifests point at assets that do not exist. This gate must stay in
    // prepare-release so it fails before any build or publish job runs.
    const releaseWorkflow = readProjectFile(".github/workflows/release.yml");

    expect(releaseWorkflow).toContain(
      "name: Verify package.json version matches release tag",
    );
    expect(releaseWorkflow).toContain(
      `node -p "require('./package.json').version"`,
    );
    expect(releaseWorkflow).toContain("pnpm version:sync");
    expect(releaseWorkflow).toMatch(
      /prepare-release:[\s\S]*Verify package\.json version matches release tag[\s\S]*publish:/,
    );
  });
});
