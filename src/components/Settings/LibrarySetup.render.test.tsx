// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const mockOpen = vi.hoisted(() => vi.fn());
const mockCreateLocalLibrary = vi.hoisted(() => vi.fn());
const mockRunRemoteLibraryRegistrationFlow = vi.hoisted(() => vi.fn());
const mockSetLanguage = vi.hoisted(() => vi.fn());
const mockSetStemMode = vi.hoisted(() => vi.fn());
const mockSetModelVariant = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      ({
        "languageNames.en": "English",
        "languageNames.zh-CN": "简体中文",
        "setup.createNew": "Create new local library",
        "setup.openExisting": "Open existing library",
        "setup.useRemoteRepository": "Use remote repository",
        "setup.remoteProvider.googleDrive.title": "Google Drive",
        "settings.library.connectGoogleDrive": "Connect Google Drive",
      })[key] ??
      options?.defaultValue ??
      key,
  }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: mockOpen,
}));

vi.mock("@/lib/tauri", () => ({
  setLanguage: mockSetLanguage,
  createLocalLibrary: mockCreateLocalLibrary,
  registerLocalLibrary: vi.fn(),
  setStemMode: mockSetStemMode,
  setModelVariant: mockSetModelVariant,
  cancelRemoteAuth: vi.fn(),
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: Object.assign(
    (selector: (state: Record<string, unknown>) => unknown) =>
      selector({
        language: "en",
        stemMode: "four_stem",
        modelVariant: "htdemucs",
        patchAppSettings: vi.fn(),
        hydrateAppSettings: vi.fn(),
      }),
    {
      getState: () => ({
        language: "en",
        stemMode: "four_stem",
        modelVariant: "htdemucs",
        patchAppSettings: vi.fn(),
        hydrateAppSettings: vi.fn(),
      }),
    },
  ),
}));

vi.mock("@/lib/i18n", () => ({
  default: {
    resolvedLanguage: "en",
    changeLanguage: vi.fn(),
  },
  SUPPORTED_LANGUAGES: [
    { code: "en", nameKey: "languageNames.en" },
    { code: "zh-CN", nameKey: "languageNames.zh-CN" },
  ],
  detectSystemLanguage: () => "en",
  resolveAppLanguage: (persisted: string | null | undefined) =>
    persisted && persisted.trim().length > 0 ? persisted.trim() : "en",
}));

vi.mock("./remote-library-flow", () => ({
  runRemoteLibraryRegistrationFlow: mockRunRemoteLibraryRegistrationFlow,
}));

import { LibrarySetup } from "./LibrarySetup";

