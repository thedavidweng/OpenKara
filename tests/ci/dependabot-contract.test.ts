import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const workflowsDir = join(projectRoot, ".github/workflows");
const dependabotConfigPath = join(projectRoot, ".github/dependabot.yml");
const dependabotSyncPath = join(workflowsDir, "dependabot-sync.yml");

function ecosystemBlock(config: string, ecosystem: string): string {
  const blocks = config.split(/\n {2}- package-ecosystem: /);
  const match = blocks
    .slice(1)
    .find((block) => block.startsWith(`"${ecosystem}"`));
  if (!match) {
    throw new Error(`Missing ecosystem block for ${ecosystem}`);
  }
  return `  - package-ecosystem: ${match}`;
}

function groupBlock(ecosystemSection: string, groupName: string): string {
  const lines = ecosystemSection.split(/\r?\n/);
  const start = lines.findIndex((line) => line === `      ${groupName}:`);
  if (start === -1) {
    throw new Error(`Missing group ${groupName}`);
  }
  const blockLines = [lines[start]];
  for (let index = start + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.trim().length > 0 && line.search(/\S/) <= 6) {
      break;
    }
    blockLines.push(line);
  }
  return blockLines.join("\n");
}

describe("Dependabot configuration and sync contract", () => {
  test("does not configure automated PR merging in workflows", () => {
    const workflows = readdirSync(workflowsDir).filter(
      (name) => name.endsWith(".yml") || name.endsWith(".yaml"),
    );
    for (const file of workflows) {
      const content = readFileSync(join(workflowsDir, file), "utf8");
      expect(content, file).not.toContain("dependabot/fetch-metadata");
      expect(content, file).not.toContain("gh pr merge --auto");
    }
  });

  test("preserves weekly Monday schedule and cooldowns across ecosystems", () => {
    const config = readFileSync(dependabotConfigPath, "utf8");
    for (const ecosystem of ["cargo", "github-actions", "npm"]) {
      const block = ecosystemBlock(config, ecosystem);
      expect(block, `${ecosystem} interval`).toContain('interval: "weekly"');
      expect(block, `${ecosystem} day`).toContain('day: "monday"');
      expect(block, `${ecosystem} cooldown`).toContain("cooldown:");
      expect(block, `${ecosystem} default-days`).toContain("default-days: 7");
    }
  });

  test("keeps cargo tauri* ignore exclusion and groups cargo updates across major, minor, and patch", () => {
    const config = readFileSync(dependabotConfigPath, "utf8");
    const cargo = ecosystemBlock(config, "cargo");
    expect(cargo).toContain('dependency-name: "tauri*"');

    const group = groupBlock(cargo, "rust-dependencies");
    expect(group).toContain('- "major"');
    expect(group).toContain('- "minor"');
    expect(group).toContain('- "patch"');
  });

  test("groups github-actions updates across major, minor, and patch", () => {
    const config = readFileSync(dependabotConfigPath, "utf8");
    const actions = ecosystemBlock(config, "github-actions");
    const group = groupBlock(actions, "github-actions");
    expect(group).toContain('- "*"');
    expect(group).toContain('- "major"');
    expect(group).toContain('- "minor"');
    expect(group).toContain('- "patch"');
  });

  test("groups npm development and production updates including minors and majors", () => {
    const config = readFileSync(dependabotConfigPath, "utf8");
    const npm = ecosystemBlock(config, "npm");

    const devGroup = groupBlock(npm, "dev-dependencies");
    expect(devGroup).toContain('dependency-type: "development"');
    expect(devGroup).toContain('- "major"');
    expect(devGroup).toContain('- "minor"');
    expect(devGroup).toContain('- "patch"');

    const prodGroup = groupBlock(npm, "production-dependencies");
    expect(prodGroup).toContain('dependency-type: "production"');
    expect(prodGroup).toContain('- "major"');
    expect(prodGroup).toContain('- "minor"');
    expect(prodGroup).toContain('- "patch"');
  });

  test("keeps Flatpak sources sync workflow scoped to lockfile paths and same-repo Dependabot PRs", () => {
    const workflow = readFileSync(dependabotSyncPath, "utf8");
    expect(workflow).toContain("paths:");
    expect(workflow).toContain('"pnpm-lock.yaml"');
    expect(workflow).toContain('"Cargo.lock"');
    expect(workflow).toContain('"src-tauri/Cargo.lock"');
    expect(workflow).toContain(
      "github.event.pull_request.user.login == 'dependabot[bot]'",
    );
    expect(workflow).toContain(
      "github.event.pull_request.head.repo.full_name == github.repository",
    );
    expect(workflow).toContain(
      "node scripts/generate-flatpak-node-sources.mjs",
    );
    expect(workflow).toContain(
      "node scripts/generate-flatpak-cargo-sources.mjs",
    );
  });
});
