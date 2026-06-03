import React from "react";
import ReactDOM from "react-dom/client";
import "@/lib/i18n";
import App from "./App";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { FullscreenPlayerView } from "@/components/Player/FullscreenPlayerView";
import { applyShellDocumentMarker } from "@/runtime/shell-document";
import "@/styles/globals.css";

const isFullscreenPlayer =
  new URLSearchParams(window.location.search).get("mode") ===
  "fullscreen-player";

applyShellDocumentMarker();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      {isFullscreenPlayer ? <FullscreenPlayerView /> : <App />}
    </ErrorBoundary>
  </React.StrictMode>,
);
