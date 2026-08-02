import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));

function readProjectFile(path: string) {
  return readFileSync(join(projectRoot, path), "utf8");
}

function readWorkflowStep(workflow: string, name: string) {
  const marker = `      - name: ${name}`;
  const start = workflow.indexOf(marker);
  if (start === -1) {
    return "";
  }

  const bodyStart = start + marker.length;
  const nextStep = workflow.indexOf("\n      - name:", bodyStart);
  const nextJobOffset = workflow.slice(bodyStart).search(/\n  [A-Za-z0-9_-]+:/);
  const nextJob = nextJobOffset === -1 ? -1 : bodyStart + nextJobOffset;
  const end =
    [nextStep, nextJob]
      .filter((index) => index >= 0)
      .sort((left, right) => left - right)[0] ?? workflow.length;

  return workflow.slice(start, end);
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
    expect(separationWorkflow).toContain("validate-local-audio-smoke");
    expect(separationWorkflow).not.toContain("node - <<'NODE'");

    const publishSection = releaseWorkflow.match(
      /  publish:[\s\S]*?\n  publish-windows:/,
    )?.[0];
    expect(publishSection).toBeDefined();
    // macOS/Linux publish must wait for Windows installed-app smoke (and the
    // other gates). A missing Windows need would let a failed Windows lifecycle
    // gate ship macOS/Linux assets.
    expect(publishSection).toMatch(/release-windows-installed-smoke/);
    expect(publishSection).toMatch(/release-separation-smoke/);
    expect(publishSection).toMatch(/release-macos-installed-smoke/);
    expect(publishSection).toMatch(/release-linux-installed-smoke/);

    expect(releaseWorkflow).toContain("publish-windows:");
    expect(releaseWorkflow).toMatch(
      /publish-windows:[\s\S]*?needs:[\s\S]*?publish[\s\S]*?release-windows-installed-smoke/,
    );
    expect(releaseWorkflow).toContain(
      "release-windows-installed-app-installer",
    );
    expect(releaseWorkflow).toContain("release-windows-installed-app-updater");
    expect(releaseWorkflow).toContain("generate-release-metadata:");
    expect(releaseWorkflow).toContain("openkara_release_evidence");
    expect(releaseWorkflow).toContain("verify-assets");
    expect(releaseWorkflow).toContain("--output latest.json");
    expect(releaseWorkflow).toContain("--output SHA256SUMS");
    expect(releaseWorkflow).toContain(
      'gh release download "${RELEASE_TAG}" --pattern "OpenKara*"',
    );
    expect(releaseWorkflow).toContain("gh release upload");
    expect(releaseWorkflow).toContain("--clobber");
    expect(releaseWorkflow).toContain("updater_target: windows-x86_64");
    expect(releaseWorkflow).not.toContain("merge-latest-json.mjs");
    expect(releaseWorkflow).not.toContain(
      "Synthesized Windows updater overlay",
    );

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
    const releaseWorkflow = readProjectFile(".github/workflows/release.yml");

    expect(reusableWorkflow).toContain("openkara_automation_driver.exe");
    expect(reusableWorkflow).toContain("validate-automation-report");
    expect(reusableWorkflow).toContain("validate-desktop-e2e");
    expect(reusableWorkflow).not.toContain("validate-installed-app-smoke.mjs");
    expect(reusableWorkflow).not.toContain("validate-desktop-e2e-report.mjs");
    expect(reusableWorkflow).toContain("clean-install:");
    expect(reusableWorkflow).toContain("upgrade-install:");
    expect(reusableWorkflow).toContain("fault-injection:");
    expect(reusableWorkflow).toContain("run_fault_injection");
    expect(reusableWorkflow).toContain("run_keyboard_uia");
    expect(reusableWorkflow).toContain("keyboard-workflow");
    expect(reusableWorkflow).toContain("--scenario fault-injection");
    expect(reusableWorkflow).toContain("--inject-faults");
    expect(reusableWorkflow).toContain("OPENKARA_APP_DATA_DIR");
    expect(reusableWorkflow).toContain("library-marker.txt");
    expect(reusableWorkflow).toContain("release_build:");
    expect(reusableWorkflow).toContain("TAURI_SIGNING_PRIVATE_KEY");
    expect(reusableWorkflow).toContain("src-tauri/tauri.release.conf.json");
    expect(reusableWorkflow).toContain("${{ inputs.artifact_prefix }}-updater");
    expect(reusableWorkflow).toContain("*setup.exe.sig");
    expect(releaseWorkflow).toContain('$_.Name -like "*setup.exe.sig"');

    // Build-path speed: non-release smoke must not re-key the rust cache per
    // commit, and must thin-LTO + opt-level=1 the release profile so driver+app
    // do not each pay a fat-LTO / size-opt link.
    expect(reusableWorkflow).not.toMatch(/key:\s*\$\{\{\s*github\.sha\s*\}\}/);
    expect(reusableWorkflow).toContain("CARGO_PROFILE_RELEASE_LTO");
    expect(reusableWorkflow).toContain("CARGO_PROFILE_RELEASE_OPT_LEVEL");
    expect(reusableWorkflow).toContain("thin");
    expect(reusableWorkflow).toContain("thin-lto-o1");
    expect(reusableWorkflow).toContain(
      "cargo build --release --features automation-driver --bin openkara_automation_driver",
    );
    expect(reusableWorkflow).toMatch(
      /tauri build --ci --bundles nsis --features automation-smoke/,
    );
    // Product tauri build must not enable automation-driver (packs the extra bin).
    expect(reusableWorkflow).not.toMatch(
      /tauri build[^\n]*--features automation-driver/,
    );
    expect(reusableWorkflow).toMatch(/cache:\s*pnpm/);
    // UIA probe builds in parallel with cargo, not as a serial follow-up step.
    expect(reusableWorkflow).toContain('Start-Process -FilePath "dotnet"');

    // Release call must enable keyboard UIA, upgrade (empty previous_version),
    // and fault injection, with long artifact retention.
    expect(releaseWorkflow).toMatch(
      /release-windows-installed-smoke:[\s\S]*?uia_scenario:\s*keyboard-workflow/,
    );
    expect(releaseWorkflow).toMatch(
      /release-windows-installed-smoke:[\s\S]*?run_fault_injection:\s*true/,
    );
    expect(releaseWorkflow).toMatch(
      /release-windows-installed-smoke:[\s\S]*?run_keyboard_uia:\s*true/,
    );
    // Display scaling matrix stays on Nightly; release keeps the shorter path.
    expect(releaseWorkflow).toMatch(
      /release-windows-installed-smoke:[\s\S]*?run_display_matrix:\s*false/,
    );
    // One primary arch for install/separation smokes; publish still multi-arch.
    expect(releaseWorkflow).toContain(
      "name: Release macOS installed-app smoke (Apple Silicon)",
    );
    expect(releaseWorkflow).toContain(
      "name: Release Linux installed-app smoke (x64)",
    );
    expect(releaseWorkflow).not.toMatch(
      /release-macos-installed-smoke:[\s\S]*?name:\s*Intel/,
    );
    expect(releaseWorkflow).not.toContain(
      "release-separation-smoke-x86_64-apple",
    );
    expect(releaseWorkflow).not.toContain(
      "release-separation-smoke-aarch64-linux",
    );
    expect(releaseWorkflow).toMatch(
      /release-windows-installed-smoke:[\s\S]*?retention_days:\s*90/,
    );
    expect(releaseWorkflow).toMatch(
      /release-windows-installed-smoke:[\s\S]*?previous_version:\s*""/,
    );
    // Signed release builds keep fat LTO via release_build: true.
    expect(releaseWorkflow).toMatch(
      /release-windows-installed-smoke:[\s\S]*?release_build:\s*true/,
    );
  });

  test("builds and requires macOS updater artifacts in release smoke", () => {
    const macOSWorkflow = readProjectFile(
      ".github/workflows/reusable-macos-installed-app-smoke.yml",
    );
    const linuxWorkflow = readProjectFile(
      ".github/workflows/reusable-linux-installed-app-smoke.yml",
    );
    const windowsWorkflow = readProjectFile(
      ".github/workflows/reusable-windows-installed-app.yml",
    );

    expect(macOSWorkflow).toContain(
      'tauri build --ci --bundles app,dmg --target "${TAURI_TARGET}"',
    );
    expect(macOSWorkflow).toContain(
      "name: ${{ inputs.artifact_prefix }}-updater",
    );
    const macOSValidationStep = readWorkflowStep(
      macOSWorkflow,
      "Validate macOS updater artifacts",
    );
    expect(macOSValidationStep).toContain("if: ${{ inputs.release_build }}");
    expect(macOSValidationStep).toContain('-name "*.tar.gz"');
    expect(macOSValidationStep).toContain(
      "RELEASE_TARGET: ${{ inputs.updater_target }}",
    );
    expect(macOSValidationStep).toContain(
      'updater_name="OpenKara_${RELEASE_VERSION}_${RELEASE_TARGET}.app.tar.gz"',
    );
    expect(macOSValidationStep).toContain(
      'cp "$archive" "$UPDATER_DIR/$updater_name"',
    );
    expect(macOSValidationStep).toContain(
      'cp "$signature" "$UPDATER_DIR/$updater_name.sig"',
    );
    expect(macOSValidationStep).toContain(
      "No macOS updater archive was produced.",
    );
    expect(macOSValidationStep).toContain(
      "No macOS updater signature was produced.",
    );

    const linuxValidationStep = readWorkflowStep(
      linuxWorkflow,
      "Validate Linux updater artifacts",
    );
    expect(linuxValidationStep).toContain("if: ${{ inputs.release_build }}");
    expect(linuxValidationStep).toContain('-name "*.AppImage"');
    expect(linuxValidationStep).toContain("mapfile -d '' appimages");
    expect(linuxValidationStep).toContain("${#appimages[@]} != 1");
    expect(linuxValidationStep).toContain("mapfile -d '' signatures");
    expect(linuxValidationStep).toContain("${#signatures[@]} != 1");
    expect(linuxValidationStep).toContain(
      '"${signatures[0]}" != "${appimage}.sig"',
    );
    expect(linuxValidationStep).toContain(
      "Expected exactly one Linux updater AppImage",
    );
    expect(linuxValidationStep).toContain(
      "Expected exactly one Linux updater signature matching",
    );
    expect(linuxValidationStep).toContain('cp "$signature" "$updater_dir/"');
    expect(linuxValidationStep).not.toContain("tar.gz");
    expect(linuxValidationStep).not.toContain('name "*.sig"');

    const windowsValidationStep = readWorkflowStep(
      windowsWorkflow,
      "Validate Windows updater artifacts",
    );
    expect(windowsValidationStep).toContain("if: ${{ inputs.release_build }}");
    expect(windowsValidationStep).toContain('-Filter "*setup.exe"');
    expect(windowsValidationStep).toContain('"$($installer.FullName).sig"');
    expect(windowsValidationStep).toContain(
      "foreach ($installer in $installers)",
    );
    expect(windowsValidationStep).toContain(
      "No Windows updater installer was produced.",
    );
    expect(windowsValidationStep).toContain(
      "No Windows updater signature was produced for",
    );

    const macOSUpdaterStep = readWorkflowStep(
      macOSWorkflow,
      "Upload macOS updater artifacts",
    );
    expect(macOSUpdaterStep).toContain(
      "path: ${{ runner.temp }}/macos-updater",
    );
    expect(macOSUpdaterStep).toContain("if-no-files-found: error");

    const macOSEvidenceStep = readWorkflowStep(
      macOSWorkflow,
      "Generate Rust release evidence fragment",
    );
    expect(macOSEvidenceStep).toContain(
      'find "${RUNNER_TEMP}/updater" -type f',
    );

    const linuxUpdaterStep = readWorkflowStep(
      linuxWorkflow,
      "Upload Linux updater artifacts",
    );
    expect(linuxUpdaterStep).toContain(
      "path: ${{ runner.temp }}/linux-updater",
    );
    expect(linuxUpdaterStep).toContain("if-no-files-found: error");
    expect(linuxUpdaterStep).not.toContain("tar.gz");
    expect(linuxUpdaterStep).not.toContain("*.sig");

    const linuxEvidenceStep = readWorkflowStep(
      linuxWorkflow,
      "Generate Rust release evidence fragment",
    );
    expect(linuxEvidenceStep).toContain(
      'find "${RUNNER_TEMP}/updater" -type f -name "*.AppImage.sig"',
    );
    expect(linuxEvidenceStep).not.toContain('name "*.tar.gz"');
    expect(linuxEvidenceStep).not.toContain('name "*.sig"');

    const windowsUpdaterStep = readWorkflowStep(
      windowsWorkflow,
      "Upload Windows updater artifacts",
    );
    expect(windowsUpdaterStep).toContain(
      "src-tauri/target/release/bundle/**/*setup.exe.sig",
    );
    expect(windowsUpdaterStep).not.toContain(
      "src-tauri/target/release/bundle/**/*setup.exe\n",
    );
    expect(windowsUpdaterStep).toContain("if-no-files-found: error");
  });

  test("requires a matching signature for each Windows installer", () => {
    const installers = [
      "OpenKara_0.12.0_x64-setup.exe",
      "OpenKara_0.12.0_arm64-setup.exe",
    ];
    const matchingSignatures = installers.map((name) => `${name}.sig`);
    const partialSignatures = [`${installers[0]}.sig`];
    const mismatchedSignatures = ["OpenKara_0.12.0_x86-setup.exe.sig"];

    expect(
      installers.every((name) => matchingSignatures.includes(`${name}.sig`)),
    ).toBe(true);
    expect(
      installers.every((name) => partialSignatures.includes(`${name}.sig`)),
    ).toBe(false);
    expect(
      installers.every((name) => mismatchedSignatures.includes(`${name}.sig`)),
    ).toBe(false);
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

  test("keeps release notes in the Release Please workflow", () => {
    const releaseWorkflow = readProjectFile(".github/workflows/release.yml");
    const releasePleaseWorkflow = readProjectFile(
      ".github/workflows/release-please.yml",
    );
    const releasePleaseConfig = readProjectFile("release-please-config.json");
    const installationTemplate = readProjectFile(
      ".github/release-notes-installation.md",
    );

    expect(installationTemplate).toMatch(
      /^<!-- markdownlint-disable MD041 -->\n\n---\n/,
    );
    expect(installationTemplate).toContain(
      "brew install thedavidweng/tap/openkara",
    );
    expect(installationTemplate).toContain(
      "winget install thedavidweng.OpenKara",
    );
    expect(releasePleaseWorkflow).toContain(
      'grep -Fq "## Installation" RELEASE_NOTES.md',
    );
    expect(releasePleaseWorkflow).toContain(
      "cat .github/release-notes-installation.md >> RELEASE_NOTES.md",
    );
    expect(releasePleaseWorkflow).toContain(
      'gh api --method PATCH "${release_endpoint}"',
    );
    expect(releasePleaseWorkflow).toContain('-F "body=@RELEASE_NOTES.md"');
    expect(releasePleaseConfig).toContain('"force-tag-creation": true');
    expect(releaseWorkflow).toContain("name: Verify draft release");
    expect(releaseWorkflow).not.toContain("release-notes-installation.md");
    expect(releaseWorkflow).not.toContain("RELEASE_NOTES.md");
  });

  test("release-please ensures a git tag and dispatches the Release workflow", () => {
    // GITHUB_TOKEN tag/release events do not start other workflows. The
    // release-please path must create a missing tag, create a formal draft
    // release, and workflow_dispatch release.yml so installed-app smokes and
    // publish run.
    const releasePleaseWorkflow = readProjectFile(
      ".github/workflows/release-please.yml",
    );

    expect(releasePleaseWorkflow).toContain("ensure-tag-and-dispatch-release:");
    expect(releasePleaseWorkflow).toContain(
      "name: Ensure tag and dispatch Release",
    );
    expect(releasePleaseWorkflow).toMatch(
      /needs:\s*\[release-please,\s*sync-native-versions\]/,
    );
    expect(releasePleaseWorkflow).toContain("release_created == 'true'");
    expect(releasePleaseWorkflow).toContain("actions: write");
    expect(releasePleaseWorkflow).toContain("git tag -a");
    expect(releasePleaseWorkflow).toContain("untagged-*");
    expect(releasePleaseWorkflow).toContain(".tag_name ==");
    expect(releasePleaseWorkflow).toContain(".name ==");
    expect(releasePleaseWorkflow).toContain(
      'gh api --method POST "repos/${GH_REPO}/releases"',
    );
    expect(releasePleaseWorkflow).toContain(
      "GitHub does not support changing an existing draft's tag in place",
    );
    expect(releasePleaseWorkflow).toContain(
      "No draft GitHub Release was found",
    );
    expect(releasePleaseWorkflow).toContain('-f "tag_name=${TAG_NAME}"');
    expect(releasePleaseWorkflow).toContain(
      '-f "target_commitish=${target_sha}"',
    );
    expect(releasePleaseWorkflow).toContain("gh workflow run release.yml");
    expect(releasePleaseWorkflow).toContain('--ref "${TAG_NAME}"');
    expect(releasePleaseWorkflow).toContain('-f "version=${version}"');
    expect(releasePleaseWorkflow).toContain("tag_name:");
    expect(releasePleaseWorkflow).toContain("version:");
    expect(releasePleaseWorkflow).toContain("sha:");
  });
});
