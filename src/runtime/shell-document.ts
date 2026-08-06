export function applyShellDocumentMarker() {
  const shellMode = "full-app";

  document.documentElement.dataset.appShell = shellMode;
  document.body.dataset.appShell = shellMode;
  document.getElementById("root")?.setAttribute("data-app-shell", shellMode);

  const isFullscreenPlayer =
    new URLSearchParams(window.location.search).get("mode") ===
    "fullscreen-player";
  if (isFullscreenPlayer) {
    document.documentElement.dataset.presentationMode = "audience";
  }
}
