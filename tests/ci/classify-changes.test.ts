// Contract tests for the CI change classifier.
//
// These tests verify the classification contract specified in issue #155:
// every changed file is known or explicitly printed as unknown, and the
// expected job set matches the issue's required result for each fixture.
//
// Fixtures are derived from real PRs (#147, #148, #149, #152, #153, #154)
// plus synthetic cases covering docs, deps, unknown, and mixed changes.

import { describe, expect, test } from "vitest";
import {
  classifyChanges,
  classifyFile,
  globToRegex,
  CATEGORY_PATTERNS,
  ALL_JOBS,
} from "../../scripts/ci/classify-changes.mjs";

// ── Helpers ───────────────────────────────────────────────────────────

/** Classify files as a pull_request and return the result. */
function pr(...files: string[]) {
  return classifyChanges(files, "pull_request");
}

/** Job sets that constitute "full CI" (all heavy + quality jobs). */
const FULL_CI_HEAVY = [
  "cargo-deny",
  "dependency-checks",
  "prepare-model",
  "rust-test",
  "rust-test-windows-compile",
  "tauri-build",
];

// ── Glob matcher unit tests ───────────────────────────────────────────

describe("globToRegex", () => {
  test("exact match", () => {
    const re = globToRegex("vite.config.ts");
    expect(re.test("vite.config.ts")).toBe(true);
    expect(re.test("src/vite.config.ts")).toBe(false);
  });

  test("dir/** matches files at any depth", () => {
    const re = globToRegex("src/**");
    expect(re.test("src/foo.ts")).toBe(true);
    expect(re.test("src/sub/bar.ts")).toBe(true);
    expect(re.test("src-tauri/foo.rs")).toBe(false);
  });

  test("**/*.md matches at any depth including root", () => {
    const re = globToRegex("**/*.md");
    expect(re.test("AGENTS.md")).toBe(true);
    expect(re.test("docs/guide.md")).toBe(true);
    expect(re.test("docs/references/contracts/x.md")).toBe(true);
    expect(re.test("site.css")).toBe(false);
  });

  test("star glob (render-*.mjs)", () => {
    const re = globToRegex("scripts/render-*.mjs");
    expect(re.test("scripts/render-flatpak-manifest.mjs")).toBe(true);
    expect(re.test("scripts/render-winget-manifests.mjs")).toBe(true);
    expect(re.test("scripts/check-i18n.mjs")).toBe(false);
  });

  test("dotfiles", () => {
    const re = globToRegex(".oxfmtrc.json");
    expect(re.test(".oxfmtrc.json")).toBe(true);
    expect(re.test("oxfmtrc.json")).toBe(false);
  });
});

// ── Single-file classification ────────────────────────────────────────

describe("classifyFile", () => {
  test("src component is frontend_app", () => {
    expect(classifyFile("src/components/Lyrics/LyricLine.tsx")).toEqual([
      "frontend_app",
    ]);
  });

  test("website CSS is website (not frontend_app)", () => {
    expect(classifyFile("website/src/site.css")).toEqual(["website"]);
  });

  test("Rust source is rust", () => {
    expect(classifyFile("src-tauri/src/audio/mixer.rs")).toEqual(["rust"]);
  });

  test("release.yml is release_workflow (not other_workflow)", () => {
    expect(classifyFile(".github/workflows/release.yml")).toEqual([
      "release_workflow",
    ]);
  });

  test("ci.yml is ci_workflow", () => {
    expect(classifyFile(".github/workflows/ci.yml")).toEqual(["ci_workflow"]);
  });

  test("packaging.yml is packaging_workflow", () => {
    expect(classifyFile(".github/workflows/packaging.yml")).toEqual([
      "packaging_workflow",
    ]);
  });

  test("mirror.yml is other_workflow", () => {
    expect(classifyFile(".github/workflows/mirror.yml")).toEqual([
      "other_workflow",
    ]);
  });

  test("AGENTS.md is docs", () => {
    expect(classifyFile("AGENTS.md")).toEqual(["docs"]);
  });

  test("package.json is deps_js", () => {
    expect(classifyFile("package.json")).toEqual(["deps_js"]);
  });

  test("Cargo.toml is deps_rust", () => {
    expect(classifyFile("src-tauri/Cargo.toml")).toEqual(["deps_rust"]);
  });

  test("prepare-onnx-runtime.mjs is model_runtime and packaging_inputs", () => {
    // packaging.yml triggers on this file, so it must classify into
    // packaging_inputs to gate validate-flatpak and build-flatpak.
    const cats = classifyFile("scripts/prepare-onnx-runtime.mjs");
    expect(cats).toContain("model_runtime");
    expect(cats).toContain("packaging_inputs");
  });

  test("playwright.config.ts is e2e and frontend_tooling", () => {
    const cats = classifyFile("playwright.config.ts");
    expect(cats).toContain("e2e");
  });

  test("tests/e2e spec is e2e", () => {
    expect(classifyFile("tests/e2e/player.spec.ts")).toEqual(["e2e"]);
  });

  test("packaging dir is packaging_inputs", () => {
    expect(classifyFile("packaging/flatpak/foo.xml")).toEqual([
      "packaging_inputs",
    ]);
  });

  test("tauri.conf.json is packaging_inputs", () => {
    expect(classifyFile("src-tauri/tauri.conf.json")).toEqual([
      "packaging_inputs",
    ]);
  });

  test("cliff.toml is release_metadata", () => {
    expect(classifyFile("cliff.toml")).toEqual(["release_metadata"]);
  });

  test("unmapped file has no categories", () => {
    expect(classifyFile("new-unmapped-root-config.xyz")).toEqual([]);
  });

  test("mise.toml is unknown (no category)", () => {
    expect(classifyFile("mise.toml")).toEqual([]);
  });

  test(".gitignore is unknown (no category)", () => {
    expect(classifyFile(".gitignore")).toEqual([]);
  });
});

