// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { renderWithBackend } from "@/test-utils/backend";
import type { DebugInfo } from "@/types/ipc";
import { SettingsAboutSection } from "./SettingsAboutSection";

const { mockGetDebugInfo, mockNotifyError } = vi.hoisted(() => ({
  mockGetDebugInfo: vi.fn(),
  mockNotifyError: vi.fn(),
}));

const backend = createMockBackend({
  overrides: { settings: { getDebugInfo: mockGetDebugInfo } },
});

function render(ui: ReactElement) {
  return renderWithBackend(ui, backend);
}

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({ t: (key: string) => key }),
  };
});

vi.mock("@/lib/errors", () => ({ notifyError: mockNotifyError }));

const sample: DebugInfo = {
  app_version: "0.9.1",
  build_sha: "abc1234",
  os: "macos",
  arch: "aarch64",
  catalog_generation: 9,
  catalog_release_id: "2026-07-25-006",
  model_variant: "htdemucs",
  model_state: "ready",
  model_installed: true,
  model_installed_version: "model-v2.1.0",
  model_pinned_version: "model-v2.1.0",
  model_path: "/tmp/models/htdemucs.onnx",
  runtime_state: "ready",
  runtime_version: "v1.27.1",
  runtime_artifact_id: "onnxruntime-1.27.1-openkara-aarch64-apple-darwin",
  runtime_target_triple: "aarch64-apple-darwin",
  runtime_path: "/tmp/runtimes/rt/libonnxruntime.dylib",
  execution_provider: "xnnpack",
  directml_available: false,
  language: "en",
  log_file: "/Users/me/Library/Logs/com.openkara.desktop/openkara.<date>.log",
};

let writeText: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.clearAllMocks();
  writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
});

afterEach(() => {
  cleanup();
});

describe("SettingsAboutSection", () => {
  test("renders version, system, and log path from the snapshot", async () => {
    mockGetDebugInfo.mockResolvedValue(sample);

    const { container } = render(<SettingsAboutSection />);

    await waitFor(() => {
      expect(mockGetDebugInfo).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(container.textContent).toContain("0.9.1");
    });
    expect(container.textContent).toContain("abc1234");
    expect(container.textContent).toContain("macos");
    expect(container.textContent).toContain("aarch64");
    expect(container.textContent).toContain("2026-07-25-006");
    expect(container.textContent).toContain("xnnpack");
    expect(container.textContent).toContain("openkara.<date>.log");
  });

  test("copies formatted debug info to the clipboard", async () => {
    mockGetDebugInfo.mockResolvedValue(sample);

    render(<SettingsAboutSection />);
    await waitFor(() => {
      expect(mockGetDebugInfo).toHaveBeenCalled();
    });

    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "settings.about.copyDebugInfo" }),
      );
    });

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledOnce();
    });
    const copied = writeText.mock.calls[0][0] as string;
    expect(copied).toContain("app.name · settings.about.label");
    expect(copied).toContain("0.9.1");
    expect(copied).toContain("v1.27.1");
  });

  test("reports an error when copying fails", async () => {
    mockGetDebugInfo.mockResolvedValue(sample);
    writeText.mockRejectedValueOnce(new Error("clipboard blocked"));

    render(<SettingsAboutSection />);
    await waitFor(() => {
      expect(mockGetDebugInfo).toHaveBeenCalled();
    });

    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "settings.about.copyDebugInfo" }),
      );
    });

    await waitFor(() => {
      expect(mockNotifyError).toHaveBeenCalledOnce();
    });
  });

  test("still renders placeholders when the fetch fails", async () => {
    mockGetDebugInfo.mockRejectedValue(new Error("ipc down"));

    const { container } = render(<SettingsAboutSection />);

    await waitFor(() => {
      expect(mockGetDebugInfo).toHaveBeenCalled();
    });
    expect(
      screen.getByRole("button", { name: "settings.about.copyDebugInfo" }),
    ).toBeTruthy();
    expect(container.textContent).toContain("—");
  });
});
