import { spawnSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));

describe("Flatpak packaging", () => {
  test.skipIf(process.platform === "win32")(
    "renders pinned channel runtimes and installs the executable and license",
    () => {
      const directory = mkdtempSync(
        join(tmpdir(), "openkara-flatpak-runtime-"),
      );
      try {
        const result = spawnSync(
          process.execPath,
          [
            "scripts/render-flatpak-manifest.mjs",
            "--version",
            "0.17.0",
            "--source-url",
            "data:application/octet-stream,source-fixture",
            "--output",
            directory,
          ],
          { cwd: projectRoot, encoding: "utf8" },
        );
        expect(result.status, result.stderr).toBe(0);
        const manifest = readFileSync(
          join(
            directory,
            "io.github.thedavidweng.OpenKara/io.github.thedavidweng.OpenKara.yml",
          ),
          "utf8",
        );
        const archives = Array.from(
          manifest.matchAll(
            /url: (https:\/\/nodejs\.org\/\S+)\n\s+sha256: (\w+)\n\s+dest: source-runtime\n\s+only-arches:\n\s+- (\S+)/g,
          ),
          (match) => ({ url: match[1], sha256: match[2], arch: match[3] }),
        );
        expect(archives).toEqual([
          {
            url: "https://nodejs.org/dist/v24.22.1/node-v24.22.1-linux-x64.tar.gz",
            sha256:
              "eb6728e77c576a35162c122c45ccadf224d89ff00abc8dae46cc2897c6002d7c",
            arch: "x86_64",
          },
          {
            url: "https://nodejs.org/dist/v24.22.1/node-v24.22.1-linux-arm64.tar.gz",
            sha256:
              "33c37be234ba8bee041961047b19160f4d7d0f164e8e246f78bdd10feff148b3",
            arch: "aarch64",
          },
        ]);
        mkdirSync(join(directory, "source-runtime/bin"), { recursive: true });
        writeFileSync(
          join(directory, "source-runtime/bin/node"),
          "runtime-fixture",
        );
        writeFileSync(
          join(directory, "source-runtime/LICENSE"),
          "license-fixture",
        );
        const commands = Array.from(
          manifest.matchAll(
            /^\s+- ((?:mkdir|install) .*source-runtime\/.*)$/gm,
          ),
          (match) => match[1],
        );
        expect(commands).toHaveLength(2);
        const destination = join(directory, "app");
        const install = spawnSync("sh", ["-ec", commands.join("\n")], {
          cwd: directory,
          env: { ...process.env, FLATPAK_DEST: destination },
          encoding: "utf8",
        });
        expect(install.status, install.stderr).toBe(0);
        expect(
          readFileSync(join(destination, "bin/openkara-source-node"), "utf8"),
        ).toBe("runtime-fixture");
        expect(
          readFileSync(
            join(
              destination,
              "share/licenses/io.github.thedavidweng.OpenKara/source-runtime/LICENSE",
            ),
            "utf8",
          ),
        ).toBe("license-fixture");
      } finally {
        rmSync(directory, { recursive: true, force: true });
      }
    },
  );
});
