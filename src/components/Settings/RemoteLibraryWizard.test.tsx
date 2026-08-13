// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { createInitializedSettingsHarness } from "@/test-utils/settings-controller";
import { RemoteLibraryWizard } from "./RemoteLibraryWizard";
import { SettingsControllerContext } from "./SettingsController.context";

const {
  mockBeginRemoteAuth,
  mockCancelRemoteAuth,
  mockPollRemoteAuth,
  mockCreateRemoteLibrary,
  mockResolveRemoteLibraryCandidate,
  mockOpenExternalUrl,
  mockRegisterRemoteLibrary,
  mockReauthorizeRemoteRepository,
  mockRelocateRemoteRepository,
} = vi.hoisted(() => ({
  mockBeginRemoteAuth: vi.fn(),
  mockCancelRemoteAuth: vi.fn(),
  mockPollRemoteAuth: vi.fn(),
  mockCreateRemoteLibrary: vi.fn(),
  mockResolveRemoteLibraryCandidate: vi.fn(),
  mockOpenExternalUrl: vi.fn(),
  mockRegisterRemoteLibrary: vi.fn(),
  mockReauthorizeRemoteRepository: vi.fn(),
  mockRelocateRemoteRepository: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  initReactI18next: {
    type: "3rdParty",
    init: () => {},
  },
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (key === "settings.library.mirrorActiveLocalDescriptionWithName") {
        return `settings.library.mirrorActiveLocalDescriptionWithName:${String(options?.displayName ?? "")}`;
      }

      return key;
    },
  }),
}));

vi.mock("@/lib/backend", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/backend")>()),
  useBackend: () => backend,
}));

const backend = createMockBackend({
  overrides: {
    remoteRepository: {
      beginRemoteAuth: mockBeginRemoteAuth,
      cancelRemoteAuth: mockCancelRemoteAuth,
      pollRemoteAuth: mockPollRemoteAuth,
      createRemoteLibrary: mockCreateRemoteLibrary,
      resolveRemoteLibraryCandidate: mockResolveRemoteLibraryCandidate,
      openExternalUrl: mockOpenExternalUrl,
      registerRemoteLibrary: mockRegisterRemoteLibrary,
      reauthorizeRemoteRepository: mockReauthorizeRemoteRepository,
      relocateRemoteRepository: mockRelocateRemoteRepository,
    },
  },
});

async function flushEffects() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await new Promise((resolve) => window.setTimeout(resolve, 0));
  });
}

