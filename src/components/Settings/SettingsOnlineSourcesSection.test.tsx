// @vitest-environment jsdom

import userEvent from "@testing-library/user-event";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createInitializedSettingsHarness } from "@/test-utils/settings-controller";
import { SettingsControllerContext } from "./SettingsController.context";
import { SettingsOnlineSourcesSection } from "./SettingsOnlineSourcesSection";

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

  test("renders both online sources off by default", async () => {
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
    const netease = container.querySelector<HTMLInputElement>(
      '[data-testid="online-source-netease"]',
    );
    expect(youtube?.checked).toBe(false);
    expect(netease?.checked).toBe(false);
  });

  test("enabling netease writes the streaming source flag", async () => {
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

    const netease = container.querySelector<HTMLInputElement>(
      '[data-testid="online-source-netease"]',
    );
    expect(netease).not.toBeNull();
    await user.click(netease!);

    expect(setOnlineSourceEnabled).toHaveBeenCalledWith("netease", true);
  });
});
