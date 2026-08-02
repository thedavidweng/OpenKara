import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));

function readProjectFile(path: string) {
  return readFileSync(join(projectRoot, path), "utf8");
}

describe("issue 303 acceptance wiring", () => {
  test("Nightly runs the complete accessibility matrix through Playwright projects", () => {
    const workflow = readProjectFile(".github/workflows/nightly-hardening.yml");
    const playwright = readProjectFile("playwright.config.ts");
    const packageJson = JSON.parse(readProjectFile("package.json"));

    expect(workflow).toContain("pnpm test:a11y:matrix");
    expect(workflow).not.toContain("tests/e2e/accessibility.spec.ts");
    expect(workflow).not.toContain("Get-ChildItem -Path $base");
    expect(packageJson.scripts["test:a11y:matrix"]).toBe(
      "node scripts/ci/run-accessibility-matrix.mjs",
    );
    expect(playwright).toContain('name: "chromium-accessibility"');
    expect(playwright).toContain('name: "webkit-accessibility"');
    expect(playwright).toContain('testMatch: "accessibility/**/*.spec.ts"');
    expect(playwright).toContain(
      'testIgnore: ["accessibility/**/*.spec.ts", "smoke.spec.ts"]',
    );
    expect(
      existsSync(join(projectRoot, "tests/e2e/accessibility.spec.ts")),
    ).toBe(false);
  });

  test("the Windows driver is the only desktop scenario source", () => {
    const driver = readProjectFile("scripts/ci/run-windows-desktop-e2e.ps1");

    expect(
      existsSync(join(projectRoot, "tests/desktop/windows/scenarios.json")),
    ).toBe(false);
    expect(driver).not.toContain("scenarios.json");
    expect(driver).not.toContain("$scenariosPath");
    expect(driver).toContain("$supportedScenarios");
    expect(driver).toContain('"keyboard-workflow"');
    expect(driver).toContain('"installed-workflow"');
    expect(driver).toContain('"multi-window-and-dialogs"');
    expect(driver).not.toContain("Test-IsCiEnvironment");
    expect(driver).not.toContain("accepting control activation");
    expect(driver).not.toContain("accepting control presence");
  });

  test("release requires fresh Nightly evidence for the candidate commit", () => {
    const release = readProjectFile(".github/workflows/release.yml");
    const nightly = readProjectFile(".github/workflows/nightly-hardening.yml");

    expect(release).toContain("verify-nightly-evidence:");
    expect(release).toContain("actions: read");
    expect(release).toContain("nightly-evidence.mjs verify");
    expect(release).toContain("--max-age-hours 24");
    expect(release).toContain("verify-nightly-evidence");
    expect(nightly).toContain("nightly-evidence.mjs create");
    expect(nightly).toContain("nightly-evidence.json");
    expect(nightly).toContain("NEEDS_JSON: ${{ toJson(needs) }}");
  });
});