// ── Invariants ────────────────────────────────────────────────────────

describe("classification invariants", () => {
  test("known file never appears in unknownFiles", () => {
    const result = pr("src/foo.ts", "unknown-file.xyz");
    expect(result.unknownFiles).toEqual(["unknown-file.xyz"]);
    expect(result.unknownFiles).not.toContain("src/foo.ts");
  });

  test("unknown file always appears in unknownFiles", () => {
    const result = pr("totally-unknown.xyz", "src/foo.ts");
    expect(result.unknownFiles).toContain("totally-unknown.xyz");
  });

  test("every file is known or in unknownFiles", () => {
    const files = ["src/foo.ts", "docs/guide.md", "unknown.xyz"];
    const result = pr(...files);
    for (const file of files) {
      const isKnown = (result.categoriesByFile[file] ?? []).length > 0;
      const isUnknown = result.unknownFiles.includes(file);
      expect(isKnown || isUnknown).toBe(true);
    }
  });

  test("unknown category is only set when unknownFiles is non-empty", () => {
    const known = pr("src/foo.ts");
    expect(known.categories).not.toContain("unknown");

    const unknown = pr("unknown.xyz");
    expect(unknown.categories).toContain("unknown");
  });
});

// ── PR fixtures from issue #155 ───────────────────────────────────────

describe("PR fixtures", () => {
  test("PR #153 — pure frontend (two Lyrics files)", () => {
    const result = pr(
      "src/components/Lyrics/LyricLine.tsx",
      "src/components/Lyrics/LyricLine.test.tsx",
    );
    expect(result.categories).toEqual(["frontend_app"]);
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("app-frontend");
    expect(result.expectedJobs).toContain("playwright-ui-smoke");
    expect(result.expectedJobs).toContain("tauri-build-smoke");
    expect(result.expectedJobs).toContain("js-quality");
    // No Rust/model/platform matrix
    for (const job of FULL_CI_HEAVY) {
      expect(result.expectedJobs).not.toContain(job);
    }
  });

  test("PR #148 — website-only CSS", () => {
    const result = pr("website/src/site.css");
    expect(result.categories).toEqual(["website"]);
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("website");
    expect(result.expectedJobs).toContain("js-quality");
    // No app frontend, Playwright, Rust, model, or platform
    expect(result.expectedJobs).not.toContain("app-frontend");
    expect(result.expectedJobs).not.toContain("playwright-ui-smoke");
    for (const job of FULL_CI_HEAVY) {
      expect(result.expectedJobs).not.toContain(job);
    }
  });

  test("PR #147 — store + store test + changelog", () => {
    const result = pr(
      "src/stores/rotation-store.ts",
      "src/stores/rotation-store.test.ts",
      "CHANGELOG.md",
    );
    expect(result.categories).toContain("frontend_app");
    expect(result.categories).toContain("docs");
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("app-frontend");
    expect(result.expectedJobs).toContain("playwright-ui-smoke");
    expect(result.expectedJobs).toContain("tauri-build-smoke");
    for (const job of FULL_CI_HEAVY) {
      expect(result.expectedJobs).not.toContain(job);
    }
  });

  test("PR #149 — components/lib/styles/tests", () => {
    const result = pr(
      "src/components/Playback/Player.tsx",
      "src/lib/cover-art.ts",
      "src/styles/globals.css",
    );
    expect(result.categories).toEqual(["frontend_app"]);
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("app-frontend");
    expect(result.expectedJobs).toContain("playwright-ui-smoke");
    expect(result.expectedJobs).toContain("tauri-build-smoke");
    for (const job of FULL_CI_HEAVY) {
      expect(result.expectedJobs).not.toContain(job);
    }
  });

  test("PR #144 — Rust audio + tests + contract doc", () => {
    const result = pr(
      "src-tauri/src/audio/mixer.rs",
      "src-tauri/tests/audio_test.rs",
      "docs/references/contracts/playback.md",
    );
    expect(result.categories).toContain("rust");
    expect(result.categories).toContain("docs");
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("rust-test");
    expect(result.expectedJobs).toContain("rust-test-windows-compile");
    expect(result.expectedJobs).toContain("cargo-deny");
    expect(result.expectedJobs).toContain("dependency-checks");
    expect(result.expectedJobs).toContain("prepare-model");
    expect(result.expectedJobs).toContain("tauri-build");
    // No frontend-only smoke
    expect(result.expectedJobs).not.toContain("tauri-build-smoke");
  });

  test("PR #152 — mixed frontend + Rust + docs = full CI", () => {
    const result = pr("src/foo.ts", "src-tauri/src/foo.rs", "docs/guide.md");
    expect(result.categories).toContain("frontend_app");
    expect(result.categories).toContain("rust");
    expect(result.categories).toContain("docs");
    expect(result.unknownFiles).toEqual([]);
    // Full CI: both frontend and Rust jobs
    expect(result.expectedJobs).toContain("app-frontend");
    expect(result.expectedJobs).toContain("rust-test");
    expect(result.expectedJobs).toContain("tauri-build");
    // tauri-build-smoke is suppressed when heavy (Rust) is present
    expect(result.expectedJobs).not.toContain("tauri-build-smoke");
  });

  test("PR #154 — release.yml only", () => {
    const result = pr(".github/workflows/release.yml");
    expect(result.categories).toEqual(["release_workflow"]);
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("workflow-lint");
    expect(result.expectedJobs).toContain("release-validation");
    // No heavy jobs
    expect(result.expectedJobs).not.toContain("app-frontend");
    expect(result.expectedJobs).not.toContain("playwright-ui-smoke");
    expect(result.expectedJobs).not.toContain("website");
    for (const job of FULL_CI_HEAVY) {
      expect(result.expectedJobs).not.toContain(job);
    }
  });
});