function setInputValue(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )?.set;
  setter?.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("RemoteLibraryWizard", () => {
  beforeEach(() => {
    mockBeginRemoteAuth.mockReset();
    mockCancelRemoteAuth.mockReset();
    mockPollRemoteAuth.mockReset();
    mockCreateRemoteLibrary.mockReset();
    mockResolveRemoteLibraryCandidate.mockReset();
    mockOpenExternalUrl.mockReset();
    mockRegisterRemoteLibrary.mockReset();
    mockReauthorizeRemoteRepository.mockReset();
    mockRelocateRemoteRepository.mockReset();
    vi.restoreAllMocks();

    (
      globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
  });

  test("uses translation keys instead of hardcoded remote-library English copy", async () => {
    const harness = await createInitializedSettingsHarness();

    const markup = renderToStaticMarkup(
      <SettingsControllerContext value={harness.controller}>
        <RemoteLibraryWizard onClose={() => {}} />
      </SettingsControllerContext>,
    );

    expect(markup).toContain("settings.library.openRemoteLibrary");
    expect(markup).toContain("settings.library.connectRemoteGoogleDrive");
    expect(markup).toContain("settings.library.displayName");
    expect(markup).not.toContain("Open Remote Library");
    expect(markup).not.toContain("Display name");
    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('aria-modal="true"');
    expect(markup).toContain('aria-labelledby="remote-library-wizard-title"');
    expect(markup).toContain('for="remote-library-display-name"');
    expect(markup).toContain('id="remote-library-display-name"');
  });

  test("labels the primary action per provider instead of a generic open label", async () => {
    const harness = await createInitializedSettingsHarness();

    const dropboxMarkup = renderToStaticMarkup(
      <SettingsControllerContext value={harness.controller}>
        <RemoteLibraryWizard onClose={() => {}} initialProvider="dropbox" />
      </SettingsControllerContext>,
    );
    expect(dropboxMarkup).toContain("settings.library.connectRemoteDropbox");

    const webdavMarkup = renderToStaticMarkup(
      <SettingsControllerContext value={harness.controller}>
        <RemoteLibraryWizard onClose={() => {}} initialProvider="webdav" />
      </SettingsControllerContext>,
    );
    expect(webdavMarkup).toContain("settings.library.connectRemoteWebdav");
  });

  test("renders reauthorization copy and preselects the requested provider", async () => {
    const harness = await createInitializedSettingsHarness();

    const markup = renderToStaticMarkup(
      <SettingsControllerContext value={harness.controller}>
        <RemoteLibraryWizard
          onClose={() => {}}
          initialProvider="webdav"
          purpose="reauthorize"
        />
      </SettingsControllerContext>,
    );

    expect(markup).toContain("settings.library.reauthorizeRemoteRepository");
    expect(markup).toContain(
      "settings.library.reauthorizeRemoteRepositoryDescription",
    );
    expect(markup).toContain("settings.library.webdavServerUrl");
  });

  test("reauthorizes an existing remote repository without registering a new one when the location is unchanged", async () => {
    mockBeginRemoteAuth.mockResolvedValue({
      session_id: "session-1",
      provider: "webdav",
      authorization_url: null,
      expires_at_ms: null,
    });
    mockResolveRemoteLibraryCandidate.mockResolvedValue({
      provider: "webdav",
      remote_root_locator: "https://dav.example.com/OpenKara/",
      remote_path_display: "dav.example.com/OpenKara",
      display_name: "Drive",
      account_id: "user@dav.example.com",
    });
    mockReauthorizeRemoteRepository.mockResolvedValue({
      active_library_id: "remote:existing",
      libraries: [],
    });
    const onClose = vi.fn();
    const harness = await createInitializedSettingsHarness({
      activeLibraryId: "remote:existing",
      libraries: [
        {
          id: "remote:existing",
          kind: "remote",
          display_name: "Drive",
          provider: "webdav",
          remote_root_locator: "https://dav.example.com/OpenKara/",
          remote_path_display: "dav.example.com/OpenKara",
          account_id: "user@dav.example.com",
          connection_config: {
            type: "webdav",
            server_url: "https://dav.example.com/",
          },
          cached_db_path: "/tmp/openkara.db",
          remote_revision: "rev-1",
        },
      ],
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <RemoteLibraryWizard
            onClose={onClose}
            libraryId="remote:existing"
            initialProvider="webdav"
            initialDisplayName="Drive"
            initialServerUrl="https://dav.example.com/"
            initialRemoteRootLocator="https://dav.example.com/OpenKara/"
            initialRemotePathDisplay="dav.example.com/OpenKara"
            initialRootPath="/OpenKara"
            purpose="reauthorize"
          />
        </SettingsControllerContext>,
      );
    });

    const inputs = [...container.querySelectorAll("input")];
    await act(async () => {
      setInputValue(inputs[3], "user");
      setInputValue(inputs[4], "secret");
    });

    const reauthorizeButton = [...container.querySelectorAll("button")].find(
      (button) =>
        button.textContent?.includes(
          "settings.library.reauthorizeRemoteRepository",
        ),
    );
    expect(reauthorizeButton).toBeTruthy();

    await act(async () => {
      reauthorizeButton?.click();
    });
    await flushEffects();

    expect(mockReauthorizeRemoteRepository).toHaveBeenCalledWith(
      "remote:existing",
      "session-1",
      "https://dav.example.com/OpenKara/",
      "Drive",
    );
    expect(mockRegisterRemoteLibrary).not.toHaveBeenCalled();
    expect(harness.librarySession.calls).toEqual([
      { entry: "switchLibrary", libraryId: "remote:existing" },
    ]);
    expect(onClose).toHaveBeenCalled();

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("asks before relocating a remote repository when reauthorization returns a different location", async () => {
    mockBeginRemoteAuth.mockResolvedValue({
      session_id: "session-1",
      provider: "webdav",
      authorization_url: null,
      expires_at_ms: null,
    });
    mockResolveRemoteLibraryCandidate.mockResolvedValue({
      provider: "webdav",
      remote_root_locator: "https://dav.example.com/MovedOpenKara/",
      remote_path_display: "dav.example.com/MovedOpenKara",
      display_name: "Drive",
      account_id: "user@dav.example.com",
    });
    mockReauthorizeRemoteRepository.mockResolvedValue({
      active_library_id: "remote:existing",
      libraries: [],
    });
    mockRelocateRemoteRepository.mockResolvedValue({
      active_library_id: "remote:existing",
      libraries: [],
    });
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const harness = await createInitializedSettingsHarness();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <RemoteLibraryWizard
            onClose={() => {}}
            libraryId="remote:existing"
            initialProvider="webdav"
            initialDisplayName="Drive"
            initialServerUrl="https://dav.example.com/"
            initialRemoteRootLocator="https://dav.example.com/OpenKara/"
            initialRemotePathDisplay="dav.example.com/OpenKara"
            initialRootPath="/OpenKara"
            purpose="reauthorize"
          />
        </SettingsControllerContext>,
      );
    });

    const inputs = [...container.querySelectorAll("input")];
    await act(async () => {
      setInputValue(inputs[3], "user");
      setInputValue(inputs[4], "secret");
    });

    const reauthorizeButton = [...container.querySelectorAll("button")].find(
      (button) =>
        button.textContent?.includes(
          "settings.library.reauthorizeRemoteRepository",
        ),
    );

    await act(async () => {
      reauthorizeButton?.click();
    });
    await flushEffects();

    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining(
        "settings.library.confirmRemoteRepositoryRelocation",
      ),
    );
    expect(mockReauthorizeRemoteRepository).toHaveBeenCalledWith(
      "remote:existing",
      "session-1",
      "https://dav.example.com/MovedOpenKara/",
      "Drive",
    );
    expect(mockRelocateRemoteRepository).toHaveBeenCalledWith(
      "remote:existing",
      "session-1",
      "https://dav.example.com/MovedOpenKara/",
      "Drive",
    );

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("shows structured command error messages instead of [object Object]", async () => {
    mockBeginRemoteAuth.mockResolvedValue({
      session_id: "session-1",
      provider: "google_drive",
      authorization_url: null,
      expires_at_ms: null,
    });
    mockCreateRemoteLibrary.mockRejectedValue({
      code: "internal",
      message: "Google Drive folder creation failed.",
      retryable: false,
    });

    const harness = await createInitializedSettingsHarness();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <RemoteLibraryWizard onClose={() => {}} />
        </SettingsControllerContext>,
      );
    });

    const openRemoteButtons = [...container.querySelectorAll("button")].filter(
      (button) =>
        button.textContent?.includes(
          "settings.library.connectRemoteGoogleDrive",
        ),
    );
    const connectButton = openRemoteButtons[openRemoteButtons.length - 1];
    expect(connectButton).toBeTruthy();

    await act(async () => {
      connectButton?.click();
    });
    await flushEffects();

    expect(container.textContent).toContain(
      "Google Drive folder creation failed.",
    );
    expect(container.textContent).not.toContain("[object Object]");

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("keeps the close button enabled while remote auth is pending", async () => {
    let resolveAuth!: (value: unknown) => void;
    mockBeginRemoteAuth.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveAuth = resolve;
        }),
    );

    const harness = await createInitializedSettingsHarness();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <RemoteLibraryWizard onClose={() => {}} />
        </SettingsControllerContext>,
      );
    });

    const buttons = [...container.querySelectorAll("button")];
    const closeButton = buttons[0];
    const openRemoteButtons = buttons.filter((button) =>
      button.textContent?.includes("settings.library.connectRemoteGoogleDrive"),
    );
    const connectButton = openRemoteButtons[openRemoteButtons.length - 1];
    expect(closeButton).toBeTruthy();
    expect(connectButton).toBeTruthy();

    await act(async () => {
      connectButton?.click();
      await Promise.resolve();
    });

    expect(closeButton?.hasAttribute("disabled")).toBe(false);

    if (resolveAuth) {
      await act(async () => {
        resolveAuth({
          session_id: "session-1",
          provider: "google_drive",
          authorization_url: null,
          expires_at_ms: null,
        });
      });
    }

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("cancels pending auth when the wizard closes", async () => {
    mockBeginRemoteAuth.mockResolvedValue({
      session_id: "session-1",
      provider: "dropbox",
      authorization_url: "https://example.com/oauth",
      expires_at_ms: null,
    });
    mockOpenExternalUrl.mockResolvedValue(undefined);

    const onClose = vi.fn();
    const harness = await createInitializedSettingsHarness();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <RemoteLibraryWizard onClose={onClose} />
        </SettingsControllerContext>,
      );
    });

    const buttons = [...container.querySelectorAll("button")];
    const closeButton = buttons[0];
    const openRemoteButtons = buttons.filter((button) =>
      button.textContent?.includes("settings.library.connectRemoteGoogleDrive"),
    );
    const connectButton = openRemoteButtons[openRemoteButtons.length - 1];

    await act(async () => {
      connectButton?.click();
      await Promise.resolve();
      await Promise.resolve();
    });

    await act(async () => {
      closeButton?.click();
      await Promise.resolve();
    });

    expect(mockCancelRemoteAuth).toHaveBeenCalledWith("session-1");
    expect(onClose).toHaveBeenCalled();

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("cancels pending auth when the wizard unmounts", async () => {
    mockBeginRemoteAuth.mockResolvedValue({
      session_id: "session-1",
      provider: "dropbox",
      authorization_url: "https://example.com/oauth",
      expires_at_ms: null,
    });
    mockOpenExternalUrl.mockResolvedValue(undefined);

    const harness = await createInitializedSettingsHarness();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <RemoteLibraryWizard onClose={() => {}} />
        </SettingsControllerContext>,
      );
    });

    const openRemoteButtons = [...container.querySelectorAll("button")].filter(
      (button) =>
        button.textContent?.includes(
          "settings.library.connectRemoteGoogleDrive",
        ),
    );
    const connectButton = openRemoteButtons[openRemoteButtons.length - 1];

    await act(async () => {
      connectButton?.click();
      await Promise.resolve();
      await Promise.resolve();
    });

    await act(async () => {
      root.unmount();
      await Promise.resolve();
    });

    expect(mockCancelRemoteAuth).toHaveBeenCalledWith("session-1");
    container.remove();
  });

  test("opens OAuth URLs through the dedicated desktop opener", async () => {
    mockBeginRemoteAuth.mockResolvedValue({
      session_id: "session-1",
      provider: "dropbox",
      authorization_url: "https://example.com/oauth",
      expires_at_ms: null,
    });
    mockOpenExternalUrl.mockResolvedValue(undefined);
    mockPollRemoteAuth.mockResolvedValue({
      session_id: "session-1",
      provider: "dropbox",
      state: "failed",
      remote_root_locator: null,
      display_name: null,
      error: { code: "internal", message: "stop", retryable: false },
    });

    const harness = await createInitializedSettingsHarness();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <RemoteLibraryWizard onClose={() => {}} />
        </SettingsControllerContext>,
      );
    });

    const openRemoteButtons = [...container.querySelectorAll("button")].filter(
      (button) =>
        button.textContent?.includes(
          "settings.library.connectRemoteGoogleDrive",
        ),
    );
    const connectButton = openRemoteButtons[openRemoteButtons.length - 1];

    await act(async () => {
      connectButton?.click();
    });
    await flushEffects();

    expect(mockOpenExternalUrl).toHaveBeenCalledWith(
      "https://example.com/oauth",
    );

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("does not report success or close when activation fails after registration", async () => {
    mockBeginRemoteAuth.mockResolvedValue({
      session_id: "session-1",
      provider: "google_drive",
      authorization_url: null,
      expires_at_ms: null,
    });
    mockCreateRemoteLibrary.mockResolvedValue({
      provider: "google_drive",
      remote_root_locator: "root-1",
      remote_path_display: "Google Drive / OpenKara",
      display_name: "Drive",
      account_id: "account-1",
    });
    mockRegisterRemoteLibrary.mockResolvedValue({
      active_library_id: "remote:new",
      libraries: [],
    });

    const onClose = vi.fn();
    const harness = await createInitializedSettingsHarness();
    harness.librarySession.failOn(
      "switchLibrary",
      new Error("endpoint unreachable"),
    );
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <RemoteLibraryWizard onClose={onClose} />
        </SettingsControllerContext>,
      );
    });

    const openRemoteButtons = [...container.querySelectorAll("button")].filter(
      (button) =>
        button.textContent?.includes(
          "settings.library.connectRemoteGoogleDrive",
        ),
    );
    const connectButton = openRemoteButtons[openRemoteButtons.length - 1];

    await act(async () => {
      connectButton?.click();
    });
    await flushEffects();

    expect(onClose).not.toHaveBeenCalled();
    expect(container.textContent).toContain("endpoint unreachable");
    expect(container.textContent).not.toContain(
      "settings.library.remoteLibraryConnected",
    );

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("keeps the wizard open and shows the error when activating an existing library fails", async () => {
    const onClose = vi.fn();
    const harness = await createInitializedSettingsHarness({
      activeLibraryId: "local:/karaoke",
      libraries: [
        {
          id: "local:/karaoke",
          kind: "local",
          display_name: "Main Library",
          root_path: "/karaoke",
        },
        {
          id: "remote:drive-1",
          kind: "remote",
          display_name: "Drive",
          provider: "google_drive",
          account_id: "account-1",
          remote_root_locator: "root-1",
          remote_path_display: "Google Drive / OpenKara",
          connection_config: null,
          cached_db_path: null,
          remote_revision: "rev-1",
        },
      ],
    });
    harness.librarySession.failOn(
      "switchLibrary",
      new Error("endpoint unreachable"),
    );
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <RemoteLibraryWizard onClose={onClose} />
        </SettingsControllerContext>,
      );
    });

    const existingLibraryButton = [...container.querySelectorAll("button p")]
      .find((paragraph) => paragraph.textContent === "Drive")
      ?.closest("button");
    expect(existingLibraryButton).toBeTruthy();

    await act(async () => {
      existingLibraryButton?.click();
    });
    await flushEffects();

    expect(onClose).not.toHaveBeenCalled();
    expect(container.textContent).toContain("endpoint unreachable");

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("closes the wizard when activating an existing library succeeds", async () => {
    const onClose = vi.fn();
    const harness = await createInitializedSettingsHarness({
      activeLibraryId: "local:/karaoke",
      libraries: [
        {
          id: "local:/karaoke",
          kind: "local",
          display_name: "Main Library",
          root_path: "/karaoke",
        },
        {
          id: "remote:drive-1",
          kind: "remote",
          display_name: "Drive",
          provider: "google_drive",
          account_id: "account-1",
          remote_root_locator: "root-1",
          remote_path_display: "Google Drive / OpenKara",
          connection_config: null,
          cached_db_path: null,
          remote_revision: "rev-1",
        },
      ],
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <RemoteLibraryWizard onClose={onClose} />
        </SettingsControllerContext>,
      );
    });

    const existingLibraryButton = [...container.querySelectorAll("button p")]
      .find((paragraph) => paragraph.textContent === "Drive")
      ?.closest("button");

    await act(async () => {
      existingLibraryButton?.click();
    });
    await flushEffects();

    expect(harness.librarySession.calls).toEqual([
      { entry: "switchLibrary", libraryId: "remote:drive-1" },
    ]);
    expect(onClose).toHaveBeenCalledOnce();

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("cancels auth if opening the external browser fails", async () => {
    mockBeginRemoteAuth.mockResolvedValue({
      session_id: "session-1",
      provider: "dropbox",
      authorization_url: "https://example.com/oauth",
      expires_at_ms: null,
    });
    mockOpenExternalUrl.mockRejectedValue(new Error("browser open failed"));

    const harness = await createInitializedSettingsHarness();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <RemoteLibraryWizard onClose={() => {}} />
        </SettingsControllerContext>,
      );
    });

    const openRemoteButtons = [...container.querySelectorAll("button")].filter(
      (button) =>
        button.textContent?.includes(
          "settings.library.connectRemoteGoogleDrive",
        ),
    );
    const connectButton = openRemoteButtons[openRemoteButtons.length - 1];

    await act(async () => {
      connectButton?.click();
    });
    await flushEffects();

    expect(mockCancelRemoteAuth).toHaveBeenCalledWith("session-1");
    expect(container.textContent).toContain("browser open failed");

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });
});
