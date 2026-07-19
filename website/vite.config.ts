import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("../src", import.meta.url)),
    },
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