// ── Synthetic fixtures from issue #155 table ──────────────────────────

describe("synthetic fixtures", () => {
  test("CI workflow (.github/workflows/ci.yml) = full CI", () => {
    const result = pr(".github/workflows/ci.yml");
    expect(result.categories).toEqual(["ci_workflow"]);
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("workflow-lint");
    expect(result.expectedJobs).toContain("app-frontend");
    expect(result.expectedJobs).toContain("rust-test");
    expect(result.expectedJobs).toContain("tauri-build");
  });

  test("Packaging workflow (.github/workflows/packaging.yml)", () => {
    const result = pr(".github/workflows/packaging.yml");
    expect(result.categories).toEqual(["packaging_workflow"]);
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("workflow-lint");
    // No app CI for packaging-workflow-only
    expect(result.expectedJobs).not.toContain("app-frontend");
    expect(result.expectedJobs).not.toContain("rust-test");
  });

  test("E2E (tests/e2e/player.spec.ts) = Playwright", () => {
    const result = pr("tests/e2e/player.spec.ts");
    expect(result.categories).toEqual(["e2e"]);
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("playwright-ui-smoke");
    expect(result.expectedJobs).toContain("js-quality");
    for (const job of FULL_CI_HEAVY) {
      expect(result.expectedJobs).not.toContain(job);
    }
  });

  test("Playwright config (playwright.config.ts) = e2e", () => {
    const result = pr("playwright.config.ts");
    expect(result.categories).toContain("e2e");
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("playwright-ui-smoke");
  });

  test("JS deps (package.json + pnpm-lock.yaml)", () => {
    const result = pr("package.json", "pnpm-lock.yaml");
    expect(result.categories).toContain("deps_js");
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("app-frontend");
    expect(result.expectedJobs).toContain("website");
    expect(result.expectedJobs).toContain("playwright-ui-smoke");
    expect(result.expectedJobs).toContain("tauri-build-smoke");
  });

  test("Rust deps (Cargo.toml + Cargo.lock)", () => {
    const result = pr("src-tauri/Cargo.toml", "src-tauri/Cargo.lock");
    expect(result.categories).toContain("deps_rust");
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("rust-test");
    expect(result.expectedJobs).toContain("tauri-build");
    expect(result.expectedJobs).toContain("cargo-deny");
  });

  test("docs only (docs/guide.md) = no heavy jobs", () => {
    const result = pr("docs/guide.md");
    expect(result.categories).toEqual(["docs"]);
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toEqual([
      "triage",
      "conventional-commits",
      "ci-gate",
    ]);
  });

  test("root docs (AGENTS.md) = no heavy jobs", () => {
    const result = pr("AGENTS.md");
    expect(result.categories).toEqual(["docs"]);
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toEqual([
      "triage",
      "conventional-commits",
      "ci-gate",
    ]);
  });

  test("unknown file = full CI", () => {
    const result = pr("new-unmapped-root-config.xyz");
    expect(result.categories).toEqual(["unknown"]);
    expect(result.unknownFiles).toEqual(["new-unmapped-root-config.xyz"]);
    // Full CI: all heavy jobs
    for (const job of FULL_CI_HEAVY) {
      expect(result.expectedJobs).toContain(job);
    }
    expect(result.expectedJobs).toContain("app-frontend");
    expect(result.expectedJobs).toContain("website");
    expect(result.expectedJobs).toContain("playwright-ui-smoke");
    expect(result.expectedJobs).toContain("workflow-lint");
  });

  test("mixed release/frontend = union of release + frontend", () => {
    const result = pr(".github/workflows/release.yml", "src/foo.ts");
    expect(result.categories).toContain("release_workflow");
    expect(result.categories).toContain("frontend_app");
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("workflow-lint");
    expect(result.expectedJobs).toContain("release-validation");
    expect(result.expectedJobs).toContain("app-frontend");
    expect(result.expectedJobs).toContain("playwright-ui-smoke");
    expect(result.expectedJobs).toContain("tauri-build-smoke");
  });

  test("mixed website/Rust = union of website + Rust", () => {
    const result = pr("website/x.ts", "src-tauri/src/x.rs");
    expect(result.categories).toContain("website");
    expect(result.categories).toContain("rust");
    expect(result.unknownFiles).toEqual([]);
    expect(result.expectedJobs).toContain("website");
    expect(result.expectedJobs).toContain("rust-test");
    expect(result.expectedJobs).toContain("tauri-build");
    // tauri-build-smoke suppressed because Rust (heavy) is present
    expect(result.expectedJobs).not.toContain("tauri-build-smoke");
  });
});

