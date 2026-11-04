import { expect, test } from "vite-plus/test";
import { classifyCodeql } from "../../scripts/ci/classify-codeql.mjs";

test("selects CodeQL languages for PR paths and keeps full scheduled scans", () => {
  expect(
    classifyCodeql(["docs/guide.md"], "pull_request", "user", "docs"),
  ).toEqual([]);
  expect(
    classifyCodeql(["src/App.tsx"], "pull_request", "user", "frontend"),
  ).toEqual(["javascript-typescript"]);
  expect(
    classifyCodeql(["src-tauri/src/lib.rs"], "pull_request", "user", "rust"),
  ).toEqual(["rust"]);
  expect(
    classifyCodeql([".github/workflows/ci.yml"], "pull_request", "user", "ci"),
  ).toHaveLength(5);
  expect(classifyCodeql([], "schedule", "", "")).toHaveLength(5);
  expect(
    classifyCodeql(
      ["package.json", "CHANGELOG.md"],
      "pull_request",
      "github-actions[bot]",
      "release-please--branches--main--openkara",
    ),
  ).toEqual([]);
});
