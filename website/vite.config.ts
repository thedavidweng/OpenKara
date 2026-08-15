import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
import {
  isPreviewCatalogModule,
  slimPreviewCatalogSource,
} from "./src/slim-preview-catalog";

const unusedPreviewModules = [
  "components/Settings/SettingsOverlay",
  "components/Settings/LibrarySetup",
  "components/Settings/ConfirmationDialog",
  "components/Settings/InputDialog",
  "components/Player/QueuePanel",
  "components/Lyrics/LyricsEditDialog",
  "components/Layout/UpdateBanner",
  "components/Layout/GlobalProgressBar",
  "components/Layout/ToastContainer",
  "components/Library/ImportCdgChoiceDialog",
  "components/Bootstrap/ModelBootstrapBanner",
  "components/Bootstrap/RuntimeUpdateBanner",
];

function escapeRegExpLiteral(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function slimPreviewCatalogPlugin(): Plugin {
  return {
    name: "slim-preview-catalog",
    transform(code, id) {
      if (!isPreviewCatalogModule(id)) {
        return null;
      }
      return {
        code: slimPreviewCatalogSource(code),
        map: null,
      };
    },
  };
}

export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  // Serve from the GitHub Pages project subpath (`thedavidweng.github.io/OpenKara/`).
  // No CNAME → no custom domain redirect. The xyz domain stays registered for
  // Dropbox app configuration but no longer serves the site.
  base: "/OpenKara/",
  plugins: [react(), tailwindcss(), slimPreviewCatalogPlugin()],
  resolve: {
    alias: [
      {
        find: /^@\/lib\/lyrics-romanizer$/,
        replacement: fileURLToPath(
          new URL("./src/preview-romanizer.ts", import.meta.url),
        ),
      },
      {
        find: /^@\/lib\/i18n$/,
        replacement: fileURLToPath(
          new URL("./src/preview-i18n.ts", import.meta.url),
        ),
      },
      ...unusedPreviewModules.map((modulePath) => ({
        find: new RegExp(`^@/${escapeRegExpLiteral(modulePath)}$`),
        replacement: fileURLToPath(
          new URL("./src/preview-unused.ts", import.meta.url),
        ),
      })),
      {
        find: "@",
        replacement: fileURLToPath(new URL("../src", import.meta.url)),
      },
    ],
  },
  server: {
    host: "0.0.0.0",
    allowedHosts: ["terminal.local"],
    fs: {
      allow: [fileURLToPath(new URL("..", import.meta.url))],
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    chunkSizeWarningLimit: 1200,
    rollupOptions: {
      input: {
        home: fileURLToPath(new URL("./index.html", import.meta.url)),
        faq: fileURLToPath(new URL("./faq/index.html", import.meta.url)),
        privacy: fileURLToPath(
          new URL("./privacy/index.html", import.meta.url),
        ),
        terms: fileURLToPath(new URL("./terms/index.html", import.meta.url)),
        "zh-faq": fileURLToPath(
          new URL("./zh/faq/index.html", import.meta.url),
        ),
        "zh-privacy": fileURLToPath(
          new URL("./zh/privacy/index.html", import.meta.url),
        ),
        "zh-terms": fileURLToPath(
          new URL("./zh/terms/index.html", import.meta.url),
        ),
      },
    },
  },
});
