import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));

function readProjectFile(path: string) {
  return readFileSync(join(projectRoot, path), "utf8");
}

describe("nightly installers workflow", () => {
  const workflow = readProjectFile(".github/workflows/nightly-installers.yml");

  test("scopes write permissions to the publish job", () => {
    expect(workflow).toMatch(/^permissions:\n  contents: read$/m);
    const writeGrants = workflow.match(/contents: write/g) ?? [];
    expect(writeGrants).toHaveLength(1);
  });
});
