import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));

function readProjectFile(path: string) {
  return readFileSync(join(projectRoot, path), "utf8");
}

describe("Flatpak packaging", () => {
  test("targets current supported Flathub runtimes and both default architectures", () => {
    const manifestTemplate = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.yml.in",
    );

    expect(manifestTemplate).toContain('runtime-version: "50"');
    expect(manifestTemplate).toContain("org.freedesktop.Sdk.Extension.node24");
    expect(manifestTemplate).not.toContain("node20");
    expect(
      existsSync(join(projectRoot, "packaging/flatpak/flathub.json")),
    ).toBe(false);

    expect(manifestTemplate).toContain("x86_64");
    expect(manifestTemplate).toContain("aarch64");
  });

  test("uses pre-built ONNX Runtime binaries with architecture-specific sources for the Flathub submission", () => {
    const manifestTemplate = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.yml.in",
    );

    expect(manifestTemplate).toContain("onnxruntime-source");
    expect(manifestTemplate).toContain("onnxruntime-linux-x64");
    expect(manifestTemplate).toContain("onnxruntime-linux-aarch64");
    expect(manifestTemplate).toContain("${FLATPAK_DEST}/share/licenses");
    expect(manifestTemplate).not.toContain("build_shared_lib");
  });

  test("builds the Flatpak app binary without asking Tauri to create Linux bundles", () => {
    const manifestTemplate = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.yml.in",
    );

    expect(manifestTemplate).toContain("pnpm tauri build");
    expect(manifestTemplate).toContain("--no-bundle");
    expect(manifestTemplate).not.toContain("--bundles none");
  });

  test("includes generated dependency manifests instead of copying them as files", () => {
    const manifestTemplate = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.yml.in",
    );
    const renderScript = readProjectFile("scripts/render-flatpak-manifest.mjs");

    expect(manifestTemplate).toMatch(/\n\s+- cargo-sources\.json\n/);
    expect(manifestTemplate).not.toMatch(
      /type:\s*file\s*\n\s*path:\s*cargo-sources\.json/,
    );

    expect(renderScript).toMatch(/` {6}- \${file}`/);
    expect(renderScript).not.toContain(
      "`      - type: file\\n        path: ${file}`",
    );
  });

  test("keeps app metadata and Flatpak-only Tauri config in the upstream source archive", () => {
    const renderScript = readProjectFile("scripts/render-flatpak-manifest.mjs");
    const metainfo = readProjectFile(
      "packaging/flatpak/io.github.thedavidweng.OpenKara.metainfo.xml",
    );

    expect(
      existsSync(
        join(
          projectRoot,
          "packaging/flatpak/io.github.thedavidweng.OpenKara.metainfo.xml",
        ),
      ),
    ).toBe(true);
    expect(
      existsSync(
        join(
          projectRoot,
          "packaging/flatpak/io.github.thedavidweng.OpenKara.metainfo.xml.in",
        ),
      ),
    ).toBe(false);

    expect(renderScript).not.toContain(
      "io.github.thedavidweng.OpenKara.desktop",
    );
    expect(renderScript).not.toContain(
      "io.github.thedavidweng.OpenKara.metainfo.xml",
    );
    expect(renderScript).not.toContain("tauri.flatpak.conf.json");
    expect(renderScript).not.toContain("flathub.json");
    expect(metainfo).not.toContain("/main/packaging/flatpak/screenshots/");
    expect(metainfo).toContain("/v0.8.1/packaging/flatpak/screenshots/");
  });

  test("keeps pnpm dependency sources in sync with the lockfile versions used by the app", () => {
    const nodeSources = readProjectFile(
      "packaging/flatpak/generated/node-sources.0.json",
    );

    expect(nodeSources).toContain("@tauri-apps__api-2.11.0.tgz");
    expect(nodeSources).toContain("react-19.2.5.tgz");
    expect(nodeSources).toContain("vite-8.0.10.tgz");
    expect(nodeSources).not.toContain("react-19.2.4.tgz");
    expect(nodeSources).not.toContain("vite-7.3.1.tgz");
  });

  test("release automation never opens initial Flathub submission PRs automatically", () => {
    const releaseWorkflow = readProjectFile(".github/workflows/release.yml");

    expect(releaseWorkflow).toContain("GITHUB_TOKEN: ${{ github.token }}");
    expect(releaseWorkflow).toContain("Ensure release source tag exists");
    expect(releaseWorkflow).toContain('git push origin "refs/tags/${tag}"');
    expect(releaseWorkflow).toContain(
      '--title "New version: ${WINGET_PACKAGE_IDENTIFIER} version ${VERSION}"',
    );
    expect(releaseWorkflow).not.toContain(
      "WinGet PR could not be created automatically.",
    );
    expect(releaseWorkflow).not.toContain("skipping WinGet PR automation");
    expect(releaseWorkflow).toContain("manual_submission_url=");
    expect(releaseWorkflow).toContain(
      "Initial Flathub submissions must be opened or updated manually",
    );
    expect(releaseWorkflow).not.toContain("--draft");
    expect(releaseWorkflow).not.toContain(
      "Open this prefilled GitHub URL to create the Flathub submission PR",
    );
    expect(releaseWorkflow).not.toContain("skipping Flathub PR automation");
  });
});
