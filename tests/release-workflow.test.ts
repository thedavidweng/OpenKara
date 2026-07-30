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
    const separationWorkflow = readProjectFile(
      ".github/workflows/reusable-separation-smoke.yml",
    );

    expect(releaseWorkflow).toContain("release-separation-smoke:");
    expect(releaseWorkflow).toContain("name: Release separation smoke");
    expect(releaseWorkflow).toContain(
      "uses: ./.github/workflows/reusable-separation-smoke.yml",
    );
    expect(releaseWorkflow).toContain("release-windows-installed-smoke:");
    expect(releaseWorkflow).toContain("Release Windows installed-app smoke");
    expect(releaseWorkflow).toContain(
      "uses: ./.github/workflows/reusable-windows-installed-app.yml",
    );
    expect(releaseWorkflow).toContain("release-macos-installed-smoke:");
    expect(releaseWorkflow).toContain(
      "uses: ./.github/workflows/reusable-macos-installed-app-smoke.yml",
    );
    expect(releaseWorkflow).toContain("release-linux-installed-smoke:");
    expect(releaseWorkflow).toContain(
      "uses: ./.github/workflows/reusable-linux-installed-app-smoke.yml",
    );

    expect(separationWorkflow).toContain("./scripts/setup.sh");
    expect(separationWorkflow).toContain("./scripts/run-local-smoke.sh");
    expect(separationWorkflow).toContain(
      "src-tauri/tests/fixtures/audio/fixture.wav",
    );
    expect(separationWorkflow).toContain("separation_passed");
    expect(separationWorkflow).toContain("separation_failed");
    expect(separationWorkflow).toContain("separation_skipped");

    const publishSection = releaseWorkflow.match(
      /  publish:[\s\S]*?\n  publish-windows:/,
    )?.[0];
    expect(publishSection).toBeDefined();

    expect(releaseWorkflow).toContain("publish-windows:");
    expect(releaseWorkflow).toMatch(
      /publish-windows:[\s\S]*?needs:[\s\S]*?publish[\s\S]*?release-windows-installed-smoke/,
    );
    expect(releaseWorkflow).toContain(
      "release-windows-installed-app-installer",
    );
    expect(releaseWorkflow).toContain("release-windows-installed-app-updater");
    expect(releaseWorkflow).toContain("merge-latest-json.mjs");
    expect(releaseWorkflow).toContain("gh release upload");
    expect(releaseWorkflow).toContain("--clobber");

    expect(releaseWorkflow).not.toMatch(
      /name: Windows\n\s+os: windows-latest\n\s+ort_target: x86_64-pc-windows-msvc/,
    );

    expect(ciWorkflow).not.toContain("Release separation smoke");
    expect(ciWorkflow).not.toContain("Release Windows installed-app smoke");
    expect(ciWorkflow).not.toContain("./scripts/run-local-smoke.sh");
  });

  test("reuses the Windows installed-app smoke build in release publishing", () => {
    const reusableWorkflow = readProjectFile(
      ".github/workflows/reusable-windows-installed-app.yml",
    );

    expect(reusableWorkflow).toContain("openkara_automation_driver.exe");
    expect(reusableWorkflow).toContain("validate-installed-app-smoke.mjs");
    expect(reusableWorkflow).toContain(
      "Install previous stable then upgrade to candidate",
    );
    expect(reusableWorkflow).toContain("release_build:");
    expect(reusableWorkflow).toContain("TAURI_SIGNING_PRIVATE_KEY");
    expect(reusableWorkflow).toContain("src-tauri/tauri.release.conf.json");
    expect(reusableWorkflow).toContain("${{ inputs.artifact_prefix }}-updater");
    expect(reusableWorkflow).toContain("latest.json");
    expect(reusableWorkflow).toContain("*.sig");
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
