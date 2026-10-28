import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";

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
  test("uploads the AppImage and its matching delta asset", () => {
    const root = mkdtempSync(join(tmpdir(), "openkara delta publication "));
    try {
      const image = join(root, "OpenKara_1.2.3_amd64.AppImage");
      writeFileSync(image, "candidate");
      writeFileSync(`${image}.zsync`, "delta control");
      writeFileSync(
        join(root, "gh"),
        '#!/bin/sh\nprintf \'%s\\n\' "$@" > "$CALL_LOG"\n',
        {
          mode: 0o755,
        },
      );
      const script = readWorkflowStep(
        readProjectFile(".github/workflows/release.yml"),
        "Publish tested Release Candidate",
      )
        .split("        run: |\n")[1]
        .split("\n")
        .map((line) => line.replace(/^ {10}/, ""))
        .join("\n");
      const result = spawnSync("bash", ["-c", script], {
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${root}:${process.env.PATH}`,
          CANDIDATE_ROOT: root,
          CALL_LOG: join(root, "upload-args"),
          GH_REPO: "test/OpenKara",
          RELEASE_TAG: "v1.2.3",
        },
      });
      expect(result.status, result.stderr).toBe(0);
      const args = readFileSync(join(root, "upload-args"), "utf8")
        .trim()
        .split("\n");
      expect(args.slice(0, 5)).toEqual([
        "release",
        "upload",
        "v1.2.3",
        "--repo",
        "test/OpenKara",
      ]);
      expect(args.slice(5, -1).sort()).toEqual([image, `${image}.zsync`]);
      expect(args.at(-1)).toBe("--clobber");
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
