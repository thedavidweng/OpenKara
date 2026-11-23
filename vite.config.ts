import { defineConfig, lazyPlugins } from "vite-plus";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
import { copyFileSync, mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
import process from "node:process";
import {
  KUROMOJI_DICT_FILES,
  resolveKuromojiDictDir,
} from "lyric-romanizer/dict";

// Tauri injects this value when remote device debugging is enabled.
const host = process.env.TAURI_DEV_HOST;

function kuromojiDictPlugin() {
  return {
    name: "kuromoji-dict",
    configResolved() {
      const srcDir = resolveKuromojiDictDir();
      const destDir = resolve(
        fileURLToPath(new URL(".", import.meta.url)),
        "public",
        "dict",
      );
      mkdirSync(destDir, { recursive: true });
      for (const file of KUROMOJI_DICT_FILES) {
        copyFileSync(join(srcDir, file), join(destDir, file));
      }
    },
  };
}

// https://vite.dev/config/
export default defineConfig(async () => ({
  define: {
    // Make package.json version available at runtime for About dialog and
    // diagnostic export. Vite replaces this at build time.
    "import.meta.env.PACKAGE_VERSION": JSON.stringify(
      process.env.npm_package_version || "unknown",
    ),
  },
  plugins: lazyPlugins(() => [react(), tailwindcss(), kuromojiDictPlugin()]),
  lint: {
    plugins: ["react", "typescript", "jsx-a11y"],
    rules: {
      "react/react-in-jsx-scope": "off",
      "react/only-export-components": "warn",
      "react/refs": "off",
      "react/globals": "off",
      "react/set-state-in-effect": "off",
      "react/incompatible-library": "off",
      // Vite+ 1.5 turned this on. Every hit is an effect-local `let`
      // (cancelled flags, unlisten handles, rAF bookkeeping) reassigned in the
      // effect's own cleanup or callbacks, not render-scope state.
      "react/immutability": "off",
      "jsx-a11y/prefer-tag-over-role": "off",
      "typescript/ban-ts-comment": "error",
      "eslint/no-warning-comments": [
        "error",
        { terms: ["todo", "fixme", "xxx"], location: "start" },
      ],
      "eslint/complexity": ["error", { max: 50 }],
      "eslint/max-lines": [
        "error",
        { max: 900, skipBlankLines: true, skipComments: true },
      ],
    },
    overrides: [
      {
        files: [
          "**/*.{test,spec}.{ts,tsx}",
          "tests/**",
          "scripts/**",
          "fixtures/**",
          "src/mock/**",
          "src/lib/han-pinyin-initials.ts",
          "src/types/ipc.ts",
        ],
        rules: {
          "eslint/max-lines": "off",
          "eslint/complexity": "off",
        },
      },
      {
        files: [
          "src/stores/catalog-store.ts",
          "src/lib/settings-controller/settings-controller.ts",
        ],
        rules: {
          "eslint/max-lines": [
            "error",
            { max: 1600, skipBlankLines: true, skipComments: true },
          ],
        },
      },
      {
        files: [
          "src/components/Catalog/StreamingBrowser.tsx",
          "src/components/Lyrics/LyricLine.tsx",
          "src/components/Settings/SettingsRuntimeSection.tsx",
          "src/components/Settings/StreamingSources.tsx",
        ],
        rules: {
          "eslint/complexity": ["error", { max: 120 }],
        },
      },
      {
        files: ["src/stores/**/*.{ts,tsx}"],
        rules: {
          "eslint/no-restricted-imports": [
            "error",
            {
              patterns: [
                {
                  group: ["@tauri-apps/*", "@/lib/tauri", "@/lib/tauri/*"],
                  message:
                    "Route platform calls through @/lib/backend instead of importing Tauri directly.",
                },
              ],
            },
          ],
        },
      },
      {
        files: ["website/src/**/*.{ts,tsx}"],
        rules: {
          "eslint/no-restricted-imports": [
            "error",
            {
              patterns: [
                {
                  group: [
                    "@tauri-apps/*",
                    "@/lib/tauri",
                    "@/lib/tauri/*",
                    "@/runtime/*",
                  ],
                  message:
                    "Keep the website preview decoupled from native Tauri APIs and desktop runtimes.",
                },
              ],
            },
          ],
        },
      },
      {
        files: ["src/types/**/*.{ts,tsx}"],
        rules: {
          "eslint/no-restricted-imports": [
            "error",
            {
              patterns: [
                {
                  group: [
                    "@tauri-apps/*",
                    "@/components/*",
                    "@/hooks/*",
                    "@/lib/*",
                    "@/runtime/*",
                    "@/stores/*",
                  ],
                  message:
                    "Keep src/types pure and free of upward imports into runtime, store, or UI layers.",
                },
              ],
            },
          ],
        },
      },
    ],
    ignorePatterns: [
      ".claude",
      ".worktrees",
      "dist",
      "node_modules",
      "src-tauri/target",
      "src-tauri/gen",
    ],
  },
  fmt: {
    singleQuote: false,
    trailingComma: "all",
    printWidth: 80,
    ignorePatterns: [
      "dist",
      "node_modules",
      "src-tauri",
      "**/*.toml",
      "CHANGELOG.md",
    ],
  },
  test: {
    // CI runners have noisy CPU scheduling under parallel jsdom forks; a
    // trivial render can measure 5+ seconds of wall-clock under contention
    // even though the test logic itself is <100ms. 15s gives headroom for
    // env setup + first render without masking genuinely hung tests.
    testTimeout: 15000,
    // Per-file setup (beforeAll/beforeEach) can also be starved under the
    // same contention; keep hooks generous so setup never fails the file.
    hookTimeout: 15000,
    // Cap worker parallelism slightly below CPU count on CI so the main
    // process + v8 coverage instrumentation get headroom. Locally
    // (multi-core dev machines) we let vitest use the default (all cores).
    // NOTE: Vitest 4 removed poolOptions; maxWorkers is now top-level.
    maxWorkers: process.env.CI ? 3 : undefined,
    // Nested git worktrees under `.worktrees/` duplicate `src/**` and must not
    // be collected as part of this package's unit test run.
    include: [
      "src/**/*.{test,spec}.?(c|m)[jt]s?(x)",
      "tests/**/*.{test,spec}.?(c|m)[jt]s?(x)",
    ],
    exclude: [
      "**/node_modules/**",
      "**/dist/**",
      "**/.worktrees/**",
      "tests/e2e/**",
    ],
    setupFiles: ["./src/test-setup.ts"],
    coverage: {
      provider: "v8",
      reporter: [
        "text",
        "text-summary",
        "json",
        "json-summary",
        "lcov",
        "html",
      ],
      reportsDirectory: "./coverage",
      thresholds: {
        // Lines/statements at 65%: nearly all testable pure logic and store
        // actions are covered; remaining gap is UI component render paths.
        // Functions/branches at 60%: remaining uncovered code is React UI
        // component event handlers that require @testing-library/react or
        // Playwright E2E to exercise — not practical for unit tests alone.
        // Note: these are 65/60, not 70% — see PR description for context.
        lines: 65,
        statements: 65,
        functions: 60,
        branches: 60,
      },
      // Files excluded from coverage measurement — these are either Tauri
      // IPC thin wrappers (invoke() calls), app entry points, Web Workers,
      // or deep native-API modules that require a real Tauri runtime.
      exclude: [
        // Tauri IPC thin wrappers
        "src/lib/tauri/*.ts",
        // App entry point
        "src/main.tsx",
        // Web Worker
        "src/workers/romanize.worker.ts",
        // Shared in-memory Tauri fake — a test double whose real exercise is
        // Playwright E2E and the website preview, not vitest.
        "src/mock/tauri-mock-impl.ts",
        // Deep Tauri native API dependencies
        "src/runtime/window-shell-runtime.ts",
        "src/runtime/theme-runtime.ts",
        "src/lib/native-context-menu.ts",
        // Heavy Tauri-dependent UI components (covered by Playwright E2E)
        "src/components/Library/ImportCdgChoiceDialog.tsx",
        "src/components/Library/SongEditDialog.tsx",
        "src/components/Library/SongPropertiesDialog.tsx",
        "src/components/Library/ImportButton.tsx",
        "src/components/Bootstrap/ModelBootstrapBanner.tsx",
        "src/components/Cdg/CdgCanvas.tsx",
      ],
    },
  },
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  build: {
    chunkSizeWarningLimit: 1000,
  },
  worker: {
    format: "es",
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