// ── Event type behavior ───────────────────────────────────────────────

describe("event type behavior", () => {
  test("push to main always runs full CI regardless of files", () => {
    const result = classifyChanges(["docs/guide.md"], "push");
    // Even docs-only changes on push trigger full CI
    for (const job of FULL_CI_HEAVY) {
      expect(result.expectedJobs).toContain(job);
    }
  });

  test("push still classifies files into categories for downstream consumers", () => {
    // The packaging workflow reads categories to gate packaging jobs. On
    // push events, categories must still reflect the actual changed files
    // so packaging gates work — not a blanket "unknown".
    const result = classifyChanges(
      ["packaging/com.openkara.OpenKara.yml", "docs/guide.md"],
      "push",
    );
    expect(result.categories).toContain("packaging_inputs");
    expect(result.categories).toContain("docs");
    expect(
      result.categoriesByFile["packaging/com.openkara.OpenKara.yml"],
    ).toContain("packaging_inputs");
    // expectedJobs is still full CI (safety path).
    for (const job of FULL_CI_HEAVY) {
      expect(result.expectedJobs).toContain(job);
    }
  });

  test("workflow_dispatch always runs full CI", () => {
    const result = classifyChanges([], "workflow_dispatch");
    for (const job of FULL_CI_HEAVY) {
      expect(result.expectedJobs).toContain(job);
    }
  });

  test("conventional-commits only expected on pull_request", () => {
    const prResult = pr("src/foo.ts");
    expect(prResult.expectedJobs).toContain("conventional-commits");

    const pushResult = classifyChanges(["src/foo.ts"], "push");
    expect(pushResult.expectedJobs).not.toContain("conventional-commits");
  });
});

// ── Structural invariants ─────────────────────────────────────────────

describe("structural invariants", () => {
  test("ALL_JOBS includes every job referenced by the workflow", () => {
    // These are the job IDs that ci.yml must define.
    const requiredJobs = [
      "triage",
      "conventional-commits",
      "ci-gate",
      "js-quality",
      "app-frontend",
      "website",
      "playwright-ui-smoke",
      "workflow-lint",
      "release-validation",
      "cargo-deny",
      "dependency-checks",
      "prepare-model",
      "rust-test",
      "rust-test-windows-compile",
      "tauri-build-smoke",
      "tauri-build",
    ];
    for (const job of requiredJobs) {
      expect(ALL_JOBS).toContain(job);
    }
  });

  test("no category pattern is empty (except e2e_workflow reserved)", () => {
    for (const [cat, patterns] of Object.entries(CATEGORY_PATTERNS)) {
      if (cat === "e2e_workflow") continue; // reserved for future
      expect(patterns.length, `${cat} has no patterns`).toBeGreaterThan(0);
    }
  });
});