describe("LibrarySetup destructive error surfaces", () => {
  let container: HTMLDivElement;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    mockOpen.mockReset();
    mockCreateLocalLibrary.mockReset();
    mockRunRemoteLibraryRegistrationFlow.mockReset();
    mockSetLanguage.mockReset();
    mockSetLanguage.mockResolvedValue({ language: "en" });
    mockSetStemMode.mockReset();
    mockSetStemMode.mockResolvedValue(undefined);
    mockSetModelVariant.mockReset();
    mockSetModelVariant.mockResolvedValue(undefined);
  });

  afterEach(() => {
    container.remove();
  });

  async function advanceToLibraryStep(root: ReturnType<typeof createRoot>) {
    await act(async () => {
      root.render(<LibrarySetup onComplete={vi.fn()} />);
    });

    const english = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent?.includes("English"),
    );
    expect(english).toBeTruthy();
    await act(async () => {
      english?.click();
    });
  }

  test("counts the remote-provider screen as part of the library step", async () => {
    const root = createRoot(container);
    await advanceToLibraryStep(root);

    const remote = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent?.includes("Use remote repository"),
    );
    await act(async () => {
      remote?.click();
    });

    const dots = Array.from(
      container.querySelectorAll("div.rounded-full.h-1\\.5, div.h-1\\.5"),
    ).filter((dot) => dot.className.includes("rounded-full"));
    expect(dots).toHaveLength(3);
    expect(
      dots.filter((dot) =>
        dot.className.includes("bg-[var(--color-control-primary)]"),
      ),
    ).toHaveLength(2);

    await act(async () => {
      root.unmount();
    });
  });

  test("renders library-step errors with the destructive text token", async () => {
    const root = createRoot(container);
    await advanceToLibraryStep(root);

    mockOpen.mockResolvedValue("/tmp/karaoke");
    mockCreateLocalLibrary.mockRejectedValue(new Error("disk full"));

    const createButton = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent?.includes("Create new local library"),
    );
    expect(createButton).toBeTruthy();

    await act(async () => {
      createButton?.click();
    });

    expect(container.innerHTML).toContain("disk full");
    expect(container.innerHTML).toContain("text-[var(--color-destructive)]");

    await act(async () => {
      root.unmount();
    });
  });

  test("renders remote-provider errors with the destructive text token", async () => {
    const root = createRoot(container);
    await advanceToLibraryStep(root);

    const remoteButton = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent?.includes("Use remote repository"),
    );
    expect(remoteButton).toBeTruthy();
    await act(async () => {
      remoteButton?.click();
    });

    mockRunRemoteLibraryRegistrationFlow.mockRejectedValue(
      new Error("auth failed"),
    );

    const googleDrive = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent?.includes("Google Drive"),
    );
    expect(googleDrive).toBeTruthy();
    await act(async () => {
      googleDrive?.click();
    });

    const connectButton = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent?.includes("Connect Google Drive"),
    );
    expect(connectButton).toBeTruthy();
    await act(async () => {
      connectButton?.click();
    });

    expect(container.innerHTML).toContain("auth failed");
    expect(container.innerHTML).toContain("text-[var(--color-destructive)]");

    await act(async () => {
      root.unmount();
    });
  });

  test("persists the model chosen during first run so the first separation downloads it", async () => {
    const onComplete = vi.fn();
    const root = createRoot(container);

    await act(async () => {
      root.render(<LibrarySetup onComplete={onComplete} />);
    });

    const clickButtonContaining = async (text: string) => {
      const button = Array.from(container.querySelectorAll("button")).find(
        (candidate) => candidate.textContent?.includes(text),
      );
      expect(button, `no button containing ${text}`).toBeTruthy();
      await act(async () => {
        button?.click();
      });
    };

    await clickButtonContaining("English");

    mockOpen.mockResolvedValue("/tmp/karaoke");
    mockCreateLocalLibrary.mockResolvedValue(undefined);
    await clickButtonContaining("Create new local library");

    expect(container.innerHTML).not.toContain("settings.modelVariant.htdemucs");

    await clickButtonContaining("setup.back");
    expect(container.innerHTML).toContain("Create new local library");
    await clickButtonContaining("Create new local library");

    await clickButtonContaining("setup.getStarted");

    expect(mockSetLanguage).toHaveBeenCalled();
    expect(mockSetStemMode).toHaveBeenCalledWith("four_stem");
    expect(mockSetModelVariant).not.toHaveBeenCalled();
    expect(onComplete).toHaveBeenCalled();

    await act(async () => {
      root.unmount();
    });
  });

  test("keeps OOBE active when language save fails on finish", async () => {
    const onComplete = vi.fn();
    const root = createRoot(container);

    await act(async () => {
      root.render(<LibrarySetup onComplete={onComplete} />);
    });

    const clickButtonContaining = async (text: string) => {
      const button = Array.from(container.querySelectorAll("button")).find(
        (candidate) => candidate.textContent?.includes(text),
      );
      expect(button, `no button containing ${text}`).toBeTruthy();
      await act(async () => {
        button?.click();
      });
    };

    await clickButtonContaining("English");
    mockOpen.mockResolvedValue("/tmp/karaoke");
    mockCreateLocalLibrary.mockResolvedValue(undefined);
    await clickButtonContaining("Create new local library");

    mockSetLanguage.mockRejectedValue(new Error("lang save failed"));
    await clickButtonContaining("setup.getStarted");

    expect(container.innerHTML).toContain("lang save failed");
    expect(container.innerHTML).toContain("text-[var(--color-destructive)]");
    expect(onComplete).not.toHaveBeenCalled();
    expect(mockSetStemMode).not.toHaveBeenCalled();

    await act(async () => {
      root.unmount();
    });
  });

  test("keeps OOBE active when stem mode save fails on finish", async () => {
    const onComplete = vi.fn();
    const root = createRoot(container);

    await act(async () => {
      root.render(<LibrarySetup onComplete={onComplete} />);
    });

    const clickButtonContaining = async (text: string) => {
      const button = Array.from(container.querySelectorAll("button")).find(
        (candidate) => candidate.textContent?.includes(text),
      );
      expect(button, `no button containing ${text}`).toBeTruthy();
      await act(async () => {
        button?.click();
      });
    };

    await clickButtonContaining("English");
    mockOpen.mockResolvedValue("/tmp/karaoke");
    mockCreateLocalLibrary.mockResolvedValue(undefined);
    await clickButtonContaining("Create new local library");

    mockSetStemMode.mockRejectedValue(new Error("stem save failed"));
    await clickButtonContaining("setup.getStarted");

    expect(container.innerHTML).toContain("stem save failed");
    expect(onComplete).not.toHaveBeenCalled();

    await act(async () => {
      root.unmount();
    });
  });
});
