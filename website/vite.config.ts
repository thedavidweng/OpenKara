import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
import {
  PREVIEW_I18N_MODULE_PATTERN,
  PREVIEW_ROMANIZER_MODULE_PATTERN,
  UNUSED_PREVIEW_MODULES,
  previewUnusedModulePattern,
} from "./src/preview-aliases";
import {
  isPreviewCatalogModule,
  slimPreviewCatalogSource,
} from "./src/slim-preview-catalog";

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
        find: PREVIEW_ROMANIZER_MODULE_PATTERN,
        replacement: fileURLToPath(
          new URL("./src/preview-romanizer.ts", import.meta.url),
        ),
      },
      {
        find: PREVIEW_I18N_MODULE_PATTERN,
        replacement: fileURLToPath(
          new URL("./src/preview-i18n.ts", import.meta.url),
        ),
      },
      ...UNUSED_PREVIEW_MODULES.map((modulePath) => ({
        find: previewUnusedModulePattern(modulePath),
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
