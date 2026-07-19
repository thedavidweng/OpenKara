/** The document shell marker is always full-app; host uses a single webview tree. */
export function applyShellDocumentMarker() {
  const shellMode = "full-app";

  document.documentElement.dataset.appShell = shellMode;
  document.body.dataset.appShell = shellMode;
  document.getElementById("root")?.setAttribute("data-app-shell", shellMode);

  // The fullscreen/audience stage retains its explicit dark presentation
  // regardless of the primary theme preference. The marker is set before
  // React render so the audience color-scheme is correct on first paint.
  const isFullscreenPlayer =
    new URLSearchParams(window.location.search).get("mode") ===
    "fullscreen-player";
  if (isFullscreenPlayer) {
    document.documentElement.dataset.presentationMode = "audience";
  }
}
