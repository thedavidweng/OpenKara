import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));

function readProjectFile(path: string) {
  return readFileSync(join(projectRoot, path), "utf8");
}

describe("release workflow", () => {
  test("gates release publishing on release-only native and installed-app smoke", () => {
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
    expect(releaseWorkflow).toContain("release-windows-installed-smoke:");
    expect(releaseWorkflow).toContain("Release Windows installed-app smoke");
    expect(releaseWorkflow).toContain("--automation-smoke prepare");
    expect(releaseWorkflow).toContain("--automation-smoke restart");
    expect(releaseWorkflow).toContain("validate-installed-app-smoke.mjs");
    expect(releaseWorkflow).toContain(
      "Install preceding stable release then release candidate",
    );
    const publishSection = releaseWorkflow.match(
      /  publish:[\s\S]*?\n  generate-checksums:/,
    )?.[0];
    expect(publishSection).toBeDefined();
    expect(publishSection).toContain("release-separation-smoke");
    expect(publishSection).toContain("release-windows-installed-smoke");
    expect(releaseWorkflow).not.toMatch(
      /name: Windows\n\s+os: windows-latest\n\s+ort_target: x86_64-pc-windows-msvc/,
    );

    expect(ciWorkflow).not.toContain("Release separation smoke");
    expect(ciWorkflow).not.toContain("Release Windows installed-app smoke");
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
