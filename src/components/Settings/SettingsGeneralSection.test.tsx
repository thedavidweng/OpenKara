// @vitest-environment jsdom

import userEvent from "@testing-library/user-event";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createInitializedSettingsHarness } from "@/test-utils/settings-controller";
import { SettingsControllerContext } from "./SettingsController.context";
import { SettingsGeneralSection } from "./SettingsGeneralSection";

describe("SettingsGeneralSection", () => {
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

  test("renders the appearance radio group with the current theme preference checked", async () => {
    const harness = await createInitializedSettingsHarness({
      settings: { theme_preference: "system" },
    });

    act(() => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <SettingsGeneralSection />
        </SettingsControllerContext>,
      );
    });

    const radios = container.querySelectorAll<HTMLInputElement>(
      'input[name="theme-preference"]',
    );
    expect(radios).toHaveLength(3);
    expect([...radios].find((radio) => radio.value === "system")?.checked).toBe(
      true,
    );
  });

  test("clicking a theme radio writes the preference", async () => {
    const harness = await createInitializedSettingsHarness({
      settings: { theme_preference: "dark" },
    });
    const setThemePreference = vi.spyOn(
      harness.backend.settings,
      "setThemePreference",
    );

    act(() => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <SettingsGeneralSection />
        </SettingsControllerContext>,
      );
    });

    const lightRadio = container.querySelector<HTMLInputElement>(
      'input[name="theme-preference"][value="light"]',
    );
    expect(lightRadio).not.toBeNull();

    await user.click(lightRadio!);

    expect(setThemePreference).toHaveBeenCalledWith("light");
  });
});
