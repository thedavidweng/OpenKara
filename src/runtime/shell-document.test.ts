// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { applyShellDocumentMarker } from "./shell-document";

describe("applyShellDocumentMarker", () => {
  const originalUrl = window.location.href;

  beforeEach(() => {
    document.documentElement.removeAttribute("data-app-shell");
    document.documentElement.removeAttribute("data-presentation-mode");
    document.body.innerHTML = '<div id="root"></div>';
    document.body.removeAttribute("data-app-shell");
  });

  afterEach(() => {
    window.history.replaceState({}, "", originalUrl);
  });

  test("always stamps full-app on html, body, and root", () => {
    applyShellDocumentMarker();

    expect(document.documentElement.dataset.appShell).toBe("full-app");
    expect(document.body.dataset.appShell).toBe("full-app");
    expect(document.getElementById("root")?.dataset.appShell).toBe("full-app");
  });

  test("stamps audience presentation mode for fullscreen-player URL", () => {
    window.history.replaceState({}, "", "/?mode=fullscreen-player");

    applyShellDocumentMarker();

    expect(document.documentElement.dataset.presentationMode).toBe("audience");
  });

  test("does not stamp audience presentation mode for the host URL", () => {
    window.history.replaceState({}, "", "/");

    applyShellDocumentMarker();

    expect(document.documentElement.dataset.presentationMode).toBeUndefined();
  });
});
