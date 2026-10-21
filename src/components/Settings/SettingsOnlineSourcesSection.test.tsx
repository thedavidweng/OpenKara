// @vitest-environment jsdom

import userEvent from "@testing-library/user-event";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createInitializedSettingsHarness } from "@/test-utils/settings-controller";
import { SettingsControllerContext } from "./SettingsController.context";
import { SettingsOnlineSourcesSection } from "./SettingsOnlineSourcesSection";
vi.mock("./StreamingSources", () => ({
  StreamingSources: () => <div>Source management</div>,
}));

describe("SettingsOnlineSourcesSection", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    user = userEvent.setup();
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
  });

  test("renders the built-in video source off by default", async () => {
    const harness = await createInitializedSettingsHarness();

    act(() => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <SettingsOnlineSourcesSection />
        </SettingsControllerContext>,
      );
    });

    const youtube = container.querySelector<HTMLInputElement>(
      '[data-testid="online-source-youtube"]',
    );
    expect(youtube?.checked).toBe(false);
  });

  test("enabling youtube writes the video source flag", async () => {
    const harness = await createInitializedSettingsHarness();
    const setOnlineSourceEnabled = vi.spyOn(
      harness.backend.settings,
      "setOnlineSourceEnabled",
    );

    act(() => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <SettingsOnlineSourcesSection />
        </SettingsControllerContext>,
      );
    });

    const youtube = container.querySelector<HTMLInputElement>(
      '[data-testid="online-source-youtube"]',
    );
    expect(youtube).not.toBeNull();
    await user.click(youtube!);

    expect(setOnlineSourceEnabled).toHaveBeenCalledWith("youtube", true);
  });
});
